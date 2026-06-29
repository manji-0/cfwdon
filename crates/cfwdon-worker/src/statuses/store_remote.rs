use super::{
    AccountStatusVisibilityScope, D1Database, RemoteActorRow, RemoteStatusRecord, RemoteStatusRow,
    ResolvedTimelineCursor, Result, normalize_hashtag, remote_status_from_record, sql_placeholders,
    unique_ordered_refs,
};
use std::collections::HashSet;
use worker::d1::D1Type;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RemoteAccountStatusListOptions<'a> {
    pub(crate) max_id: Option<&'a str>,
    pub(crate) min_id: Option<&'a str>,
    pub(crate) limit: u32,
    pub(crate) visibility: AccountStatusVisibilityScope,
    pub(crate) only_media: bool,
    pub(crate) exclude_replies: bool,
    pub(crate) exclude_reblogs: bool,
    pub(crate) tagged: Option<&'a str>,
}

pub(crate) async fn list_remote_public_timeline_statuses(
    db: &D1Database,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
        remote_public_timeline_statuses_sql(),
        &remote_public_timeline_statuses_bindings(cursor, limit),
    )
    .await
}

fn remote_public_timeline_statuses_bindings<'a>(
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 5] {
    [
        optional_text_binding(cursor.max_timestamp.as_deref()),
        optional_text_binding(cursor.max_id.as_deref()),
        optional_text_binding(cursor.min_timestamp.as_deref()),
        optional_text_binding(cursor.min_id.as_deref()),
        D1Type::Integer(limit as i32),
    ]
}

fn remote_public_timeline_statuses_sql() -> &'static str {
    "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND (
                ?1 IS NULL
                OR rs.published_at < ?1
                OR (rs.published_at = ?1 AND rs.id < ?2)
           )
           AND (
                ?3 IS NULL
                OR rs.published_at > ?3
                OR (rs.published_at = ?3 AND rs.id > ?4)
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?5"
}

pub(crate) async fn list_remote_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
        remote_home_timeline_statuses_sql(),
        &remote_home_timeline_statuses_bindings(viewer_account_id, cursor, limit),
    )
    .await
}

fn remote_home_timeline_statuses_bindings<'a>(
    viewer_account_id: &'a str,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 6] {
    [
        D1Type::Text(viewer_account_id),
        optional_text_binding(cursor.max_timestamp.as_deref()),
        optional_text_binding(cursor.max_id.as_deref()),
        optional_text_binding(cursor.min_timestamp.as_deref()),
        optional_text_binding(cursor.min_id.as_deref()),
        D1Type::Integer(limit as i32),
    ]
}

fn remote_home_timeline_statuses_sql() -> &'static str {
    "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         JOIN follows f
           ON f.target_actor_uri = rs.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE rs.visibility IN ('public', 'unlisted', 'private')
           AND (
                ?2 IS NULL
                OR rs.published_at < ?2
                OR (rs.published_at = ?2 AND rs.id < ?3)
           )
           AND (
                ?4 IS NULL
                OR rs.published_at > ?4
                OR (rs.published_at = ?4 AND rs.id > ?5)
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?6"
}

pub(crate) async fn find_remote_statuses_with_actors_by_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let ids = unique_ordered_refs(status_ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let sql = remote_statuses_with_actors_by_ids_sql(ids.len());
    let bindings = remote_statuses_with_actors_by_ids_bindings(&ids);

    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_statuses_with_actors_by_ids_bindings<'a>(ids: &[&'a String]) -> Vec<D1Type<'a>> {
    ids.iter().map(|id| D1Type::Text(id.as_str())).collect()
}

fn remote_statuses_with_actors_by_ids_sql(id_count: usize) -> String {
    let placeholders = sql_placeholders(1, id_count);
    format!(
        "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.id IN ({placeholders})"
    )
}

pub(crate) async fn list_remote_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    list_remote_public_statuses_by_tags(db, &[normalize_hashtag(tag)], cursor, limit).await
}

