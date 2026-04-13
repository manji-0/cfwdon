# Initial Roadmap

## Immediate Next Steps
1. `../rustresort/tests/e2e_mastodon_api.rs` を参考に API 互換テストを導入する。
2. outgoing activity queue を status 以外にも広げ、remote `Follow` / `Undo` を同期送信から切り離す。
3. `../rustresort/tests/e2e_mastodon_api.rs` を参考に compatibility test を追加する。
4. `GET /api/v2/search` の URL resolve / ranking と hashtag 精度を拡張する。
5. compatibility test の基盤を整えて、`home/context/search/media` の回帰を固定する。

## Completed In Bootstrap
- Cargo workspace と Worker crate の初期化
- `devbox.json` による Rust + Wrangler ベースの開発環境定義
- D1 初期 migration の追加
- `instance_settings` を読む `/api/v1/instance` の D1 配線
- Cloudflare Access ヘッダー読取りと `verify_credentials` の最小実装
- `Cf-Access-Jwt-Assertion` の署名検証、issuer/audience 検証、ローカルアカウント自動払い出し
- `/.well-known/webfinger` の JRD 応答
- `/users/:username` の最小 ActivityPub actor 応答
- account actor key の生成・保持と `publicKey` 応答
- `POST /api/v1/statuses` の最小作成フロー
  テキスト投稿、visibility、CW、language、ローカル reply、`media_ids` を D1 に保存
  `poll` は未実装のため 422 で明示的に拒否
- `POST /api/v2/media` の最小作成フロー
  multipart upload を Cloudflare Access 配下で受け、R2 に保存し、D1 に metadata を記録
- `GET /media/:id`
  Worker 経由で R2 object を公開配信
- `GET /api/v1/media/:id`
  media metadata の取得
- `PUT/PATCH /api/v1/media/:id`, `PUT/PATCH /api/v2/media/:id`
  description 更新と `focus` metadata の最小実装
- `GET /users/:username/outbox`
  public / unlisted status だけを `OrderedCollection` として返す
- `GET /users/:username/statuses/:id`
  local status を `Note` として返す
- public / unlisted status 作成時の outbound queue
  `Create` activity を D1 の `outbox_deliveries` に積む
- `/internal/outbox/process`
  generic queue を follower inbox ごとの target queue に展開し、RSA-SHA256 の HTTP Signature 付きで配送
  失敗時は backoff 付きで retry し、5 回目で terminal failure に落とす
- `followers` テーブル
  remote actor / inbox / shared inbox を保持する最小スキーマを追加
- `POST /users/:username/inbox`
  `Follow` / `Undo(Follow)` を受けて `followers` を upsert / delete し、`Accept` を署名付きで返送
  incoming HTTP Signature の `Signature` / `Date` / `Digest` / `publicKeyPem` 検証を追加
- `POST /inbox`
  shared inbox として `Follow` / `Undo(Follow)` を受け、activity payload から対象 local actor を解決
- `GET /users/:username/followers`
  remote follower actor URI を `OrderedCollection` として返す
- `GET /users/:username/following`
  local follow 未実装のため現状は空 collection を返す
- remote actor / remote status の最小保存
  signed `Create(Note)` を受けると `remote_actors`, `remote_statuses` に upsert
- `GET /api/v1/timelines/public`
  local public status と受信済み remote public status を作成日時順に返す
- `GET /api/v1/statuses/:id`
  public/unlisted は公開、private/direct は owner のみ見える Mastodon status 取得を追加
- `DELETE /api/v1/statuses/:id`
  owner 限定の削除を追加し、`delete_media=true` の即時添付削除にも対応
- `/internal/media/prune-orphans`
  未紐付け media を 24 時間経過後に R2 + D1 から掃除する内部 route を追加
- `GET /api/v1/accounts/:id`
  local account の公開プロフィール取得を追加
- `GET /api/v1/accounts/:id/statuses`
  local account の投稿一覧取得を追加
- `GET /api/v1/accounts/verify_credentials`
  follower/status count を返すように拡張
- `devbox check`
  `rustup` 管理 toolchain の `rustc` を明示するように修正
- `follows` テーブル
  local/remote outgoing follow state を保持できる最小スキーマを追加
