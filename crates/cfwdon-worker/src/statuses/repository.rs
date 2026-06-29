use super::{
    D1Database, LocalAccount, MediaAttachmentRow, Result, StatusRow, can_view_local_status,
    find_account_by_id, find_media_attachments_by_status_id, find_status_by_id,
    load_in_reply_to_account_id,
};

pub(crate) struct LocalStatusResponsePreload {
    pub(crate) media: Vec<MediaAttachmentRow>,
    pub(crate) in_reply_to_account_id: Option<String>,
}

pub(crate) struct LoadedLocalStatusResponseSubject {
    pub(crate) status: StatusRow,
    pub(crate) account: LocalAccount,
    pub(crate) preload: LocalStatusResponsePreload,
}

pub(crate) enum ResolvedLocalStatusResponseSubject {
    Loaded(LoadedLocalStatusResponseSubject),
    Hidden,
}

pub(crate) async fn find_local_status_owner_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<String>> {
    Ok(find_status_by_id(db, status_id)
        .await?
        .map(|status| status.account_id))
}

pub(crate) async fn find_owned_local_status(
    db: &D1Database,
    status_id: &str,
    owner_id: &str,
) -> Result<Option<StatusRow>> {
    let Some(status) = find_status_by_id(db, status_id).await? else {
        return Ok(None);
    };
    if status.account_id != owner_id {
        return Ok(None);
    }
    Ok(Some(status))
}

pub(crate) async fn load_local_status_response_preload(
    db: &D1Database,
    status: &StatusRow,
) -> Result<LocalStatusResponsePreload> {
    Ok(LocalStatusResponsePreload {
        media: find_media_attachments_by_status_id(db, &status.id).await?,
        in_reply_to_account_id: load_in_reply_to_account_id(db, status).await?,
    })
}

pub(crate) async fn resolve_local_status_response_subject(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: StatusRow,
) -> Result<Option<ResolvedLocalStatusResponseSubject>> {
    let Some(account) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    if !can_view_local_status(db, &status, viewer, &account).await? {
        return Ok(Some(ResolvedLocalStatusResponseSubject::Hidden));
    }
    let preload = load_local_status_response_preload(db, &status).await?;
    Ok(Some(ResolvedLocalStatusResponseSubject::Loaded(
        LoadedLocalStatusResponseSubject {
            status,
            account,
            preload,
        },
    )))
}

pub(crate) async fn load_visible_local_status_response_subject(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: StatusRow,
) -> Result<Option<LoadedLocalStatusResponseSubject>> {
    match resolve_local_status_response_subject(db, viewer, status).await? {
        Some(ResolvedLocalStatusResponseSubject::Loaded(subject)) => Ok(Some(subject)),
        Some(ResolvedLocalStatusResponseSubject::Hidden) | None => Ok(None),
    }
}

pub(crate) async fn find_visible_local_status_response_subject(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status_id: &str,
) -> Result<Option<LoadedLocalStatusResponseSubject>> {
    let Some(status) = find_status_by_id(db, status_id).await? else {
        return Ok(None);
    };
    load_visible_local_status_response_subject(db, viewer, status).await
}

pub(crate) async fn find_owned_local_status_response_subject(
    db: &D1Database,
    status_id: &str,
    owner: &LocalAccount,
) -> Result<Option<LoadedLocalStatusResponseSubject>> {
    let Some(status) = find_owned_local_status(db, status_id, owner.id()).await? else {
        return Ok(None);
    };
    load_visible_local_status_response_subject(db, Some(owner), status).await
}