pub(crate) async fn list_remote_public_statuses_by_tags(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let (mut rows, tags) =
        list_remote_public_statuses_by_tags_indexed(db, tags, cursor, limit).await?;
    if rows.len() >= limit as usize {
        return Ok(rows);
    }
    let mut seen_ids = rows
        .iter()
        .map(|(status, _)| status.id.clone())
        .collect::<HashSet<_>>();
    for (status, actor) in
        list_remote_public_statuses_by_tags_legacy(db, &tags, cursor, limit).await?
    {
        if seen_ids.insert(status.id.clone()) {
            rows.push((status, actor));
        }
    }
    rows.sort_by(|(left_status, _), (right_status, _)| {
        right_status
            .published_at
            .cmp(&left_status.published_at)
            .then_with(|| right_status.id.cmp(&left_status.id))
    });
    rows.truncate(limit as usize);
    Ok(rows)
}

async fn list_remote_public_statuses_by_tags_indexed(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<(Vec<(RemoteStatusRow, RemoteActorRow)>, Vec<String>)> {
    let tags = normalized_unique_remote_status_tags(tags);
    if tags.is_empty() {
        return Ok((Vec::new(), tags));
    }

    let sql = remote_public_statuses_by_tags_indexed_sql(tags.len());
    let bindings = remote_public_statuses_by_tags_indexed_bindings(&tags, cursor, limit);

    Ok((
        query_remote_statuses_with_actor(db, &sql, &bindings).await?,
        tags,
    ))
}

fn normalized_unique_remote_status_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .map(|tag| normalize_hashtag(tag))
        .filter(|tag| !tag.is_empty() && seen.insert(tag.clone()))
        .collect()
}

fn remote_public_statuses_by_tags_indexed_sql(tag_count: usize) -> String {
    let tag_placeholders = (1..=tag_count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let max_timestamp_index = tag_count + 1;
    let max_id_index = tag_count + 2;
    let min_timestamp_index = tag_count + 3;
    let min_id_index = tag_count + 4;
    let limit_index = tag_count + 5;
    format!(
        "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND rs.id IN (
               SELECT h.status_id
               FROM remote_status_hashtags h
               WHERE h.tag IN ({tag_placeholders})
           )
           AND (
                ?{max_timestamp_index} IS NULL
                OR rs.published_at < ?{max_timestamp_index}
                OR (rs.published_at = ?{max_timestamp_index} AND rs.id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR rs.published_at > ?{min_timestamp_index}
                OR (rs.published_at = ?{min_timestamp_index} AND rs.id > ?{min_id_index})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_index}"
    )
}

fn remote_public_statuses_by_tags_indexed_bindings<'a>(
    tags: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = tags
        .iter()
        .map(|tag| D1Type::Text(tag.as_str()))
        .collect::<Vec<_>>();
    extend_remote_timeline_cursor_bindings(&mut bindings, cursor, limit);
    bindings
}

async fn list_remote_public_statuses_by_tags_legacy(
    db: &D1Database,
    tags: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let patterns = remote_public_statuses_by_tags_legacy_patterns(tags);
    let sql = remote_public_statuses_by_tags_legacy_sql(patterns.len());
    let bindings = remote_public_statuses_by_tags_legacy_bindings(&patterns, cursor, limit);

    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_public_statuses_by_tags_legacy_patterns(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| format!("%#{}%", normalize_hashtag(tag)))
        .collect()
}

