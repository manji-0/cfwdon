#!/usr/bin/env node
/**
 * Backfill unresolved Announce boost targets into remote_statuses.
 *
 *   node scripts/backfill_boost_targets.mjs --dry-run
 *   node scripts/backfill_boost_targets.mjs --remote
 */
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";

const args = new Set(process.argv.slice(2));
const database = valueAfter("--database") ?? "cfwdon";
const remoteMode = args.has("--local") ? "--local" : "--remote";
const dryRun = args.has("--dry-run");
const PUBLIC = "https://www.w3.org/ns/activitystreams#Public";

function valueAfter(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function sqlString(value) {
  if (value == null) return "NULL";
  return `'${String(value).replaceAll("'", "''")}'`;
}

function entityId(bytes = 8) {
  return randomBytes(bytes).toString("hex");
}

function stripBasicHtmlTags(html) {
  let output = "";
  let inTag = false;
  for (const ch of html) {
    if (ch === "<") inTag = true;
    else if (ch === ">") inTag = false;
    else if (!inTag) output += ch;
  }
  return output;
}

function decodeBasicHtmlEntities(value) {
  return value
    .replaceAll("&nbsp;", " ")
    .replaceAll("&#39;", "'")
    .replaceAll("&#x27;", "'")
    .replaceAll("&quot;", '"')
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&");
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

// Mirrors crates/cfwdon-worker/src/content_helpers.rs::render_status_html
function renderStatusHtml(text) {
  const escaped = escapeHtml(text.trim());
  const paragraphs = escaped
    .split("\n\n")
    .map((paragraph) => paragraph.replaceAll("\n", "<br />"))
    .map((paragraph) => `<p>${paragraph}</p>`);
  return paragraphs.length > 0 ? paragraphs.join("") : "<p></p>";
}

// Mirrors crates/cfwdon-worker/src/content_helpers.rs::sanitize_remote_status_html
function sanitizeRemoteStatusHtml(html) {
  return renderStatusHtml(decodeBasicHtmlEntities(stripBasicHtmlTags(html)));
}

function sanitizeSummaryHtml(summary) {
  if (typeof summary !== "string" || !summary.trim()) return "";
  return sanitizeRemoteStatusHtml(summary);
}

function wrangler(command) {
  const result = spawnSync(
    "wrangler",
    ["d1", "execute", database, remoteMode, "--json", "--command", command],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `wrangler exited with ${result.status}`);
  }
  return JSON.parse(result.stdout);
}

function resultRows(output) {
  const first = Array.isArray(output) ? output[0] : output;
  return first?.results ?? first?.result?.[0]?.results ?? [];
}

function asId(value) {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (value && typeof value === "object") {
    if (typeof value.id === "string" && value.id.trim()) return value.id.trim();
    if (typeof value["@id"] === "string" && value["@id"].trim()) return value["@id"].trim();
  }
  return null;
}

function audienceHasPublic(value) {
  if (value == null) return false;
  if (typeof value === "string") {
    return value === PUBLIC || value.endsWith("#Public");
  }
  if (Array.isArray(value)) return value.some(audienceHasPublic);
  if (typeof value === "object") return audienceHasPublic(value.id);
  return false;
}

function isPublicNote(object) {
  return audienceHasPublic(object.to) || audienceHasPublic(object.cc);
}

function extractNote(document) {
  if (!document || typeof document !== "object") return null;
  const types = []
    .concat(document.type ?? [])
    .flat()
    .map(String);
  if (types.some((t) => ["Note", "Question", "Article"].includes(t))) {
    return document;
  }
  const object = document.object;
  if (object && typeof object === "object") {
    const objectTypes = []
      .concat(object.type ?? [])
      .flat()
      .map(String);
    if (objectTypes.some((t) => ["Note", "Question", "Article"].includes(t))) {
      return object;
    }
  }
  return null;
}

function authority(url) {
  try {
    return new URL(url).host.toLowerCase();
  } catch {
    return null;
  }
}

async function fetchJson(url) {
  const response = await fetch(url, {
    headers: {
      Accept:
        'application/activity+json, application/ld+json; profile="https://www.w3.org/ns/activitystreams", application/json',
      "User-Agent": "cfwdon-backfill/0.1 (+https://fedi.manji.app)",
    },
    redirect: "follow",
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status} for ${url}`);
  }
  return response.json();
}

function contentHtml(object) {
  if (typeof object.content === "string") return sanitizeRemoteStatusHtml(object.content);
  if (typeof object.name === "string") return renderStatusHtml(object.name);
  return "";
}

function visibility(object) {
  const toPublic = audienceHasPublic(object.to);
  const ccPublic = audienceHasPublic(object.cc);
  if (toPublic) return "public";
  if (ccPublic) return "unlisted";
  return "private";
}

function pickUrl(object) {
  if (typeof object.url === "string") return object.url;
  if (Array.isArray(object.url)) {
    for (const entry of object.url) {
      if (typeof entry === "string") return entry;
      if (entry && typeof entry.href === "string") return entry.href;
    }
  }
  return null;
}

function language(object) {
  if (object.contentMap && typeof object.contentMap === "object") {
    const keys = Object.keys(object.contentMap);
    if (keys.length > 0) return keys[0].toLowerCase();
  }
  return null;
}

function parseActor(document, fetchedUri) {
  const actorUri = asId(document) ?? fetchedUri;
  const inbox = asId(document.inbox);
  if (!inbox) throw new Error(`actor missing inbox: ${fetchedUri}`);
  const publicKey = document.publicKey ?? {};
  const publicKeyId = asId(publicKey.id) ?? `${actorUri}#main-key`;
  const publicKeyPem =
    typeof publicKey.publicKeyPem === "string" ? publicKey.publicKeyPem : "missing";
  const preferredUsername =
    typeof document.preferredUsername === "string" && document.preferredUsername
      ? document.preferredUsername
      : actorUri.split("/").filter(Boolean).pop() ?? "unknown";
  const host = authority(actorUri) ?? "unknown.example";
  const icon = document.icon;
  const image = document.image;
  return {
    actor_uri: actorUri,
    username: preferredUsername.toLowerCase(),
    domain: host,
    locked: Boolean(document.manuallyApprovesFollowers),
    bot: Boolean(document.bot) || String(document.type).includes("Service"),
    discoverable: document.discoverable !== false,
    indexable: document.indexable !== false,
    inbox_uri: inbox,
    shared_inbox_uri: asId(document.endpoints?.sharedInbox),
    public_key_id: publicKeyId,
    public_key_pem: publicKeyPem,
    display_name: typeof document.name === "string" ? document.name : "",
    summary_html: sanitizeSummaryHtml(document.summary),
    profile_url: typeof document.url === "string" ? document.url : actorUri,
    avatar_url: typeof icon?.url === "string" ? icon.url : typeof icon === "string" ? icon : null,
    header_url:
      typeof image?.url === "string" ? image.url : typeof image === "string" ? image : null,
  };
}

function upsertActorSql(actor) {
  return `INSERT INTO remote_actors (
    actor_uri, username, domain, locked, bot, discoverable, indexable,
    inbox_uri, shared_inbox_uri, public_key_id, public_key_pem,
    display_name, summary_html, profile_url, avatar_url, header_url,
    created_at, updated_at
  ) VALUES (
    ${sqlString(actor.actor_uri)},
    ${sqlString(actor.username)},
    ${sqlString(actor.domain)},
    ${actor.locked ? 1 : 0},
    ${actor.bot ? 1 : 0},
    ${actor.discoverable ? 1 : 0},
    ${actor.indexable ? 1 : 0},
    ${sqlString(actor.inbox_uri)},
    ${sqlString(actor.shared_inbox_uri)},
    ${sqlString(actor.public_key_id)},
    ${sqlString(actor.public_key_pem)},
    ${sqlString(actor.display_name)},
    ${sqlString(actor.summary_html)},
    ${sqlString(actor.profile_url)},
    ${sqlString(actor.avatar_url)},
    ${sqlString(actor.header_url)},
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
  )
  ON CONFLICT(actor_uri) DO UPDATE SET
    username = excluded.username,
    domain = excluded.domain,
    inbox_uri = excluded.inbox_uri,
    shared_inbox_uri = excluded.shared_inbox_uri,
    public_key_id = excluded.public_key_id,
    public_key_pem = excluded.public_key_pem,
    display_name = excluded.display_name,
    summary_html = excluded.summary_html,
    profile_url = excluded.profile_url,
    avatar_url = excluded.avatar_url,
    header_url = excluded.header_url,
    updated_at = CURRENT_TIMESTAMP`;
}

function upsertStatusSql(status) {
  return `INSERT INTO remote_statuses (
    id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri,
    content_html, text_content, spoiler_text, visibility, sensitive, language,
    quote_state, published_at, raw_object_json, created_at, updated_at,
    federated_emojis_json
  ) VALUES (
    ${sqlString(status.id)},
    ${sqlString(status.actor_uri)},
    ${sqlString(status.object_uri)},
    ${sqlString(status.url)},
    ${sqlString(status.in_reply_to_uri)},
    NULL,
    NULL,
    ${sqlString(status.content_html)},
    ${sqlString(status.text_content)},
    ${sqlString(status.spoiler_text)},
    ${sqlString(status.visibility)},
    ${status.sensitive ? 1 : 0},
    ${sqlString(status.language)},
    'accepted',
    ${sqlString(status.published_at)},
    ${sqlString(status.raw_object_json)},
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP,
    '[]'
  )
  ON CONFLICT(object_uri) DO UPDATE SET
    actor_uri = excluded.actor_uri,
    url = excluded.url,
    in_reply_to_uri = excluded.in_reply_to_uri,
    content_html = excluded.content_html,
    text_content = excluded.text_content,
    spoiler_text = excluded.spoiler_text,
    visibility = excluded.visibility,
    sensitive = excluded.sensitive,
    language = excluded.language,
    published_at = excluded.published_at,
    raw_object_json = excluded.raw_object_json,
    updated_at = CURRENT_TIMESTAMP`;
}

const unresolved = resultRows(
  wrangler(`SELECT DISTINCT b.boost_of_uri AS uri
     FROM remote_statuses b
     WHERE b.boost_of_uri IS NOT NULL
       AND NOT EXISTS (
         SELECT 1 FROM remote_statuses t
         WHERE t.object_uri = b.boost_of_uri OR t.url = b.boost_of_uri
       )
       AND NOT EXISTS (
         SELECT 1 FROM statuses t WHERE t.ap_id = b.boost_of_uri
       )
     ORDER BY b.boost_of_uri`),
).map((row) => row.uri);

console.log(`unresolved boost targets: ${unresolved.length}`);

let ok = 0;
let skipped = 0;
let failed = 0;

for (const uri of unresolved) {
  try {
    const document = await fetchJson(uri);
    const note = extractNote(document);
    if (!note) {
      console.log(`skip (no note): ${uri}`);
      skipped += 1;
      continue;
    }
    if (!isPublicNote(note)) {
      console.log(`skip (not public): ${uri}`);
      skipped += 1;
      continue;
    }
    const objectId = asId(note.id) ?? uri;
    const attributed = asId(note.attributedTo);
    if (!attributed) {
      console.log(`skip (no attributedTo): ${uri}`);
      skipped += 1;
      continue;
    }
    if (authority(uri) !== authority(objectId) || authority(objectId) !== authority(attributed)) {
      console.log(`skip (authority mismatch): ${uri}`);
      skipped += 1;
      continue;
    }

    const actorDoc = await fetchJson(attributed);
    const actor = parseActor(actorDoc, attributed);
    const html = contentHtml(note);
    const status = {
      id: entityId(8),
      actor_uri: actor.actor_uri,
      object_uri: objectId,
      url: pickUrl(note),
      in_reply_to_uri: asId(note.inReplyTo),
      content_html: html,
      text_content: stripBasicHtmlTags(html),
      spoiler_text: typeof note.summary === "string" ? note.summary : "",
      visibility: visibility(note),
      sensitive: Boolean(note.sensitive),
      language: language(note),
      published_at:
        typeof note.published === "string" && note.published
          ? note.published
          : new Date().toISOString(),
      raw_object_json: JSON.stringify(note),
    };

    if (dryRun) {
      console.log(`dry-run ok: ${uri} -> ${objectId} by ${actor.actor_uri}`);
      ok += 1;
      continue;
    }

    wrangler(upsertActorSql(actor));
    wrangler(upsertStatusSql(status));
    console.log(`ok: ${uri}`);
    ok += 1;
  } catch (error) {
    failed += 1;
    console.error(`fail: ${uri}: ${error.message ?? error}`);
  }
}

console.log(
  JSON.stringify(
    {
      unresolved: unresolved.length,
      ok,
      skipped,
      failed,
      dryRun,
      fingerprint: createHash("sha256").update(unresolved.join("\n")).digest("hex").slice(0, 12),
    },
    null,
    2,
  ),
);