- `POST /api/v1/accounts/:id/follow`, `POST /api/v1/accounts/:id/unfollow`
  local account 間の follow/unfollow と `Relationship` 応答を追加
- `GET /api/v1/accounts/relationships`
  local account IDs に対する relationship 取得を追加
- `GET /api/v1/accounts/lookup`
  local/remote account の lookup と remote actor cache を追加
- `GET /api/v1/accounts/search`
  local account と cache 済み remote actor の検索、`resolve=true` 時の remote lookup を追加
- `GET /api/v2/search`
  account search を土台に accounts/statuses の最小検索を追加し、未認証時は `resolve` / `following` / `offset` を拒否
- `GET /api/v1/timelines/tag/:hashtag`, `GET /api/v1/tags/:name`
  local/remote public statuses から hashtag を抽出して tag timeline / tag entity を返す最小実装を追加
- `GET /api/v1/statuses/:id/context`
  local/remote cached status の ancestor / descendant を辿る最小 thread context 実装を追加
- `GET /api/v1/timelines/home`
  Cloudflare Access 認証済み viewer 向けに self + accepted follows の local/remote statuses を時系列マージする最小 home timeline を追加
- `GET /api/v2/instance`
  conservative な capability 広告で `api_versions` / `registrations` / media/status limits を返し、`SOURCE_URL` / `INSTANCE_LANGUAGES` / `CONTACT_EMAIL` / `INSTANCE_THUMBNAIL_URL` で補足 metadata を設定可能にした
- `PATCH /api/v1/accounts/update_credentials`
  Cloudflare Access 認証済み account の `display_name` / `note` / `source[privacy,sensitive,language]` を更新し、`verify_credentials` でも `source` を返すようにした
- `GET /api/v1/instance`
  custom summary ではなく Mastodon `V1::Instance` 形状に寄せ、local account/status count と known remote domain count を返すようにした
- `POST /api/v1/accounts/:id/block`, `POST /api/v1/accounts/:id/unblock`
  local block state を D1 に保存し、`GET /api/v1/accounts/relationships` の `blocking` / `blocked_by` に反映するようにした
- `POST /api/v1/statuses/:id/favourite`, `POST /api/v1/statuses/:id/unfavourite`, `GET /api/v1/favourites`
  local/remote cached status への favourite state を D1 に保存し、status entity の `favourited` / `favourites_count` と favourites timeline に反映するようにした
- `POST /api/v1/statuses/:id/bookmark`, `POST /api/v1/statuses/:id/unbookmark`, `GET /api/v1/bookmarks`
  local/remote cached status の bookmark state を D1 に保存し、status entity の `bookmarked` と bookmarks timeline に反映するようにした
- `GET /api/v1/notifications`
  最小 slice として `follow` / `favourite` / `reblog` 通知を返し、`types[]` / `exclude_types[]` / `account_id` の最低限フィルタに対応した
- `POST /api/v1/statuses/:id/reblog`, `POST /api/v1/statuses/:id/unreblog`
  local/remote cached status の boost state を D1 に保存し、status entity の `reblogged` / `reblogs_count` に反映するようにした
- `POST /api/v1/accounts/:id/mute`, `POST /api/v1/accounts/:id/unmute`, `GET /api/v1/mutes`
  local/remote actor の mute state を D1 に保存し、`relationships` の `muting` 系と home timeline / notifications の最小 filter に反映するようにした
- local mention extraction
  local status 本文から local mention を抽出し、status entity の `mentions` と `GET /api/v1/notifications` の `mention` 最小実装に反映するようにした
- remote mention extraction
  remote status HTML から local/remote mention を抽出し、status entity の `mentions` と `GET /api/v1/notifications` の remote `mention` 最小実装に反映するようにした
- local status delete federation
  `DELETE /api/v1/statuses/:id` 実行時に follower 向け ActivityPub `Delete` を outbox queue に積むようにした
- incoming remote delete
  personal/shared inbox で signed `Delete` を受けたとき、owner 一致する cached remote status を削除するようにした
- incoming remote update
  personal/shared inbox で signed `Update(Note)` を受けたとき、audience が合う cached remote status を upsert 更新するようにした