fn remote_public_statuses_by_tags_legacy_sql(pattern_count: usize) -> String {
    let match_clause = (1..=pattern_count)
        .map(|index| format!("lower(rs.content_html) LIKE ?{index}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let max_timestamp_index = pattern_count + 1;
    let max_id_index = pattern_count + 2;
    let min_timestamp_index = pattern_count + 3;
    let min_id_index = pattern_count + 4;
    let limit_index = pattern_count + 5;
    format!(
        "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ({match_clause})
           AND (
                ?{max_timestamp_index} IS NULL
                OR rs.published_at < ?{max_timestamp_index}
                OR (rs.published_at = ?{max_timestamp_index} AND rs.id < ?{max_id_index})
           )
           AND (
                ?{min_timestamp_index} IS NULL
                OR rs.published_at > ?{min_timestamp_index}
                OR (rs.published_at = ?{min_timestamp_index} AND rs.id > ?{min_id_index})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_index}"
    )
}

fn remote_public_statuses_by_tags_legacy_bindings<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    extend_remote_timeline_cursor_bindings(&mut bindings, cursor, limit);
    bindings
}

pub(crate) async fn list_remote_public_statuses_by_link(
    db: &D1Database,
    urls: &[String],
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    if urls.is_empty() {
        return Ok(Vec::new());
    }

    let patterns = remote_public_statuses_by_link_patterns(urls);
    let sql = remote_public_statuses_by_link_sql(patterns.len());
    let bindings = remote_public_statuses_by_link_bindings(&patterns, cursor, limit);
    query_remote_statuses_with_actor(db, &sql, &bindings).await
}

fn remote_public_statuses_by_link_patterns(urls: &[String]) -> Vec<String> {
    urls.iter().map(|url| format!("%{url}%")).collect()
}

fn remote_public_statuses_by_link_sql(pattern_count: usize) -> String {
    let match_clause = (1..=pattern_count)
        .map(|position| {
            format!(
                "(rs.content_html LIKE ?{position} OR rs.url LIKE ?{position} OR rs.object_uri LIKE ?{position})"
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let cursor_offset = pattern_count;
    format!(
        "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ra.discoverable = 1
           AND ({match_clause})
           AND (
                ?{max_timestamp} IS NULL
                OR rs.published_at < ?{max_timestamp}
                OR (rs.published_at = ?{max_timestamp} AND rs.id < ?{max_id})
           )
           AND (
                ?{min_timestamp} IS NULL
                OR rs.published_at > ?{min_timestamp}
                OR (rs.published_at = ?{min_timestamp} AND rs.id > ?{min_id})
           )
         ORDER BY rs.published_at DESC, rs.id DESC
         LIMIT ?{limit_position}",
        max_timestamp = cursor_offset + 1,
        max_id = cursor_offset + 2,
        min_timestamp = cursor_offset + 3,
        min_id = cursor_offset + 4,
        limit_position = cursor_offset + 5,
    )
}

fn remote_public_statuses_by_link_bindings<'a>(
    patterns: &'a [String],
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) -> Vec<D1Type<'a>> {
    let mut bindings = patterns
        .iter()
        .map(|pattern| D1Type::Text(pattern.as_str()))
        .collect::<Vec<_>>();
    extend_remote_timeline_cursor_bindings(&mut bindings, cursor, limit);
    bindings
}

fn extend_remote_timeline_cursor_bindings<'a>(
    bindings: &mut Vec<D1Type<'a>>,
    cursor: &'a ResolvedTimelineCursor,
    limit: u32,
) {
    bindings.extend([
        optional_text_binding(cursor.max_timestamp.as_deref()),
        optional_text_binding(cursor.max_id.as_deref()),
        optional_text_binding(cursor.min_timestamp.as_deref()),
        optional_text_binding(cursor.min_id.as_deref()),
        D1Type::Integer(limit as i32),
    ]);
}

fn optional_text_binding(value: Option<&str>) -> D1Type<'_> {
    value.map_or(D1Type::Null, D1Type::Text)
}

pub(crate) async fn list_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    options: RemoteAccountStatusListOptions<'_>,
) -> Result<Vec<RemoteStatusRow>> {
    let tagged_pattern = remote_actor_status_tagged_pattern(options.tagged);
    let query_bindings =
        remote_actor_statuses_by_actor_uri_bindings(actor_uri, options, tagged_pattern.as_deref());
    let predicates = remote_actor_status_predicates(options, query_bindings.tagged_binding);
    let sql = remote_actor_statuses_by_actor_uri_sql(&predicates, query_bindings.limit_binding);

    query_remote_status_rows(db, &sql, &query_bindings.bindings).await
}

struct RemoteActorStatusQueryBindings<'a> {
    bindings: Vec<D1Type<'a>>,
    tagged_binding: Option<usize>,
    limit_binding: usize,
}

