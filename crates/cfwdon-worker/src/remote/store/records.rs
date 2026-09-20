use cfwdon_domain::{RemoteStatus, remote_status_default_quote_state};
use worker::{Error, Result};

pub(crate) use cfwdon_domain::RemoteStatusRecord;

pub(crate) type RemoteStatusRow = RemoteStatus;

pub(crate) fn remote_status_from_record(record: RemoteStatusRecord) -> Result<RemoteStatusRow> {
    RemoteStatus::try_from_record(record).map_err(|error| Error::RustError(error.to_string()))
}

pub(crate) fn remote_statuses_from_records(
    records: Vec<RemoteStatusRecord>,
) -> Result<Vec<RemoteStatusRow>> {
    records.into_iter().map(remote_status_from_record).collect()
}

pub(crate) fn default_remote_quote_state() -> String {
    remote_status_default_quote_state()
}

pub(crate) fn effective_remote_status_quote_state(status: &RemoteStatusRow) -> &'static str {
    status.effective_quote_state().as_str()
}
