//! Strip the metadata from media stored before uploads were stripped (see
//! `migrations/127_media_metadata_backfill.sql`). Each hourly run takes a page
//! of queued object keys, rewrites the objects that carry metadata in place,
//! and deletes their rows; a key whose read or write fails stays queued for
//! the next run.

use crate::db_utils::d1_results;
use crate::observability::log_json_event;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{MalformedMedia, restripped_stored_media};
use serde::Deserialize;
use worker::{Bucket, Conditional, Result, d1::D1Type};

/// Keys listed per run.
const PAGE_ROWS: i32 = 25;
const MIB: u64 = 1024 * 1024;
/// Bytes read per run; once spent, the rest of the page waits for the next run.
const PAGE_BYTES: u64 = 64 * MIB;
/// An object is read whole and its rewrite is a copy, so one object costs
/// about twice its size in memory (Workers allow 128 MB). Every image upload
/// fits; a larger video is left as it is and logged.
const OBJECT_BYTES: u64 = 32 * MIB;

#[derive(Debug, Deserialize)]
struct QueuedObjectRow {
    object_key: String,
}

/// What happened to one stored object.
enum Outcome {
    Stripped,
    Unchanged,
    Missing,
    Unreadable(MalformedMedia),
    TooLarge(u64),
    /// Reading or writing failed, or the object changed since it was read.
    Deferred(String),
}

impl Outcome {
    const fn label(&self) -> &'static str {
        match self {
            Self::Stripped => "stripped",
            Self::Unchanged => "unchanged",
            Self::Missing => "missing",
            Self::Unreadable(_) => "unreadable",
            Self::TooLarge(_) => "too_large",
            Self::Deferred(_) => "deferred",
        }
    }
}

/// One page of the backfill.
pub(crate) async fn strip_stored_media_metadata_page(
    db: &D1Database,
    bucket: &Bucket,
) -> Result<()> {
    let result = db
        .prepare("SELECT object_key FROM media_metadata_backfill ORDER BY object_key LIMIT ?1")
        .bind_refs(&D1Type::Integer(PAGE_ROWS))?
        .all()
        .await?;
    let rows = d1_results::<QueuedObjectRow>(&result)?;
    let mut bytes_read = 0;
    for row in rows {
        if bytes_read >= PAGE_BYTES {
            break;
        }
        let (outcome, read) = strip_stored_object(bucket, &row.object_key).await;
        bytes_read += read;
        log_outcome(&row.object_key, &outcome);
        if !matches!(outcome, Outcome::Deferred(_)) {
            db.prepare("DELETE FROM media_metadata_backfill WHERE object_key = ?1")
                .bind_refs(&D1Type::Text(&row.object_key))?
                .run()
                .await?;
        }
    }
    Ok(())
}

/// Strip one object in place; also returns how many bytes were read.
async fn strip_stored_object(bucket: &Bucket, object_key: &str) -> (Outcome, u64) {
    let object = match bucket.get(object_key).execute().await {
        Ok(Some(object)) => object,
        Ok(None) => return (Outcome::Missing, 0),
        Err(error) => return (Outcome::Deferred(error.to_string()), 0),
    };
    let size = object.size();
    if size > OBJECT_BYTES {
        return (Outcome::TooLarge(size), 0);
    }
    let bytes = match object.body().map(|body| body.bytes()) {
        Some(read) => match read.await {
            Ok(bytes) => bytes,
            Err(error) => return (Outcome::Deferred(error.to_string()), 0),
        },
        None => return (Outcome::Deferred("object has no body".to_owned()), 0),
    };
    let read = bytes.len() as u64;
    let rewrite = match restripped_stored_media(&bytes) {
        Ok(Some(rewrite)) => rewrite,
        Ok(None) => return (Outcome::Unchanged, read),
        Err(error) => return (Outcome::Unreadable(error), read),
    };
    let custom_metadata = match object.custom_metadata() {
        Ok(metadata) => metadata,
        Err(error) => return (Outcome::Deferred(error.to_string()), read),
    };
    // Written only over the object read, keeping its content type and the rest.
    let written = bucket
        .put(object_key, rewrite)
        .http_metadata(object.http_metadata())
        .custom_metadata(custom_metadata)
        .only_if(Conditional {
            etag_matches: Some(object.etag()),
            ..Default::default()
        })
        .execute()
        .await;
    let outcome = match written {
        Ok(Some(_)) => Outcome::Stripped,
        Ok(None) => Outcome::Deferred("object changed since it was read".to_owned()),
        Err(error) => Outcome::Deferred(error.to_string()),
    };
    (outcome, read)
}

fn log_outcome(object_key: &str, outcome: &Outcome) {
    let detail = match outcome {
        Outcome::Unreadable(error) => error.to_string(),
        Outcome::TooLarge(size) => format!("{size} bytes"),
        Outcome::Deferred(reason) => reason.clone(),
        Outcome::Stripped | Outcome::Unchanged | Outcome::Missing => String::new(),
    };
    log_json_event(serde_json::json!({
        "event": "media_metadata_backfill",
        "outcome": outcome.label(),
        "object_key": object_key,
        "detail": detail,
    }));
}