fn remote_actor_statuses_by_actor_uri_bindings<'a>(
    actor_uri: &'a str,
    options: RemoteAccountStatusListOptions<'a>,
    tagged_pattern: Option<&'a str>,
) -> RemoteActorStatusQueryBindings<'a> {
    let mut bindings = vec![
        D1Type::Text(actor_uri),
        optional_text_binding(options.max_id),
        optional_text_binding(options.min_id),
    ];
    let tagged_binding = tagged_pattern.map(|_| bindings.len() + 1);
    if let Some(pattern) = tagged_pattern {
        bindings.push(D1Type::Text(pattern));
    }

    let limit_binding = bindings.len() + 1;
    bindings.push(D1Type::Integer(options.limit as i32));

    RemoteActorStatusQueryBindings {
        bindings,
        tagged_binding,
        limit_binding,
    }
}

fn remote_actor_status_tagged_pattern(tagged: Option<&str>) -> Option<String> {
    tagged
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .map(|tag| format!("%{tag}%"))
}

fn remote_actor_status_predicates(
    options: RemoteAccountStatusListOptions<'_>,
    tagged_binding: Option<usize>,
) -> Vec<String> {
    let mut predicates = vec![
        "actor_uri = ?1".to_owned(),
        "(
            ?2 IS NULL
            OR NOT EXISTS (SELECT 1 FROM max_cursor)
            OR EXISTS (
                SELECT 1 FROM max_cursor
                WHERE remote_statuses.published_at < max_cursor.published_at
                   OR (remote_statuses.published_at = max_cursor.published_at AND remote_statuses.id < max_cursor.id)
            )
        )"
        .to_owned(),
        "(
            ?3 IS NULL
            OR NOT EXISTS (SELECT 1 FROM min_cursor)
            OR EXISTS (
                SELECT 1 FROM min_cursor
                WHERE remote_statuses.published_at > min_cursor.published_at
                   OR (remote_statuses.published_at = min_cursor.published_at AND remote_statuses.id > min_cursor.id)
            )
        )"
        .to_owned(),
    ];

    match options.visibility {
        AccountStatusVisibilityScope::All => {}
        AccountStatusVisibilityScope::Public => {
            predicates.push("visibility IN ('public', 'unlisted')".to_owned());
        }
        AccountStatusVisibilityScope::PublicUnlistedPrivate => {
            predicates.push("visibility IN ('public', 'unlisted', 'private')".to_owned());
        }
    }
    if options.exclude_reblogs {
        predicates.push("boost_of_uri IS NULL".to_owned());
    }
    if options.exclude_replies {
        predicates.push("in_reply_to_uri IS NULL".to_owned());
    }
    if options.only_media {
        predicates.push(
            "EXISTS (
                SELECT 1 FROM remote_status_attachments media
                WHERE media.status_id = remote_statuses.id
            )"
            .to_owned(),
        );
    }
    if let Some(tagged_binding) = tagged_binding {
        predicates.push(format!("lower(content_html) LIKE ?{tagged_binding}"));
    }

    predicates
}

fn remote_actor_statuses_by_actor_uri_sql(predicates: &[String], limit_binding: usize) -> String {
    format!(
        "WITH max_cursor AS (
            SELECT id, published_at FROM remote_statuses WHERE id = ?2 LIMIT 1
         ),
         min_cursor AS (
            SELECT id, published_at FROM remote_statuses WHERE id = ?3 LIMIT 1
         )
         SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
         FROM remote_statuses
         WHERE {}
         ORDER BY published_at DESC, id DESC
         LIMIT ?{limit_binding}",
        predicates.join("\n           AND ")
    )
}

pub(crate) async fn list_public_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    max_id: Option<&str>,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = public_remote_statuses_by_actor_uri_bindings(actor_uri, max_id, min_id, limit);
    query_remote_status_rows(db, public_remote_statuses_by_actor_uri_sql(), &bindings).await
}

fn public_remote_statuses_by_actor_uri_bindings<'a>(
    actor_uri: &'a str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
    limit: u32,
) -> [D1Type<'a>; 4] {
    [
        D1Type::Text(actor_uri),
        optional_text_binding(max_id),
        optional_text_binding(min_id),
        D1Type::Integer(limit as i32),
    ]
}

