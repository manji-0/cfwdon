# Mastodon API Compatibility

`cfwdon` の Mastodon API 互換作業を、Mastodon upstream の定義に対するマッピングとして管理する。

## Source of Truth

- Upstream route definition:
  - `https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes.rb`
  - `https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes/api.rb`
- Local route definition:
  - `crates/cfwdon-worker/src/router.rs`
- Existing project TODO:
  - `docs/full-todo.md`

`docs.joinmastodon.org` と `config/routes/api.rb` の間で deprecated endpoint の記載差分があるため、このディレクトリでは upstream の route 定義を優先する。

## Scope

初回 inventory の対象は、Mastodon の外部公開 API のうち `cfwdon` が互換対象として追う価値が高いものに絞る。

- discovery / OAuth metadata
- `/api/oembed`
- `/api/v1_alpha`
- `/api/v1`
- `/api/v2`

現時点では次は対象外にしている。

- `/api/v1/admin`, `/api/v2/admin`
- `/api/web`
- ActivityPub actor / inbox / outbox そのもの

## Status Labels

- `implemented`: upstream route と同じ path/method が `cfwdon` にある
- `compat-gap`: route はあるが、既存 TODO や実装メモ上で互換差分が分かっている
- `missing`: upstream route が `cfwdon` に無い
- `extra`: `cfwdon` にはあるが、current upstream route には無い

## Files

- `inventory.md`: upstream API 一覧と `cfwdon` マッピング
- `todo-unimplemented.md`: `missing` のみを抜き出した TODO
- `todo-compat.md`: `compat-gap` のみを抜き出した TODO
- `../scripts/generate_mastodon_api_compat.py`: inventory / TODO 再生成スクリプト

## Refresh

```bash
rtk python scripts/generate_mastodon_api_compat.py
```

## Current Extra Routes In cfwdon

current upstream の `config/routes/api.rb` には無いが、`cfwdon` にはある route。

- `GET /api/v1/timelines/direct` via `direct_timeline_response`
- `GET /api/v1/statuses/:id/card` via `status_card_response`
- `PUT /api/v2/media/:id` via `update_media_attachment`
- `PATCH /api/v2/media/:id` via `update_media_attachment`
- `GET /api/v1/follow_requests/:id` via `follow_request_response`
- `GET /api/v1/search` via `search_v1`

deprecated route を残している可能性があるので、削除ではなく upstream 側の扱いを確認してから整理する。

## Snapshot

- tracked upstream routes: `231`
- local tracked routes: `216`
- implemented routes: `193`
- compatibility gaps: `22`
- missing routes: `16`
- extra routes: `6`
