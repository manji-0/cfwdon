# Mastodon API Compatibility

`cfwdon` tracks Mastodon API compatibility as a route-level mapping between upstream Mastodon route definitions and local Worker route handlers.

## Source of Truth

- Upstream route definitions:
  - `https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes.rb`
  - `https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes/api.rb`
- Local route definition:
  - `crates/cfwdon-worker/src/router.rs`
- Project planning:
  - `docs/planning/full-todo.md`

When `docs.joinmastodon.org` and `config/routes/api.rb` disagree about deprecated endpoints, this inventory follows the upstream route definitions.

## Scope

The inventory focuses on externally visible Mastodon API surfaces that are useful compatibility targets for `cfwdon`.

- discovery / OAuth metadata
- `/api/oembed`
- `/api/v1_alpha`
- `/api/v1`
- `/api/v2`

The current inventory excludes:

- `/api/v1/admin`, `/api/v2/admin`
- `/api/web`
- ActivityPub actor / inbox / outbox routes themselves

## Status Labels

- `implemented`: `cfwdon` has the same upstream path and method.
- `compat-gap`: the route exists, but known implementation notes or TODOs describe a compatibility gap.
- `missing`: the upstream route is not present in `cfwdon`.
- `extra`: `cfwdon` has a route that is not present in the current upstream route set.

## Files

- `inventory.md`: upstream API list and local route mapping.
- `todo-unimplemented.md`: TODO list for `missing` routes only.
- `todo-compat.md`: TODO list for `compat-gap` routes only.
- `../../scripts/generate_mastodon_api_compat.py`: inventory and TODO regeneration script.

## Refresh

```bash
python3 scripts/generate_mastodon_api_compat.py
```

## Current Extra Routes In cfwdon

Routes that exist in `cfwdon` but not in the current upstream `config/routes/api.rb` snapshot:

- `GET /api/v1/timelines/direct` via `direct_timeline_response`
- `GET /api/v1/statuses/:id/card` via `status_card_response`
- `PUT /api/v2/media/:id` via `update_media_attachment`
- `PATCH /api/v2/media/:id` via `update_media_attachment`
- `GET /api/v1/follow_requests/:id` via `follow_request_response`
- `GET /api/v1/search` via `search_v1`

Treat these as compatibility review items. Some may be deprecated Mastodon routes or deliberate extensions, so confirm upstream behavior before removing them.

## Snapshot

- tracked upstream routes: `225`
- local tracked routes: `226`
- implemented routes: `225`
- compatibility gaps: `0`
- missing routes: `0`
- extra routes: `6`