fn public_remote_statuses_by_actor_uri_sql() -> &'static str {
    "WITH max_cursor AS (
                SELECT id, published_at FROM remote_statuses WHERE id = ?2 LIMIT 1
             ),
             min_cursor AS (
                SELECT id, published_at FROM remote_statuses WHERE id = ?3 LIMIT 1
             )
             SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE actor_uri = ?1
               AND visibility IN ('public', 'unlisted')
               AND (
                    ?2 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM max_cursor)
                    OR EXISTS (
                        SELECT 1 FROM max_cursor
                        WHERE remote_statuses.published_at < max_cursor.published_at
                           OR (remote_statuses.published_at = max_cursor.published_at AND remote_statuses.id < max_cursor.id)
                    )
               )
               AND (
                    ?3 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM min_cursor)
                    OR EXISTS (
                        SELECT 1 FROM min_cursor
                        WHERE remote_statuses.published_at > min_cursor.published_at
                           OR (remote_statuses.published_at = min_cursor.published_at AND remote_statuses.id > min_cursor.id)
                    )
               )
             ORDER BY published_at DESC, id DESC
             LIMIT ?4"
}

pub(crate) async fn list_direct_remote_replies_by_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    query_remote_statuses_with_actor(
        db,
        direct_remote_replies_by_uri_sql(),
        &direct_remote_replies_by_uri_bindings(object_uri),
    )
    .await
}

fn direct_remote_replies_by_uri_bindings(object_uri: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(object_uri)]
}

fn direct_remote_replies_by_uri_sql() -> &'static str {
    "SELECT
            rs.id,
            rs.actor_uri,
            rs.object_uri,
            rs.url,
            rs.in_reply_to_uri,
            rs.boost_of_uri,
            rs.quote_of_uri,
            rs.content_html,
            rs.spoiler_text,
            rs.visibility,
            rs.sensitive,
            rs.language,
            rs.quote_state,
            rs.published_at,
            ra.username,
            ra.domain,
            ra.display_name,
            ra.summary_html,
            ra.profile_url,
            ra.avatar_url,
            ra.header_url,
            ra.locked,
            ra.bot,
            ra.discoverable,
            ra.indexable
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.in_reply_to_uri = ?1
         ORDER BY rs.published_at ASC"
}

async fn query_remote_statuses_with_actor(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let result = db.prepare(sql).bind_refs(bindings)?.all().await?;
    let values = result.results::<serde_json::Value>()?;
    Ok(values
        .into_iter()
        .map(|value| {
            (
                remote_status_row_from_value(&value),
                RemoteActorRow::from_value(&value),
            )
        })
        .collect())
}

