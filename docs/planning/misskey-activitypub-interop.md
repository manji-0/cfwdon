# Misskey ActivityPub Interop

Tracked checklist for ActivityPub federation between `cfwdon` and Misskey.
This is not Misskey native client API coverage.

## Purpose
<!-- constrained-by ./full-todo.md#activitypub-follow-up -->
<!-- constrained-by ../architecture/cfwdon-architecture.md#activitypub-and-federation-modules -->

Record code-backed and fixture-backed Misskey federation gaps so residual live
interop tests stay narrow. Statuses below come from local handler/parser
behavior; live rows stay `unverified` until a Misskey environment is available.

## Status Labels

- `supported`: behavior matches Misskey expectations for the listed surface
- `partial`: works for a Mastodon-shaped subset, but Misskey-specific fields or types are lost or ignored
- `unsupported`: Misskey traffic is dropped or cannot be expressed
- `unverified`: needs live Misskey round-trip evidence

## Discovery
<!-- derived-from ../architecture/cfwdon-architecture.md#activitypub-and-federation-modules -->

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| WebFinger `acct:` lookup | supported | `resolve_webfinger_actor_uri` | Standard WebFinger self link to actor | Low | Code path present; live round-trip still useful |
| Actor types `Person`/`Service`/`Application`/`Group`/`Organization` | supported | `is_activitypub_actor_type` | Misskey may emit non-`Person` actors | Medium | Fixture: actor-type acceptance |
| Shared inbox via `endpoints.sharedInbox` | supported | `remote_actor_shared_inbox_uri` | Misskey actors expose shared inbox under `endpoints` | High if missing | Fixture + existing unit test |
| Shared inbox via top-level `sharedInbox` only | unsupported | `remote_actor_shared_inbox_uri` | Some documents also expose top-level `sharedInbox` | Medium | Fixture: top-level alone yields `None` |
| NodeInfo software identity | partial | `/.well-known/nodeinfo` routes | Misskey reads NodeInfo for software hints | Low | Routes exist; schema parity unverified live |
| Signed GET for remote fetch | unsupported | `fetch_remote_http_json` | Some strict peers require signed GET; Misskey often does not | Medium vs strict peers | Unsigned GET only |

## Inbound Activities
<!-- derived-from ./full-todo.md#activitypub-follow-up -->

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| `Follow` / `Accept` / `Reject` / `Undo` | supported | `dispatch_inbox_activity` | Standard follow state machine | High | Handled in inbox dispatch |
| `Create` / `Update` / `Delete` Note | supported | `handle_inbox_create` | Federated notes | High | Handled; live residual |
| `Like` (plain) | supported | `handle_inbox_like` | Favourite-equivalent | Medium | Handled |
| `Like` with `_misskey_reaction` | partial | `handle_inbox_like` | Custom emoji reaction | High for reaction UX | Fixture: treated as plain Like |
| `EmojiReact` | unsupported | `dispatch_inbox_activity` default | Custom reaction activity | High for reaction UX | Fixture: type recognized, dispatch no-op |
| `Announce` | supported | `handle_inbox_announce` | Renote / boost | Medium | Handled |
| Poll vote as `Create` Note/`Question` with `name` + `inReplyTo` | supported | `handle_inbox_poll_vote` | Common federated vote shape | High | Handler present |
| `Vote` activity type | unsupported | `dispatch_inbox_activity` default | Misskey may emit dedicated `Vote` | Medium | Fixture: dispatch no-op |
| `Flag` | unsupported | `dispatch_inbox_activity` default | Remote report | Low | Fixture: dispatch no-op |
| Unknown types | partial | `_ => Ok(())` | Safe ignore vs ack | Low | Silent 202-path ignore after verify |

## Outbound Activities

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| Follow / Accept / Reject / Undo | supported | `activitypub/social_activities.rs` | Standard follow objects | High | Builders present; live residual |
| Create / Update / Delete status | supported | `activitypub/updates.rs`, delete builders | Note delivery | High | Builders present; live residual |
| Like / Announce / Undo | supported | `activitypub/social_activities.rs` | Favourite / renote | Medium | Builders present |
| Quote fields on Note | supported | `build` paths in `activitypub/objects.rs` | `_misskey_quote` / `quoteUri` / `quoteUrl` | High | Fixtures + existing tests |
| Custom emoji reaction outbound | unsupported | no reaction builder | `EmojiReact` or `_misskey_reaction` | High for reaction UX | No code path |
| Quote authorization (FEP-044f) | partial | quote authorization builders | Misskey may use quote URI fields without FEP flow | Medium | Present locally; Misskey acceptance unverified |

## Note And Question Shape

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| HTML `content` | supported | `remote_status_content_html` | Misskey usually federates rendered HTML | Medium | Fixture |
| `_misskey_content` / MFM `source` | unsupported | `remote_status_content_html` | Original MFM retained in extensions | Medium for formatting | Fixture: ignored |
| `summary` CW + `sensitive` | supported | `activity_pub_status_input_from_object` | CW and sensitive flags | Medium | Mapped |
| Attachments as AS `attachment` | partial | status object builders / remote upsert | Standard attachments; cache policy unfinished | Medium | Code present; remote media policy open |
| `Question` polls | supported | poll modules + Create path | `oneOf`/`anyOf`, votes | High | Implemented; live residual |
| Custom emoji `tag` objects | unverified | emoji tags not federated as first-class | `Emoji` tags with icon URLs | Medium | Needs live sample |