- remote interaction federation
  remote status への favourite/reblog 時に targeted outbound `Like` / `Announce` を送り、unfavourite/unreblog では保存済み activity id を使って `Undo` を送るようにした
- incoming remote interactions
  personal/shared inbox で signed `Like` / `Announce` / `Undo` を受けたとき、local status 向けの remote interaction を保存・削除し、status count と notification に反映するようにした
- remote actor key cache
  incoming HTTP Signature 検証で `remote_actors` にある inbox/public key を優先利用し、cache 不一致時だけ actor を再 fetch するようにした
- inbox idempotency
  signed inbox activity の `id` を D1 に記録し、成功済み activity の再処理を避ける最小 replay protection を入れた
- collection paging
  `GET /users/:username/followers` と `GET /users/:username/following` で `OrderedCollectionPage` を返せる最小 paging を追加した
- remote actor profile media cache
  remote actor fetch 時に `icon` / `image` から avatar/header URL を抽出して D1 に保存し、cached remote account entity に反映するようにした
- compatibility test baseline
  response builder を対象に `verify_credentials` / status / relationship / instance v1/v2 の JSON shape を固定する最小 compat test を追加した
- mutes pagination
  `GET /api/v1/mutes` で D1 `rowid` を内部 cursor とする `max_id` / `since_id` と `Link` header を返す最小 pagination を追加した
- targeted outbound activity queue
  remote `Follow` / `Undo` / `Like` / `Announce` / `Accept` の queue 投入を helper に寄せ、inbox follow の `Accept` も retry 可能な `outbound_activities` に載せるようにした
- search v2 URL resolve
  `resolve=true` 時に handle だけでなく actor URL / status URL も解決し、remote status URL は cache miss なら ActivityPub document fetch から D1 cache まで行う最小 path を追加した
- search v2 hashtag resolve
  `resolve=true&type=hashtags` 時に `#tag` と `/tags/:name` URL を exact tag 名へ解決し、既存 tag response builder を返せるようにした
- search pagination / ranking
  accounts 検索は local/remote を merge した後に `offset` / `limit` を適用するように直し、accounts/hashtags ともに exact/prefix match を優先する単純 ranking を入れた
- tag history aggregation speed-up
  tag response は local/remote status row を実際に読み出して数えるのをやめ、D1 の `COUNT(*)` / `COUNT(DISTINCT ...)` で uses/accounts を返すようにした
- instance metadata integration
  `/.well-known/nodeinfo`, `/nodeinfo/2.0`, `/api/v1/instance/peers`, `/api/v1/instance/privacy_policy`, `/api/v1/instance/terms_of_service`, `/api/v1/instance/extended_description` を追加し、`/api/v2/instance` の URL fields も設定済み endpoint に結びつけた
- notification state endpoints
  `GET /api/v1/notifications/:id`, `POST /api/v1/notifications/:id/dismiss`, `POST /api/v1/notifications/clear`, `GET /api/v1/notifications/unread_count` を追加し、D1 に dismiss/clear state を保存する最小実装を入れた
- notification status type
  `notify=true` の local/remote follow を対象に `GET /api/v1/notifications` の `status` type を追加し、follow の更新時刻以降の投稿を通知として返すようにした
- account profile metadata
  `PATCH /api/v1/accounts/update_credentials` で `fields_attributes` / `discoverable` を保存し、`verify_credentials` / account entity / ActivityPub actor `attachment` に反映するようにした
- account directory
  `GET /api/v1/directory` で local discoverable account を `active` / `new` の最小 order で返すようにした
- outgoing profile update federation
  local profile 更新時に remote follower 向け ActivityPub `Update(Person)` を queue し、avatar/header/fields/discoverable の変化を配送できるようにした
- incoming profile update federation
  personal/shared inbox で signed `Update(Person)` を受けたとき、accepted local follow がある remote actor の cached profile を refresh するようにした
- local polls
  `POST /api/v1/statuses` で local poll を保存し、local status entity の `poll` field と `GET /api/v1/polls/:id`, `POST /api/v1/polls/:id/votes` の最小実装を追加した
