# TODO: Compatibility Gaps

このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。

route はあるが、互換性の詰めが残っているもの。

## Discovery / OAuth / Meta

- [ ] `GET /oauth/userinfo`
  userinfo claims、401、app bearer rejection は寄せたので、OAuth user token + `profile` scope semantics を upstream に寄せる。
- [ ] `POST /oauth/userinfo`
  userinfo claims、401、app bearer rejection は寄せたので、OAuth user token + `profile` scope semantics を upstream に寄せる。

## Instance / Apps / Trends

- [ ] `POST /api/v1/emails/confirmations`
  401/403 と confirmed-email state は寄せたので、mail dispatch 条件と application ownership 条件を upstream に寄せる。
- [ ] `GET /api/v1/emails/check_confirmation`
  authenticated local account の email presence で confirmation bool は返すので、unconfirmed-user/application-token semantics を upstream に寄せる。

## Timelines / Search

- [ ] `GET /api/v1/timelines/home`
  followed hashtag 混在、sorting、cursor pagination、401 invalid access token、app bearer rejection は入ったので、OAuth user token semantics を upstream に寄せる。
- [ ] `GET /api/v2/search`
  HTTP URL resolve-only semantics、exact handle prepend、short-query gating、acct-aware account ranking/following boost、popularity tie-break、profile note/bio matching、statuses の account_id/min_id/max_id filter、statuses relevance-then-recency sorting、basic status query syntax (`from/before/after/during/language/is:/has:media/poll/embed`)、hashtags offset と usage/recency ranking は寄せたので、advanced query syntax / FTS quality の差分をここで追う。
- [ ] `GET /api/v1/streaming`
  streaming transport と channel multiplexing を upstream に寄せる。
- [ ] `GET /api/v1/streaming/(*any)`
  streaming transport と channel multiplexing を upstream に寄せる。

## Statuses / Polls

- [ ] `POST /api/v1/statuses`
  quote_approval_policy の保存、account default quote policy、quote+media/poll 制約、local/remote quote state と count/list/revoke 整合、scheduled_at の validation と scheduled status persistence/media attachment expansion、idempotency echo、registered app bearer path の application_id echo は寄せたので、manual acceptance semantics と OAuth token ownership semantics を upstream に寄せる。

## Accounts / Endorsements

- [ ] `POST /api/v1/accounts`
  account registration / approval / token 発行の挙動を upstream に寄せる。

## Push Subscription

- [ ] `POST /api/v1/push/subscription`
  WebPush subscription の保存と `server_key` 応答は入ったので、実配送を upstream に寄せる。
- [ ] `GET /api/v1/push/subscription`
  WebPush subscription の保存と `server_key` 応答は入ったので、実配送を upstream に寄せる。
- [ ] `PUT /api/v1/push/subscription`
  alerts / policy 更新と `server_key` 応答は入ったので、実配送を upstream に寄せる。
- [ ] `PATCH /api/v1/push/subscription`
  alerts / policy 更新と `server_key` 応答は入ったので、実配送を upstream に寄せる。
- [ ] `DELETE /api/v1/push/subscription`
  subscription 削除は入ったので、WebPush 実配送を upstream に寄せる。

## Status Extras

- [ ] `PUT /api/v1/statuses/:id/interaction_policy`
  quote policy の保存と response 反映、local/remote quote pending 反映は寄せたので、manual acceptance semantics を upstream に寄せる。
- [ ] `PATCH /api/v1/statuses/:id/interaction_policy`
  quote policy の保存と response 反映、local/remote quote pending 反映は寄せたので、manual acceptance semantics を upstream に寄せる。
- [ ] `POST /api/v1/statuses/:id/translate`
  404/403、target `lang` semantics、response shape は寄せたので、翻訳 provider 連携を upstream に寄せる。

## Scheduled Statuses

- [ ] `GET /api/v1/scheduled_statuses`
  owner-scoped persistence、media attachment expansion、idempotency echo、pagination、registered app bearer path の application_id echo と app-owned filter は入ったので、OAuth user token semantics を upstream に寄せる。
- [ ] `GET /api/v1/scheduled_statuses/:id`
  owner-scoped persistence と 404、media attachment expansion、idempotency echo、registered app bearer path の application_id echo と app-owned filter は入ったので、OAuth user token semantics を upstream に寄せる。
- [ ] `PUT /api/v1/scheduled_statuses/:id`
  owner-scoped persistence と scheduled_at update/validation、media attachment expansion、idempotency echo、registered app bearer path の application_id echo と app-owned filter は入ったので、OAuth user token semantics を upstream に寄せる。
- [ ] `PATCH /api/v1/scheduled_statuses/:id`
  owner-scoped persistence と scheduled_at update/validation、media attachment expansion、idempotency echo、registered app bearer path の application_id echo と app-owned filter は入ったので、OAuth user token semantics を upstream に寄せる。
