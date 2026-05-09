# Initial Roadmap

This document records the original bootstrap plan and the way it evolved. For the current plan, use [Project Plan](full-todo.md).

## Bootstrap Goals

The first implementation slice aimed to prove that a Mastodon-compatible server could run as a Rust Cloudflare Worker while keeping the system boundaries clear.

- Create a Cargo workspace and Worker entrypoint.
- Establish reproducible development tooling through `devbox`.
- Add D1 migrations and R2 media storage bindings.
- Implement Cloudflare Access based local account authentication.
- Serve basic instance, account, status, media, WebFinger, and actor surfaces.
- Add ActivityPub inbox/outbox foundations, signed delivery, and follower state.
- Start route-level Mastodon API compatibility tracking.

## Status
<!-- derived-from full-todo.md -->

The bootstrap slice is complete. The project now has a broad Worker implementation with D1-backed local state, R2 media, Mastodon API route coverage, ActivityPub federation slices, generated compatibility inventory, and baseline compatibility tests.

The old immediate next steps have also landed: compatibility tests exist, outbound activity queueing covers more than statuses, search URL and hashtag resolution improved, and timeline/context/media regressions have baseline coverage. Remaining work has moved from endpoint discovery to behavioral compatibility, operational safety, interop testing, and data model hardening.

## First Compatibility Slice

The initial compatibility slice covered:

- `GET /api/v1/instance`
- `GET /api/v1/accounts/verify_credentials`
- `GET /.well-known/webfinger`
- `GET /users/:username`
- `GET /api/v1/accounts/:id`
- `GET /api/v1/accounts/:id/statuses`
- `GET /api/v1/statuses/:id`
- `DELETE /api/v1/statuses/:id`
- `POST /api/v1/statuses`
- `POST /api/v2/media`
- Cloudflare Access `me -> local account` resolution

Those routes are no longer the whole project scope; they are kept here as the historical first slice.

## Reference Sources

- Mastodon endpoint shape:
  - `../rustresort/src/api/mastodon/instance.rs`
  - `../rustresort/src/api/mastodon/accounts.rs`
  - `../rustresort/src/api/mastodon/statuses.rs`
  - `../rustresort/src/api/mastodon/media.rs`
- Federation flow:
  - `../rustresort/src/federation/webfinger.rs`
  - `../rustresort/src/federation/signature.rs`
  - `../rustresort/src/federation/delivery.rs`
- Compatibility test ideas:
  - `../rustresort/tests/e2e_mastodon_api.rs`
  - `../rustresort/tests/e2e_activitypub.rs`

## Deferred Items That Have Since Landed

The bootstrap roadmap deferred filters, notifications, polls, bookmarks, WebPush, and broad Mastodon API expansion. Most of those areas now have at least a minimal implementation. The remaining concern is depth: behavioral parity, permission edges, delivery robustness, and operational tooling.