## Reactions

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| Binary favourite via `Like` | supported | `handle_inbox_like` | Works as heart/like | Medium | Code |
| Custom reaction payload | unsupported | no reaction storage | Unicode or custom emoji reaction | High | Fixtures show lossy Like-only path |
| Reaction undo semantics | partial | `Undo` of `Like` | Undo reaction vs undo like | Medium | Like undo only |

## Quotes

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| Inbound `_misskey_quote` | supported | `quote_target_uri_from_object` | Misskey quote URI | High | Fixture |
| Inbound `quoteUri` / `quoteUrl` | supported | `quote_target_uri_from_object` | Fedibird / AS aliases | Medium | Fixture |
| Outbound triple quote fields | supported | `activitypub/objects.rs` | Misskey can resolve `_misskey_quote` | High | Existing unit tests |
| Quote-only Note without other quote keys | supported | adapters + parse | Misskey often sends `_misskey_quote` alone | High | Fixture |

## Signatures And Delivery
<!-- derived-from ../architecture/cfwdon-architecture.md#api-and-federation-behavior -->

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| Inbound HTTP Signature verify | supported | inbox signature verification | Signed POST activities | High | Code; live residual |
| Outbound signed delivery + retry | supported | delivery queue modules | Async shared-inbox delivery | High | Code; live residual |
| Shared inbox target routing | supported | `resolve_shared_inbox_target_accounts` | Correct audience targeting | High | Code; live residual |
| Signed GET / query canonicalization | unsupported / unverified | unsigned `fetch_remote_http_json` | Peer-dependent | Medium | No signed GET |

## Visibility

| Item | Status | cfwdon pointer | Misskey expectation | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| public / unlisted / followers / direct audience mapping | supported | `visibility_from_activitypub_object` | Mapped via `to`/`cc` | High | Existing tests |
| Misskey `localOnly` non-federation | supported (N/A inbound) | no localOnly emitter for remote | Local-only notes are not federated | Low | No remote object expected |
| Followers-only interaction checks | supported | `remote_actor_may_interact_with_local_status` | Followers visibility | High | Code; live residual |

## Fixture Evidence
<!-- derived-from #inbound-activities -->
<!-- derived-from #note-and-question-shape -->
<!-- derived-from #reactions -->
<!-- derived-from #quotes -->

Fixture-backed assertions live in `crates/cfwdon-worker/src/activitypub/misskey_compat_tests.rs`
and inbox activity-type coverage in `crates/cfwdon-worker/src/inbox.rs` tests.

Covered shapes (all green under `cargo test -p cfwdon-worker misskey_`):

- Note with `_misskey_content` / MFM `source` (content HTML kept, extensions ignored)
- Note with `_misskey_quote` only
- `Like` with `_misskey_reaction`
- `EmojiReact`, `Vote`, and `Flag` activity types (recognized, not handled)
- Poll vote-shaped `Create` Note (`name` + `inReplyTo`)
- Actor with `endpoints.sharedInbox`
- Actor with top-level `sharedInbox` only
- Non-`Person` actor types
- `EmojiReact` does not resolve a Like-style inbox target username

Matrix rows above that cite these fixtures are code-backed, not speculative.
Refresh by running worker unit tests (`devbox run test` / crate tests) after parser or inbox changes.

## Residual Live Tests

Live Misskey round-trips were **not executed** in this pass: no Docker runtime and no
local Misskey checkout were available in the agent environment (2026-07-29).
The shortlist below is what remains after fixture/code evidence filled the matrix.

1. Mutual WebFinger + actor document fetch (cfwdon <-> Misskey)
2. Follow / Accept and Undo Follow both directions
3. Create public Note delivery both directions, including reply threading
4. Announce / Like round-trip
5. Quote Note with `_misskey_quote` only from Misskey into cfwdon timeline
6. Question create + vote (`Create` vote shape) both directions
7. Delete / Undo delivery and tombstone visibility
8. Optional: custom reaction from Misskey (`EmojiReact` or `_misskey_reaction`) to confirm lossy Like-only behavior in production
9. Optional: confirm Misskey accepts cfwdon outbound quote triple fields

When a Misskey instance is available (local Docker or disposable test host), run the
numbered items against `devbox run worker:dev`, then move matching matrix rows from
`unverified` / live-residual notes to observed pass/fail with HTTP status evidence.

## Summary
<!-- derived-from #discovery -->
<!-- derived-from #inbound-activities -->
<!-- derived-from #outbound-activities -->
<!-- derived-from #reactions -->
<!-- derived-from #quotes -->
<!-- derived-from #residual-live-tests -->

Core Mastodon-shaped federation and Misskey quote URI fields look viable from
code and fixtures. The largest product gaps are custom reactions and MFM source
retention. Highest-risk remaining work is live signature/delivery validation,
not additional route inventory.
