# Full TODO

## Principles
- GoToSocial の責務分離を参考にしつつ、Workers + D1 + R2 向けに作り直す。
- `../rustresort` は API 形状と federation 分割の参照に使うが、仕様確認を挟んで採用する。
- 初期段階では「互換 endpoint を返すこと」より「署名・可視性・所有権が壊れていないこと」を優先する。

## Done
- Cargo workspace 初期化
- `devbox` ベースの開発環境
- Cloudflare Access JWT 検証
- local actor key 生成と `publicKey` 広告
- WebFinger
- actor / outbox / local status object
- media upload / read / metadata update
- local status create
- follower 保存
- personal inbox / shared inbox
- incoming HTTP Signature 最小検証
- outbound queue / target expansion / signed delivery
- followers / following collection
- remote actor / remote Note 保存
- public timeline の local+remote merge
- `GET /api/v1/statuses/:id`
- `DELETE /api/v1/statuses/:id`
- orphan media cleanup
- `GET /api/v1/accounts/:id`
- `GET /api/v1/accounts/:id/statuses`
- `verify_credentials` counts
- `devbox check` の toolchain/rustc 固定化
- local `follows` テーブル
- local follow / unfollow API
- `GET /users/:username/following` 実データ化
- `POST /users/:username/inbox` の `Accept` / `Reject` 最小反映
- `GET /api/v1/accounts/relationships`
- `GET /api/v1/accounts/lookup`
- outgoing remote `Follow`
- outgoing remote `Undo(Follow)`
- remote relationship 解決の最小実装
- DNS 解決ベースの SSRF 防御
- `GET /api/v1/accounts/search`
- `GET /api/v2/search` の最小実装
- authenticated status search の D1 `LIKE` ベース実装
- `GET /api/v1/timelines/tag/:hashtag` の最小実装
- `GET /api/v1/tags/:name` の最小実装
- `GET /api/v2/search` の hashtag search 最小実装
- status entity の hashtag 抽出
- `GET /api/v1/statuses/:id/context` の最小実装
- `GET /api/v1/timelines/home` の最小実装
- media `focus` metadata の保存と応答
- `GET /api/v2/instance` の最小実装
- `SOURCE_URL` / `INSTANCE_LANGUAGES` / `CONTACT_EMAIL` / `INSTANCE_THUMBNAIL_URL` による instance metadata 設定
- `PATCH /api/v1/accounts/update_credentials` の最小実装
- `GET /api/v1/accounts/verify_credentials` の `source` 応答
- account profile defaults (`bio_text` / default visibility / sensitive / language) の保存
- `PATCH /api/v1/accounts/update_credentials` の avatar/header upload
- account entity の avatar/header 実データ化
- ActivityPub actor の `icon` / `image` 最小実装
- `GET /api/v1/instance` の Mastodon-compatible shape への拡張
- `POST /api/v1/accounts/:id/block`
- `POST /api/v1/accounts/:id/unblock`
- `GET /api/v1/accounts/relationships` の blocking / blocked_by 実データ化
- `POST /api/v1/statuses/:id/favourite`
- `POST /api/v1/statuses/:id/unfavourite`
- `GET /api/v1/favourites`
- local/remote cached status の `favourited` / `favourites_count`
- `POST /api/v1/statuses/:id/bookmark`
- `POST /api/v1/statuses/:id/unbookmark`
- `GET /api/v1/bookmarks`
- local/remote cached status の `bookmarked`
- `GET /api/v1/notifications` の最小実装
- `notifications` の `follow` / `favourite` / `reblog` 最小実装
- `POST /api/v1/statuses/:id/reblog`
- `POST /api/v1/statuses/:id/unreblog`
- local/remote cached status の `reblogged` / `reblogs_count`
- `POST /api/v1/accounts/:id/mute`
- `POST /api/v1/accounts/:id/unmute`
- `GET /api/v1/mutes`
- `GET /api/v1/accounts/relationships` の muting state 実データ化
- home timeline / notifications の mute filter 最小実装
- local status entity の local mention 抽出
- `GET /api/v1/notifications` の `mention` 最小実装
- local/remote status entity の `mentions` 最小実装
- `GET /api/v1/notifications` の remote `mention` 最小実装
- local status delete 時の outgoing `Delete`
- `POST /users/:username/inbox` / `POST /inbox` の `Delete`
- `POST /users/:username/inbox` / `POST /inbox` の `Update(Note)`
- remote status に対する outgoing `Like` / `Undo(Like)`
- remote status に対する outgoing `Announce` / `Undo(Announce)`
- `POST /users/:username/inbox` / `POST /inbox` の `Like`
- `POST /users/:username/inbox` / `POST /inbox` の `Announce`
- local status の remote `favourite` / `reblog` counts 反映
- `GET /api/v1/notifications` の remote `favourite` / `reblog` 最小実装
- incoming signature 検証時の remote actor public key cache 利用
- `GET /users/:username/followers` paging
- `GET /users/:username/following` paging
- inbox replay protection / idempotency table の最小実装
- remote actor avatar/header cache
- compatibility test の最小導入
- `GET /api/v1/mutes` pagination
- outgoing activity queue の targeted remote activity 共通化
- `GET /api/v2/search` の URL resolve 拡張
- remote follow の retry/backoff queue 化
- `GET /api/v2/search` の hashtag resolve 拡張
- search pagination / ranking の改善
- tag history 集計の高速化
- `GET /api/v1/instance` の nodeinfo / peers / privacy policy / terms 連携
- `GET /api/v1/notifications/:id`, `POST /api/v1/notifications/:id/dismiss`, `POST /api/v1/notifications/clear`, `GET /api/v1/notifications/unread_count`
- `GET /api/v1/notifications` の `status` type 最小実装
- account `discoverable` / profile fields の保存と `verify_credentials` / account entity / ActivityPub actor `attachment` 反映
- `GET /api/v1/directory` の local discoverable account 最小実装
- `PATCH /api/v1/accounts/update_credentials` 時の outgoing ActivityPub `Update(Person)`
- `POST /users/:username/inbox` / `POST /inbox` の `Update(Person)` で remote actor profile refresh
- local status poll の保存と status entity への `poll` 応答
- `GET /api/v1/polls/:id`, `POST /api/v1/polls/:id/votes` の local poll 最小実装
- `GET /api/v1/notifications` の local `poll` type 最小実装
- `GET /api/v1/notifications` の `admin.sign_up` 最小実装 (`ADMIN_EMAILS` ベース)
- `POST /api/v1/reports` の最小実装
- `GET /api/v1/notifications` の `admin.report` 最小実装
- remote `Question` の受信保存と remote status entity / `GET /api/v1/polls/:id` への read-only poll 反映
- `POST /api/v1/polls/:id/votes` の remote poll 最小実装
- incoming `Create(Note)` による local poll への remote vote 最小反映
- local poll を ActivityPub `Question` として `/users/:username/statuses/:id` / outbox `Create` に反映
- local poll vote 時の outgoing `Update(Question)` 最小実装
- expired local poll を一度だけ `Update(Question)` で閉鎖反映する internal job
- incoming `Undo(Create(Note vote))` による local poll vote 取り消し最小反映
- remote poll `own_votes` を option title 優先で再解決し、remote `Question` の選択肢並び替えや option 削除後の stale vote を吸収

