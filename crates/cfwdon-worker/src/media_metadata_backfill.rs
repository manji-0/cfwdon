//! Strip the metadata from media stored before uploads were stripped (see
//! `migrations/127_media_metadata_backfill.sql`). Each hourly run, last in
//! the scheduled handler, takes a page of queued object keys, rewrites the
//! objects that carry metadata in place, and deletes their rows; a key whose
//! read or write fails stays queued for the next run.
//!
//! A run counts an attempt on an object before it reads it, so an object
//! that ends the invocation (over the CPU or memory limit) is not retried at
//! the head of every run: it sorts behind the objects not yet tried, and
//! after `MAX_ATTEMPTS` runs it is logged as `abandoned` and dequeued.

use crate::db_utils::d1_results;
use crate::observability::log_json_event;
use crate::runtime_config::MAX_AV_UPLOAD_BYTES;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{MalformedMedia, StoredMediaRestrip, restripped_stored_media};
use futures_util::StreamExt;
use serde::Deserialize;
use worker::{Bucket, Conditional, Env, FixedLengthStream, Result, console_error, d1::D1Type};

/// Keys listed per run; the byte budget is what usually ends a run.
const PAGE_ROWS: i32 = 200;
const MIB: u64 = 1024 * 1024;
/// Bytes read per run; once spent, the rest of the page waits for the next run.
const PAGE_BYTES: u64 = 64 * MIB;
/// An object is read in chunks into one buffer of its size. A video is
/// rewritten in that buffer and an image into one copy, and the result is
/// written back in chunks, so an object costs at most about twice its size
/// in memory (Workers allow 128 MB). Every upload, up to the 40 MiB video
/// limit, fits.
const OBJECT_BYTES: u64 = MAX_AV_UPLOAD_BYTES as u64;
/// Bytes handed to R2 at a time when writing an object back.
const WRITE_CHUNK: usize = MIB as usize;
/// Runs that may start on one object before it is given up.
const MAX_ATTEMPTS: i64 = 3;

#[derive(Debug, Deserialize)]
struct QueuedObjectRow {
    object_key: String,
    attempts: i64,
}

/// What happened to one stored object.
enum Outcome {
    Stripped,
    Unchanged,
    Missing,
    /// A TIFF, which no lossless rewrite strips; it keeps its EXIF.
    Unstrippable,
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
            Self::Unstrippable => "unstrippable",
            Self::Unreadable(_) => "unreadable",
            Self::TooLarge(_) => "too_large",
            Self::Deferred(_) => "deferred",
        }
    }
}

/// One page of the backfill, with its own D1 and R2 bindings so that a
/// failure here touches nothing else the scheduled handler does.
pub(crate) async fn run_stored_media_metadata_backfill(env: &Env, config: &AppConfig) {
    let result = async {
        let db = D1Database::new(env.d1(&config.database_binding)?);
        let bucket = env.bucket(&config.media_binding)?;
        strip_stored_media_metadata_page(&db, &bucket).await
    }
    .await;
    if let Err(error) = result {
        console_error!("stored media metadata backfill failed: {error}");
    }
}

async fn strip_stored_media_metadata_page(db: &D1Database, bucket: &Bucket) -> Result<()> {
    let result = db
        .prepare(
            "SELECT object_key, attempts FROM media_metadata_backfill \
             ORDER BY attempts, object_key LIMIT ?1",
        )
        .bind_refs(&D1Type::Integer(PAGE_ROWS))?
        .all()
        .await?;
    let rows = d1_results::<QueuedObjectRow>(&result)?;
    let mut bytes_read = 0;
    for row in rows {
        if bytes_read >= PAGE_BYTES {
            break;
        }
        if row.attempts >= MAX_ATTEMPTS {
            log_event(
                &row.object_key,
                "abandoned",
                &format!("{} attempts", row.attempts),
            );
            dequeue(db, &row.object_key).await?;
            continue;
        }
        db.prepare(
            "UPDATE media_metadata_backfill SET attempts = attempts + 1 WHERE object_key = ?1",
        )
        .bind_refs(&D1Type::Text(&row.object_key))?
        .run()
        .await?;
        let (outcome, read) = strip_stored_object(bucket, &row.object_key).await;
        bytes_read += read;
        log_outcome(&row.object_key, &outcome);
        if !matches!(outcome, Outcome::Deferred(_)) {
            dequeue(db, &row.object_key).await?;
        }
    }
    Ok(())
}

async fn dequeue(db: &D1Database, object_key: &str) -> Result<()> {
    db.prepare("DELETE FROM media_metadata_backfill WHERE object_key = ?1")
        .bind_refs(&D1Type::Text(object_key))?
        .run()
        .await?;
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
    let Some(body) = object.body() else {
        return (Outcome::Deferred("object has no body".to_owned()), 0);
    };
    // Read chunk by chunk rather than as one array buffer, which would be a
    // second whole copy outside the Wasm heap.
    let mut bytes = Vec::with_capacity(size as usize);
    let mut chunks = match body.stream() {
        Ok(chunks) => chunks,
        Err(error) => return (Outcome::Deferred(error.to_string()), 0),
    };
    while let Some(chunk) = chunks.next().await {
        match chunk {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(error) => return (Outcome::Deferred(error.to_string()), bytes.len() as u64),
        }
    }
    let read = bytes.len() as u64;
    let rewrite = match restripped_stored_media(bytes) {
        Ok(StoredMediaRestrip::Rewrite(rewrite)) => rewrite,
        Ok(StoredMediaRestrip::Unchanged) => return (Outcome::Unchanged, read),
        Ok(StoredMediaRestrip::Tiff) => return (Outcome::Unstrippable, read),
        Err(error) => return (Outcome::Unreadable(error), read),
    };
    let custom_metadata = match object.custom_metadata() {
        Ok(metadata) => metadata,
        Err(error) => return (Outcome::Deferred(error.to_string()), read),
    };
    let length = rewrite.len() as u64;
    let chunks = futures_util::stream::iter(
        (0..rewrite.len())
            .step_by(WRITE_CHUNK)
            .map(move |at| Ok(rewrite[at..rewrite.len().min(at + WRITE_CHUNK)].to_vec())),
    );
    // Written only over the object read, keeping its content type and the rest.
    let written = bucket
        .put(object_key, FixedLengthStream::wrap(chunks, length))
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
        Outcome::Stripped | Outcome::Unchanged | Outcome::Missing | Outcome::Unstrippable => {
            String::new()
        }
    };
    log_event(object_key, outcome.label(), &detail);
}

fn log_event(object_key: &str, outcome: &str, detail: &str) {
    log_json_event(serde_json::json!({
        "event": "media_metadata_backfill",
        "outcome": outcome,
        "object_key": object_key,
        "detail": detail,
    }));
}
