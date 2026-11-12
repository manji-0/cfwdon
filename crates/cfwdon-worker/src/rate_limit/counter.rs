use super::CounterHitRequest;
use cfwdon_domain::rate_limit::{RateLimitCounterTable, count_hits, table_expires_at_ms};
use std::cell::{Cell, RefCell};
use worker::{DurableObject, Env, Method, Request, Response, Result, State, durable_object};

const TABLE_KEY: &str = "counters";
/// How long after its last window ends an idle subject's storage stays. A
/// subject that keeps counting moves its alarm at most once per this long.
const CLEANUP_DELAY_MS: u64 = 60 * 60 * 1000;

/// One rate-limit subject's counters (`RateLimitSubject::key` names the
/// instance: `ip:…`, `account:…`, `token:…`, `domain:…`), so no one instance
/// sees more than one subject's traffic. A Durable Object is the one place a
/// subject's count is consistent across isolates and locations, which Workers
/// KV (eventually consistent, one write per second per key) and the Rate
/// Limiting binding (10 or 60 second windows, counted per location, no
/// remaining count) are not, and it keeps D1 off the hot path.
///
/// The table (a few counters) is kept in memory and written back on every
/// hit, so an evicted instance resumes where it was. Ended windows are dropped
/// on the next hit; an alarm, armed once and left alone while it is pending,
/// deletes the storage of a subject idle for `CLEANUP_DELAY_MS`.
#[durable_object]
pub struct RateLimitCounter {
    state: State,
    table: RefCell<Option<RateLimitCounterTable>>,
    alarm_pending: Cell<bool>,
}

impl RateLimitCounter {
    async fn load(&self) -> RateLimitCounterTable {
        if let Some(table) = self.table.borrow().as_ref() {
            return table.clone();
        }
        let stored = self
            .state
            .storage()
            .get::<String>(TABLE_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // Another event may have filled the table while storage was read.
        self.table.borrow_mut().get_or_insert(stored).clone()
    }

    async fn save(&self, table: RateLimitCounterTable) -> Result<()> {
        let storage = self.state.storage();
        let Some(expires_at_ms) = table_expires_at_ms(&table) else {
            self.table.replace(Some(table));
            self.alarm_pending.set(false);
            return storage.delete_all().await;
        };
        let raw = serde_json::to_string(&table)
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
        self.table.replace(Some(table));
        storage.put(TABLE_KEY, raw).await?;
        if !self.alarm_pending.replace(true) {
            let alarm_at_ms = expires_at_ms.saturating_add(CLEANUP_DELAY_MS);
            storage
                .set_alarm(i64::try_from(alarm_at_ms).unwrap_or(i64::MAX))
                .await?;
        }
        Ok(())
    }
}

impl DurableObject for RateLimitCounter {
    fn new(state: State, _env: Env) -> Self {
        Self {
            state,
            table: RefCell::new(None),
            alarm_pending: Cell::new(false),
        }
    }

    /// `POST /hit` with `{hits, now_ms}` (the caller's clock): count each hit
    /// and answer each counter's state after it, in order.
    async fn fetch(&self, mut req: Request) -> Result<Response> {
        if req.method() != Method::Post {
            return Response::error("Method Not Allowed", 405);
        }
        let request = req.json::<CounterHitRequest>().await?;
        let mut table = self.load().await;
        // No await between reading the table and keeping the new one, so
        // concurrent hits on this instance cannot lose a count.
        let states = count_hits(&mut table, &request.hits, request.now_ms);
        self.save(table).await?;
        Response::from_json(&states)
    }

    /// Drop the windows that have ended: the instance's storage goes with the
    /// last, and a subject still counting gets the next alarm.
    async fn alarm(&self) -> Result<Response> {
        self.alarm_pending.set(false);
        let mut table = self.load().await;
        count_hits(&mut table, &[], js_sys::Date::now() as u64);
        self.save(table).await?;
        Response::ok("")
    }
}