async fn query_remote_status_rows(
    db: &D1Database,
    sql: &str,
    bindings: &[D1Type<'_>],
) -> Result<Vec<RemoteStatusRow>> {
    let result = db.prepare(sql).bind_refs(bindings)?.all().await?;
    result
        .results::<RemoteStatusRecord>()
        .map(|rows| rows.into_iter().map(remote_status_from_record).collect())
}

fn remote_status_row_from_value(value: &serde_json::Value) -> RemoteStatusRow {
    remote_status_from_record(RemoteStatusRecord {
        id: json_string(value, "id"),
        actor_uri: json_string(value, "actor_uri"),
        object_uri: json_string(value, "object_uri"),
        url: optional_json_string(value, "url"),
        in_reply_to_uri: optional_json_string(value, "in_reply_to_uri"),
        boost_of_uri: optional_json_string(value, "boost_of_uri"),
        quote_of_uri: optional_json_string(value, "quote_of_uri"),
        content_html: json_string(value, "content_html"),
        spoiler_text: json_string(value, "spoiler_text"),
        visibility: json_string(value, "visibility"),
        sensitive: json_i32(value, "sensitive"),
        language: optional_json_string(value, "language"),
        quote_state: json_string_or(value, "quote_state", "accepted"),
        published_at: json_string(value, "published_at"),
    })
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    json_string_or(value, key, "")
}

fn json_string_or(value: &serde_json::Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn optional_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn json_i32(value: &serde_json::Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_unique_remote_status_tags_trims_normalizes_and_deduplicates() {
        let tags = normalized_unique_remote_status_tags(&[
            " Rust ".to_owned(),
            "#rust".to_owned(),
            "Masto".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(tags, ["rust", "masto"]);
    }

    #[test]
    fn remote_public_statuses_by_tags_indexed_sql_uses_tag_and_cursor_slots() {
        let sql = remote_public_statuses_by_tags_indexed_sql(2);

        assert!(sql.contains("h.tag IN (?1, ?2)"));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("rs.id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("rs.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn remote_public_statuses_by_tags_legacy_patterns_preserve_fallback_shape() {
        let patterns = remote_public_statuses_by_tags_legacy_patterns(&[
            " Rust ".to_owned(),
            "#Masto".to_owned(),
            "  ".to_owned(),
        ]);

        assert_eq!(patterns, ["%#rust%", "%#masto%", "%#%"]);
    }

    #[test]
    fn remote_public_statuses_by_tags_legacy_sql_uses_pattern_and_cursor_slots() {
        let sql = remote_public_statuses_by_tags_legacy_sql(2);

        assert!(sql.contains("lower(rs.content_html) LIKE ?1 OR lower(rs.content_html) LIKE ?2"));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("rs.id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("rs.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn remote_public_statuses_by_link_patterns_wrap_urls_for_like_search() {
        let patterns = remote_public_statuses_by_link_patterns(&[
            "https://example.test/a".to_owned(),
            "acct:alice@example.test".to_owned(),
        ]);

        assert_eq!(
            patterns,
            ["%https://example.test/a%", "%acct:alice@example.test%"]
        );
    }

    #[test]
    fn remote_public_statuses_by_link_sql_uses_pattern_and_cursor_slots() {
        let sql = remote_public_statuses_by_link_sql(2);

        assert!(sql.contains(
            "(rs.content_html LIKE ?1 OR rs.url LIKE ?1 OR rs.object_uri LIKE ?1) OR \
             (rs.content_html LIKE ?2 OR rs.url LIKE ?2 OR rs.object_uri LIKE ?2)"
        ));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("rs.id < ?4"));
        assert!(sql.contains("?5 IS NULL"));
        assert!(sql.contains("rs.id > ?6"));
        assert!(sql.contains("LIMIT ?7"));
    }

    #[test]
    fn extend_remote_timeline_cursor_bindings_appends_cursor_slots_and_limit() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("status-max".to_owned()),
            min_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            min_id: Some("status-min".to_owned()),
        };
        let mut bindings = vec![D1Type::Text("prefix")];

        extend_remote_timeline_cursor_bindings(&mut bindings, &cursor, 12);

        assert!(matches!(bindings[0], D1Type::Text("prefix")));
        assert!(matches!(bindings[1], D1Type::Text("2026-01-02T00:00:00Z")));
        assert!(matches!(bindings[2], D1Type::Text("status-max")));
        assert!(matches!(bindings[3], D1Type::Text("2026-01-01T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Text("status-min")));
        assert!(matches!(bindings[5], D1Type::Integer(12)));
    }

    #[test]
    fn extend_remote_timeline_cursor_bindings_uses_null_for_open_bounds() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: None,
            max_id: None,
            min_timestamp: None,
            min_id: None,
        };
        let mut bindings = Vec::new();

        extend_remote_timeline_cursor_bindings(&mut bindings, &cursor, 8);

        assert!(matches!(bindings[0], D1Type::Null));
        assert!(matches!(bindings[1], D1Type::Null));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Integer(8)));
    }

    #[test]
    fn optional_text_binding_maps_some_and_none() {
        assert!(matches!(
            optional_text_binding(Some("value")),
            D1Type::Text("value")
        ));
        assert!(matches!(optional_text_binding(None), D1Type::Null));
    }

    #[test]
    fn remote_actor_status_tagged_pattern_normalizes_and_discards_blank() {
        assert_eq!(
            remote_actor_status_tagged_pattern(Some(" RustLang ")).as_deref(),
            Some("%rustlang%")
        );
        assert!(remote_actor_status_tagged_pattern(Some("  ")).is_none());
        assert!(remote_actor_status_tagged_pattern(None).is_none());
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_bindings_keep_slots_stable() {
        let query_bindings = remote_actor_statuses_by_actor_uri_bindings(
            "actor",
            RemoteAccountStatusListOptions {
                max_id: Some("max"),
                min_id: Some("min"),
                limit: 20,
                visibility: AccountStatusVisibilityScope::All,
                only_media: false,
                exclude_replies: false,
                exclude_reblogs: false,
                tagged: Some("rust"),
            },
            Some("%rust%"),
        );

        assert!(matches!(query_bindings.bindings[0], D1Type::Text("actor")));
        assert!(matches!(query_bindings.bindings[1], D1Type::Text("max")));
        assert!(matches!(query_bindings.bindings[2], D1Type::Text("min")));
        assert!(matches!(query_bindings.bindings[3], D1Type::Text("%rust%")));
        assert!(matches!(query_bindings.bindings[4], D1Type::Integer(20)));
        assert_eq!(query_bindings.tagged_binding, Some(4));
        assert_eq!(query_bindings.limit_binding, 5);
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_bindings_use_null_open_cursors() {
        let query_bindings = remote_actor_statuses_by_actor_uri_bindings(
            "actor",
            RemoteAccountStatusListOptions {
                max_id: None,
                min_id: None,
                limit: 8,
                visibility: AccountStatusVisibilityScope::All,
                only_media: false,
                exclude_replies: false,
                exclude_reblogs: false,
                tagged: None,
            },
            None,
        );

        assert!(matches!(query_bindings.bindings[1], D1Type::Null));
        assert!(matches!(query_bindings.bindings[2], D1Type::Null));
        assert!(matches!(query_bindings.bindings[3], D1Type::Integer(8)));
        assert_eq!(query_bindings.tagged_binding, None);
        assert_eq!(query_bindings.limit_binding, 4);
    }

    #[test]
    fn remote_actor_status_predicates_reflect_filters_and_tag_slot() {
        let predicates = remote_actor_status_predicates(
            RemoteAccountStatusListOptions {
                max_id: Some("max"),
                min_id: Some("min"),
                limit: 20,
                visibility: AccountStatusVisibilityScope::PublicUnlistedPrivate,
                only_media: true,
                exclude_replies: true,
                exclude_reblogs: true,
                tagged: Some("rust"),
            },
            Some(4),
        );

        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "actor_uri = ?1")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "visibility IN ('public', 'unlisted', 'private')")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "boost_of_uri IS NULL")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "in_reply_to_uri IS NULL")
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate.contains("remote_status_attachments media"))
        );
        assert!(
            predicates
                .iter()
                .any(|predicate| predicate == "lower(content_html) LIKE ?4")
        );
    }

    #[test]
    fn remote_actor_statuses_by_actor_uri_sql_uses_predicates_and_limit_slot() {
        let predicates = vec![
            "actor_uri = ?1".to_owned(),
            "boost_of_uri IS NULL".to_owned(),
        ];
        let sql = remote_actor_statuses_by_actor_uri_sql(&predicates, 4);

        assert!(sql.contains("WITH max_cursor AS"));
        assert!(sql.contains("WHERE actor_uri = ?1\n           AND boost_of_uri IS NULL"));
        assert!(sql.contains("LIMIT ?4"));
    }

    #[test]
    fn public_remote_statuses_by_actor_uri_sql_uses_fixed_cursor_slots() {
        let sql = public_remote_statuses_by_actor_uri_sql();

        assert!(sql.contains("WHERE actor_uri = ?1"));
        assert!(sql.contains("SELECT id, published_at FROM remote_statuses WHERE id = ?2"));
        assert!(sql.contains("SELECT id, published_at FROM remote_statuses WHERE id = ?3"));
        assert!(sql.contains("visibility IN ('public', 'unlisted')"));
        assert!(sql.contains("LIMIT ?4"));
    }

    #[test]
    fn direct_remote_replies_by_uri_sql_uses_reply_slot_and_ascending_order() {
        let sql = direct_remote_replies_by_uri_sql();

        assert!(sql.contains("JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri"));
        assert!(sql.contains("WHERE rs.in_reply_to_uri = ?1"));
        assert!(sql.contains("ORDER BY rs.published_at ASC"));
    }

    #[test]
    fn remote_public_timeline_statuses_sql_uses_cursor_slots_and_limit() {
        let sql = remote_public_timeline_statuses_sql();

        assert!(sql.contains("WHERE rs.visibility = 'public'"));
        assert!(sql.contains("?1 IS NULL"));
        assert!(sql.contains("rs.id < ?2"));
        assert!(sql.contains("?3 IS NULL"));
        assert!(sql.contains("rs.id > ?4"));
        assert!(sql.contains("LIMIT ?5"));
    }

    #[test]
    fn remote_home_timeline_statuses_sql_uses_viewer_cursor_slots_and_limit() {
        let sql = remote_home_timeline_statuses_sql();

        assert!(sql.contains("f.follower_account_id = ?1"));
        assert!(sql.contains("f.state = 'accepted'"));
        assert!(sql.contains("WHERE rs.visibility IN ('public', 'unlisted', 'private')"));
        assert!(sql.contains("?2 IS NULL"));
        assert!(sql.contains("rs.id < ?3"));
        assert!(sql.contains("?4 IS NULL"));
        assert!(sql.contains("rs.id > ?5"));
        assert!(sql.contains("LIMIT ?6"));
    }

    #[test]
    fn remote_statuses_with_actors_by_ids_sql_uses_id_placeholders() {
        let sql = remote_statuses_with_actors_by_ids_sql(3);

        assert!(sql.contains("JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri"));
        assert!(sql.contains("WHERE rs.id IN (?1, ?2, ?3)"));
    }

    #[test]
    fn remote_status_row_from_value_maps_optional_and_default_fields() {
        let row = remote_status_row_from_value(&serde_json::json!({
            "id": "status-1",
            "actor_uri": "https://remote.example/users/alice",
            "object_uri": "https://remote.example/users/alice/statuses/1",
            "url": "https://remote.example/@alice/1",
            "in_reply_to_uri": "https://remote.example/statuses/root",
            "boost_of_uri": "https://remote.example/statuses/boost",
            "quote_of_uri": "https://remote.example/statuses/quote",
            "content_html": "<p>Hello</p>",
            "spoiler_text": "CW",
            "visibility": "unlisted",
            "sensitive": 1,
            "language": "ja",
            "quote_state": "pending",
            "published_at": "2026-01-01T00:00:00Z"
        }));

        assert_eq!(row.id, "status-1");
        assert_eq!(row.actor_uri, "https://remote.example/users/alice");
        assert_eq!(
            row.object_uri,
            "https://remote.example/users/alice/statuses/1"
        );
        assert_eq!(row.url.as_deref(), Some("https://remote.example/@alice/1"));
        assert_eq!(
            row.in_reply_to_uri.as_deref(),
            Some("https://remote.example/statuses/root")
        );
        assert_eq!(
            row.boost_of_uri.as_deref(),
            Some("https://remote.example/statuses/boost")
        );
        assert_eq!(
            row.quote_of_uri.as_deref(),
            Some("https://remote.example/statuses/quote")
        );
        assert_eq!(row.content_html, "<p>Hello</p>");
        assert_eq!(row.spoiler_text, "CW");
        assert_eq!(row.visibility, cfwdon_domain::Visibility::Unlisted);
        assert!(row.sensitive);
        assert_eq!(row.language.as_deref(), Some("ja"));
        assert_eq!(row.quote_state, cfwdon_domain::QuoteState::Pending);
        assert_eq!(row.published_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn remote_status_row_from_value_uses_empty_and_quote_state_defaults() {
        let row = remote_status_row_from_value(&serde_json::json!({}));

        assert_eq!(row.id, "");
        assert_eq!(row.actor_uri, "");
        assert_eq!(row.object_uri, "");
        assert!(row.url.is_none());
        assert!(row.in_reply_to_uri.is_none());
        assert!(row.boost_of_uri.is_none());
        assert!(row.quote_of_uri.is_none());
        assert_eq!(row.content_html, "");
        assert_eq!(row.spoiler_text, "");
        assert_eq!(row.visibility, cfwdon_domain::Visibility::Public);
        assert!(!row.sensitive);
        assert!(row.language.is_none());
        assert_eq!(row.quote_state, cfwdon_domain::QuoteState::Accepted);
        assert_eq!(row.published_at, "");
    }
}
