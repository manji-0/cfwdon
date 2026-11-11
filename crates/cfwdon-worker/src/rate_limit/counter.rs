use super::CounterHitRequest;
use cfwdon_domain::rate_limit::{RateLimitCounterTable, count_hits, table_expires_at_ms};
use std::cell::{Cell, RefCell};
use worker::{DurableObject, Env, Method, Request, Response, Result, State, durable_object};

const TABLE_KEY: &str = "counters";

/// One rate-limit subject's counters (`RateLimitSubject::key` names the
/// instance: `ip:…`, `account:…`, `token:…`, `domain:…`), so no one instance
/// sees more than one subject's traffic. A Durable Object is the one place a
/// subject's count is consistent across isolates and locations, which Workers
/// KV (eventually consistent, one write per second per key) and the Rate
/// Limiting binding (10 or 60 second windows, counted per location, no
/// remaining count) are not, and it keeps D1 off the hot path.
///
/// The table (a few counters) is kept in memory and written back on every
/// hit, so an evicted instance resumes where it was; an alarm at the last
/// window's end deletes it, so subjects seen once leave nothing behind.
#[durable_object]
pub struct RateLimitCounter {
    state: State,
    table: RefCell<Option<RateLimitCounterTable>>,
    alarm_at_ms: Cell<Option<u64>>,
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
            self.alarm_at_ms.set(None);
            return storage.delete_all().await;
        };
        let raw = serde_json::to_string(&table)
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
        self.table.replace(Some(table));
        storage.put(TABLE_KEY, raw).await?;
        if self
            .alarm_at_ms
            .get()
            .is_none_or(|alarm_at_ms| expires_at_ms > alarm_at_ms)
        {
            self.alarm_at_ms.set(Some(expires_at_ms));
            storage
                .set_alarm(i64::try_from(expires_at_ms).unwrap_or(i64::MAX))
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
            alarm_at_ms: Cell::new(None),
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

    /// Drop the windows that have ended; the instance's storage goes with the last.
    async fn alarm(&self) -> Result<Response> {
        self.alarm_at_ms.set(None);
        let mut table = self.load().await;
        count_hits(&mut table, &[], js_sys::Date::now() as u64);
        self.save(table).await?;
        Response::ok("")
    }
}