- poll notifications
  expired した local poll について、作成者または投票済み account に `GET /api/v1/notifications` の `poll` type を返す最小実装を追加した
- admin sign-up notifications
  `ADMIN_EMAILS` に含まれる local account に対して、新規 local account 作成を `GET /api/v1/notifications` の `admin.sign_up` type で返す最小実装を追加した
- reports / admin.report notifications
  `POST /api/v1/reports` で local/remote account への report 作成を受け付け、admin account には `GET /api/v1/notifications` の `admin.report` type と `Report` entity を返す最小実装を追加した
- remote question polls
  incoming `Create/Update(Question)` を read-only remote poll として保存し、remote status entity と `GET /api/v1/polls/:id` で参照できるようにした
- remote poll voting
  `POST /api/v1/polls/:id/votes` で cached remote `Question` に対する ActivityPub vote (`Create(Note{name,inReplyTo})`) を送信し、local 側では own_votes を追跡する最小実装を追加した
- incoming remote poll votes
  local poll status に対する incoming `Create(Note)` vote を受け付け、`status_poll_votes` に remote actor 単位で反映するようにした
- local question federation
  local poll status を ActivityPub `Question` として `/users/:username/statuses/:id` と outbox `Create` object に載せ、`oneOf` / `anyOf`, `endTime`, `closed`, `votersCount` を広告するようにした
- local question update federation
  local/remote vote で local poll count が変わったとき、follower 向けに outgoing `Update(Question)` を queue して remote cache 側の poll count refresh を促す最小実装を追加した
- expired poll close federation
  `/internal/polls/process-expired` を追加し、期限到達した public/unlisted local poll を一度だけ `Update(Question)` で再配信して `closed` 状態を remote に伝えられるようにした
- incoming remote poll vote undo
  remote actor からの `Undo(Create(Note vote))` を local poll vote の取り消しとして受け付け、票数を戻したうえで follower 向け `Update(Question)` を再送する最小実装を追加した
- remote poll own_votes remap
  remote poll vote 保存時に option title も保持し、remote `Question` update で選択肢順が変わっても `own_votes` を現行 option 配列へ title 優先で再マップし、解決不能になった stale vote track は prune するようにした
- remote follow terminal failure reconciliation
  `outbound_activities` の terminal failure で remote `Follow` の pending state を `failed` に落とし、retry 枯渇後に relationship が `requested` のまま残らないようにした
- `PATCH /api/v1/accounts/update_credentials`
  multipart avatar/header upload を受けて R2 に profile media を保存し、account entity と ActivityPub actor の `icon` / `image` に反映するようにした
- outgoing remote `Follow` / `Undo(Follow)`
  remote actor に署名付き follow/unfollow を送信し、`Accept` / `Reject` 受信で state を更新
- DNS 解決ベースの SSRF ガード
  remote WebFinger / actor / inbox / public key URL を DoH で A/AAAA 解決し、private/loopback/link-local 宛先を拒否
- `GET /users/:username/following`
  accepted follow を `OrderedCollection` として返すように拡張
- `POST /users/:username/inbox`, `POST /inbox`
  `Accept` / `Reject` 受信時に follow state を更新する最小実装を追加
- private status API visibility
  local follower に対して `GET /api/v1/statuses/:id` と `GET /api/v1/accounts/:id/statuses` の followers-only 可視性を反映

## First Compatibility Slice
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
- Cloudflare Auth 経由の `me -> local account` 解決

## Concrete Reference Sources
- Mastodon endpoint shape
  `../rustresort/src/api/mastodon/instance.rs`
  `../rustresort/src/api/mastodon/accounts.rs`
  `../rustresort/src/api/mastodon/statuses.rs`
  `../rustresort/src/api/mastodon/media.rs`
- Federation flow
  `../rustresort/src/federation/webfinger.rs`
  `../rustresort/src/federation/signature.rs`
  `../rustresort/src/federation/delivery.rs`
- Compatibility test ideas
  `../rustresort/tests/e2e_mastodon_api.rs`
  `../rustresort/tests/e2e_activitypub.rs`

## Deferred Until After First Slice
- フィルタ、通知、投票、ブックマーク
- 管理 UI
- WebPush
- Mastodon OAuth クライアント互換
