# TODO: Compatibility Gaps

このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。

route はあるが、互換性の詰めが残っているもの。

## Discovery / OAuth / Meta

- [ ] `GET /oauth/userinfo`
  OAuth scope と claims の定義を upstream に寄せる。
- [ ] `POST /oauth/userinfo`
  OAuth scope と claims の定義を upstream に寄せる。

## Instance / Apps / Trends

- [ ] `GET /api/v1/apps/verify_credentials`
  response shape は寄せたので、bearer application token の検証を upstream に寄せる。
- [ ] `POST /api/v1/emails/confirmations`
  auth gate は入ったので、mail dispatch 条件と application ownership 条件を upstream に寄せる。
- [ ] `GET /api/v1/emails/check_confirmation`
  boolean response と auth gate は入ったので、email confirmation 状態判定を upstream に寄せる。

## Timelines / Search

- [ ] `GET /api/v1/timelines/link`
  link timeline の trending 判定と article 単位の抽出精度を upstream に寄せる。
- [ ] `GET /api/v1/timelines/home`
  followed hashtag 混在、sorting、cursor pagination は入ったので、access control settings 差分を upstream に寄せる。
- [ ] `GET /api/v1/timelines/tag/:hashtag`
  tag filter / pagination / remote 混在は入っているので、public preview access control settings を upstream に寄せる。
- [ ] `GET /api/v2/search`
  `docs/full-todo.md` にある URL resolve / hashtag resolve / ranking の差分をここで追う。
- [ ] `GET /api/v1/streaming`
  streaming transport と channel multiplexing を upstream に寄せる。
- [ ] `GET /api/v1/streaming/(*any)`
  streaming transport と channel multiplexing を upstream に寄せる。

## Statuses / Polls

- [ ] `GET /api/v1/statuses/:id/context`
  ancestor / descendant の組み立ては改善済み。未認証時の limit と `Mastodon-Async-Refresh` を upstream に寄せる。
- [ ] `POST /api/v1/statuses/:id/quotes/:id/revoke`
  quote revoke の remote 連携と response semantics を upstream に寄せる。
- [ ] `GET /api/v1/polls/:id`
  private / remote permissions と remote poll の扱いを upstream に寄せる。
- [ ] `POST /api/v1/polls/:id/votes`
  local / remote vote の反映、取り消し、再解決の精度を上げる。

## Accounts / Profile

- [ ] `PATCH /api/v1/accounts/update_credentials`
  profile fields / media / ActivityPub `Update(Person)` 反映を含めて upstream 挙動に寄せる。
- [ ] `GET /api/v1/directory`
  remote discoverable account は混在したので、ordering と ranking 精度を upstream に寄せる。

## Accounts / Endorsements

- [ ] `GET /api/v1/accounts/:id/endorsements`
  remote account の featured collection 取得を含めて account endorsement 一覧を upstream に寄せる。
- [ ] `POST /api/v1/accounts`
  account registration / approval / token 発行の挙動を upstream に寄せる。
- [ ] `POST /api/v1/accounts/:id/remove_from_followers`
  follower removal の federation semantics を upstream に寄せる。

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
  response shape ではなく、quote policy の保存と response への反映を upstream に寄せる。
- [ ] `PATCH /api/v1/statuses/:id/interaction_policy`
  response shape ではなく、quote policy の保存と response への反映を upstream に寄せる。
- [ ] `POST /api/v1/statuses/:id/translate`
  response shape は寄せたので、翻訳 provider 連携と target language semantics を upstream に寄せる。

## Scheduled Statuses

- [ ] `GET /api/v1/scheduled_statuses`
  一覧 shape ではなく永続化と pagination を upstream に寄せる。
- [ ] `GET /api/v1/scheduled_statuses/:id`
  detail shape は寄せたので、永続化と ownership 404 を upstream に寄せる。
- [ ] `PUT /api/v1/scheduled_statuses/:id`
  detail shape は寄せたので、scheduled_at 更新 semantics と validation を upstream に寄せる。
- [ ] `PATCH /api/v1/scheduled_statuses/:id`
  detail shape は寄せたので、scheduled_at 更新 semantics と validation を upstream に寄せる。
- [ ] `DELETE /api/v1/scheduled_statuses/:id`
  削除 response shape ではなく、scheduled status delete semantics を upstream に寄せる。