## Next Up
- ActivityPub `Question` update での remote vote refresh / option rename 精度の拡張

## Mastodon API
- `GET /api/v1/directory` の remote directory / ordering 精度拡張
- `GET /api/v1/notifications` の mention / reblog / poll / admin 系拡張
- `GET /api/v1/polls/:id` の private/remote permissions 精度拡張

## ActivityPub
- `POST /inbox` の `Create` paging / dedupe / replay 防止
- ActivityPub `Question` vote refresh / remote poll update

## Storage / Data
- pending follow requests
- remote media attachment 保存方針
- remote object dedupe
- status tombstone / soft delete
- retry dead-letter 状態
- migration test

## Security
- actor fetch の DNS 解決ベース SSRF 防御
- shared inbox abuse rate limit
- signature clock skew 設定化
- digest / header canonicalization hardening
- Cloudflare Access 保護 route の整理

## Media Delivery Notes
- 現状の `/media/:id` は Worker が `bucket.get()` した body を返しているため、Cloudflare の main request cache に自動で確実に乗る前提にはしない
- Worker 経由で確実に edge cache を使いたい場合は `caches.default` か `fetch()` ベースの cache 制御が必要
- ただし Cache API は Cloudflare Access fronted Worker では使えない制約があるため、public media route は Access から外すか、R2 custom domain + cache rule / fetch subrequest 方式を検討する
- Tiered Cache を使いたい場合は `bucket.get()` 直返しより、cache rule が効く `fetch()` 経路または R2 custom domain 配信を優先する

## Ops / DX
- `wrangler dev` 用 seed script
- D1 migration runner script
- structured logging
- compatibility fixtures
- e2e API tests
- federation interop tests
- CI
