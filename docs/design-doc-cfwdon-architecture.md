# cfwdon Architecture

## Summary
- `cfwdon` は Rust 製の Mastodon 互換サーバーを Cloudflare Workers 上に載せるための実装である。
- 実装方針は GoToSocial の責務分離を参考にするが、Go の実装を移植するのではなく、Workers + D1 + R2 に合わせて再設計する。
- 最初の段階では、HTTP エッジ層、ドメインモデル、設定/共通エラーの境界を先に作り、D1 と R2 は後続マイルストーンで接続する。

## Goals
- Cloudflare Workers 上で動作する Mastodon 互換 API サーバーを Rust で実装する。
- 永続化は D1、メディア保存は R2、公開配信や非同期処理は Workers の制約内で運用できる設計にする。
- GoToSocial のように責務を明確に分割し、ActivityPub 処理、API 処理、メディア処理を疎結合に保つ。
- 将来的に単一 Worker から複数 crate へ自然に拡張できる Cargo workspace を先に整える。

## Non-Goals
- GoToSocial のコードを機械的に移植すること。
- 初期段階で Mastodon API と ActivityPub の全互換を目指すこと。
- 初回コミットで管理画面、通知、全文検索、推奨アルゴリズムまで揃えること。
- Cloudflare Workers の制約を無視して長時間ジョブ前提の構成にすること。

## Background / Context
- 参照先の GoToSocial は `cmd` と `internal` を分離し、その下で `api`, `db`, `federation`, `media`, `oauth`, `processing`, `router`, `state`, `storage`, `workers` などの責務ごとにパッケージを分割している。
- この構成は「HTTP 入口」「共有状態」「非同期処理」「永続化」「連合処理」の境界が明快で、Mastodon 互換サーバーに必要な複雑さを整理しやすい。
- 一方で Cloudflare Workers では長寿命プロセス、常駐ジョブキュー、ローカルファイルシステム、TCP 常時接続などの前提が置けないため、そのままの構成は適さない。
- そのため `cfwdon` では GoToSocial の責務分割を借りつつ、Workers ランタイムに合わせて単一エッジエントリポイントから内側へ依存が流れる構成を採用する。
- さらにローカル参照実装として `../rustresort` を使う。特に `src/api/mastodon/`, `src/federation/`, `docs/API.md`, `docs/FEDERATION.md`, `migrations/` は Mastodon 互換面と ActivityPub 面の具体的な実装順を決める際の一次参照にする。

## Requirements
### Functional
- Mastodon 互換の基本 API を段階的に実装する。
- ActivityPub の送受信を処理できる。
- ローカルアカウント、ステータス、フォロー関係、添付メディアを保持できる。
- メディアアップロードを受け付けて R2 に保存できる。
- D1 にローカル状態を永続化できる。
- 認証済みの管理系・投稿系 API は Cloudflare Auth で保護できる。

### Non-Functional
- Workers 制約を前提に短時間リクエストで完結すること。
- 単一責務の crate 境界を維持し、将来的な API 追加で破綻しないこと。
- D1 と R2 のバインディング欠如時に明確な設定エラーを返せること。
- 依存関係は `worker` crate に寄せ、Cloudflare 固有コードをエッジ層へ閉じ込めること。
- アプリケーション内で OAuth サーバーを持たず、認証責務を Cloudflare 側へ寄せること。

## Proposed Design
### Architecture Overview
- `crates/cfwdon-worker`
  Workers エントリポイント。HTTP ルーティング、Cloudflare の `Env` バインディング解決、レスポンス整形を担当する。
- `crates/cfwdon-core`
  設定値、ビルドメタデータ、共通エラーなど、プラットフォーム非依存の基礎型を置く。
- `crates/cfwdon-domain`
  アカウント、ステータス、メディア、インスタンス表現などのドメイン型を置く。
- 将来的な追加候補
  `cfwdon-application`, `cfwdon-federation`, `cfwdon-storage-d1`, `cfwdon-storage-r2`。

### Development Environment
- 開発環境管理は `devbox.json` を入口にする。
- `rustup`, `wasm32-unknown-unknown`, `wrangler`, `pkg-config`, `openssl`, `wasm-bindgen-cli`, `binaryen`, `cargo-generate`, `jq` を devbox package として定義し、Workers 向け Wasm ビルドと `worker-build` を再現可能にする。
- `rust-toolchain.toml` は保持し、devbox shell の `init_hook` で `rustup` に stable toolchain と `wasm32-unknown-unknown` target を揃えさせる。

### Mapping From GoToSocial
- `router` -> `cfwdon-worker`
- `state` -> Worker の `Env` と今後追加する `AppState`
- `processing` -> 将来の `cfwdon-application`
- `db` -> 将来の D1 アダプタ crate
- `storage` / `media` -> 将来の R2 アダプタと media service
- `federation` -> 将来の ActivityPub 送受信モジュール
- `workers` -> Queue / Cron / `waitUntil` を使った非同期ジョブ層

### Mapping From RustResort
- `../rustresort/src/api/mastodon/*.rs`
  Mastodon 互換エンドポイントの網羅順と DTO 粒度の参照元。
- `../rustresort/src/federation/*.rs`
  WebFinger、HTTP Signature、delivery、key cache の責務分割の参照元。
- `../rustresort/migrations/*.sql`
  `accounts`, `statuses`, `media`, `follow_requests`, `activitypub_event_state` などのスキーマ候補。
- `../rustresort/tests/e2e_*.rs`
  互換 API の優先順位を決めるための実装チェックリスト。

### Data Model
- 初期モデルは `AccountHandle`, `StatusId`, `MediaAttachment`, `InstanceSummary` を定義する。
- D1 永続化時は以下の主テーブルから始める。
  `accounts`, `statuses`, `media_attachments`, `follows`, `inboxes`, `outbox_deliveries`
- R2 にはメディアの本体だけを保存し、D1 側には object key と MIME 情報を保存する。
- filters、lists、polls、notifications については `../rustresort/migrations/003` 以降の段階的分離を参考にして、`cfwdon` でも migration を細かく刻む。

### Authentication Model
- ユーザー向けの認証は Cloudflare Auth を前提にする。
- `cfwdon` 自身は OAuth authorization server や access token 発行機能を持たない。
- 保護対象のルートでは Cloudflare Access などが付与する認証済みコンテキストを受け取り、対応するローカルアカウントへマップする。
- 公開ルートと連合ルートはアプリ内認証なしで公開し、署名検証が必要な ActivityPub リクエストだけ別途検証する。
- この方針により、Mastodon 互換 API のうち OAuth クライアント登録と bearer token 発行を前提とする部分は非対応または Cloudflare 側認証に置換される。
- 初期スキャフォールドでは `Cf-Access-Jwt-Assertion` を Worker 内で検証し、Cloudflare Access の JWK 取得、issuer/audience 検証、email claim 照合を fail-closed にする。
- 必須設定は `ACCESS_TEAM_DOMAIN` と `ACCESS_AUD` とし、Cloudflare Access 未設定環境では保護 API を通さない。

### APIs / Interfaces
- 初期実装で用意するもの
  `/`
  `/healthz`
  `/api/v1/instance`
  `/api/v2/instance`
  `/api/v1/accounts/verify_credentials`
  `/api/v1/accounts/update_credentials`
  `/.well-known/webfinger`
  `/users/:username`
  `POST /api/v1/statuses`
  `POST /api/v2/media`
  `/media/:id`
  `/api/v1/media/:id`
  `/api/v2/media/:id`
  `/users/:username/outbox`
  `/users/:username/statuses/:id`
- 次段階で追加するもの
  `/api/v1/statuses/*`
  `/inbox`
  `/outbox`
- Mastodon API の追加順は `../rustresort/docs/API.md` と `../rustresort/src/api/mastodon/` の分割に合わせ、`instance -> accounts -> statuses -> timelines -> media` の順で進める。
- ActivityPub の追加順は `../rustresort/docs/FEDERATION.md` に合わせ、`WebFinger -> actor -> object -> inbox verification -> outbound delivery` の順で進める。
- ただし `rustresort` が返している `profile-page` link や未実装 endpoint の広告はそのまま踏襲せず、`cfwdon` では実装済みの `self` link と actor URL のみをまず返す。
- local account には Worker で生成した RSA 鍵を保持し、actor には `publicKey.id`, `publicKey.owner`, `publicKeyPem` を載せる。
- 秘密鍵は当面 D1 に JWK 文字列として保持し、後続の outbound HTTP Signature 実装で再 import して使う。
- `POST /api/v1/statuses` の初期互換スライスは text/CW/visibility/language/local reply/`media_ids` に対応し、`poll` は未対応として 422 を返す。
- `POST /api/v2/media` は multipart の `file` を受け、R2 に object を保存し、D1 に `media_attachments` 行を作る。
- upload 応答の `url` は R2 custom domain を前提にし、`MEDIA_PUBLIC_BASE_URL` 配下の object key URL を返す。
- Worker の `/media/:id` は後方互換の fallback / redirect として残し、公開配信の正規 URL には使わない。
- media metadata 更新は `PUT/PATCH /api/v1/media/:id` と `PUT/PATCH /api/v2/media/:id` で description と `focus` に対応する。
- ActivityPub object 表現として `GET /users/:username/statuses/:id` は local status を `Note` で返し、`GET /users/:username/outbox` は public / unlisted のみを `OrderedCollection` として返す。
- public / unlisted status の作成時には outbound 用 `Create` payload を `outbox_deliveries` に queue し、後続の fan-out / retry worker が配送を担当する前提にする。

### Data Flow
- HTTP リクエストは Worker に入り、ルーティング後に設定解決と入力検証を行う。
- ドメイン操作は application 層へ委譲し、application 層が D1/R2/連合送信を組み合わせる。
- 即時応答に不要な処理は `waitUntil` または Workers Queue に逃がす。
- ActivityPub 配信結果や再試行情報は D1 に書き戻す。
- 初期配送プロセッサは `/internal/outbox/process` を Cloudflare Access 配下に置き、generic な `Create` queue を follower inbox 単位の job に展開して処理する。
- target ごとの delivery row は `activity_id + target_inbox` を一意にし、同一アクティビティの重複 fan-out を抑止する。
- 署名は local account の D1 保存 JWK を Worker 内で再 import し、`(request-target) host date digest` を RSA-SHA256 で署名する。
- inbox 受信の最小実装では `/users/:username/inbox` で `Follow` / `Undo(Follow)` を処理し、remote actor を dereference して `followers` を更新する。
- actor document には `endpoints.sharedInbox` を載せ、`POST /inbox` でも同じ follow/unfollow 処理を受けられるようにする。
- actor が広告する `/followers` と `/following` も初期段階から 200 を返す。`followers` は D1 の follower actor URI を返し、`following` は outgoing follow 実装まで空 collection とする。
- signed `Create(Note)` を受信したら remote actor/profile を `remote_actors` に、Note 本体を `remote_statuses` に保存する。
- `GET /api/v1/timelines/public` は local `statuses` と `remote_statuses` をマージして返し、初期段階では public visibility のみを対象にする。
- `Follow` 受信時は local actor の鍵で `Accept` を署名送信する。
- incoming request では `Signature` ヘッダの `keyId` を actor と突き合わせ、actor document の `publicKeyPem` で署名検証する。あわせて `Date` と `Digest` を fail-closed に検証する。
- ただし actor fetch に対する SSRF ガードは現時点では scheme / host / localhost / private-IP-literal の粗いチェックに留まる。DNS 解決ベースの防御は後続で追加する。

## Operational Plan
### Deployment / Environments
- 単一 Cloudflare Worker として配備する。
- `wrangler.toml` で `INSTANCE_*` 変数を管理する。
- instance metadata は `SOURCE_URL`, `INSTANCE_LANGUAGES`, `CONTACT_EMAIL`, `INSTANCE_THUMBNAIL_URL` で補足できるようにする。
- media 配信は R2 custom domain を必須前提にし、`MEDIA_PUBLIC_BASE_URL` を環境変数として定義する。
- public media domain は Cloudflare Access の保護対象から外し、Cache Rules / Smart Tiered Cache を適用する。
- 保護 API の運用には `ACCESS_TEAM_DOMAIN`, `ACCESS_AUD`, `ACCESS_EMAIL_HEADER`, `ACCESS_JWT_HEADER` を環境変数として定義する。
- D1 と R2 は本番 deploy 前に binding を追加する。

### Observability
- まずは構造化 JSON ログを優先する。
- 後続で request ID、actor ID、delivery target を含むイベントログを追加する。
- D1 クエリ失敗、R2 保存失敗、federation delivery 失敗を主要監視イベントにする。

### Reliability / Failure Modes
- D1 バインディング未設定時は 500 ではなく設定不備として検出しやすいメッセージを返す。
- R2 保存と D1 書き込みの片側失敗に備え、メディア確定前状態を持つ。
- 連合配送は冪等ジョブとして再実行可能にする。

### Security / Privacy
- Cloudflare Auth と ActivityPub 署名検証は最初から設計に含める。
- 秘密鍵や API secret は Worker secret で管理する。
- 非公開投稿の可視性判定はドメイン層ではなく application 層で統一する。

## Rollout / Migration Plan
- Phase 0
  Cargo workspace、Worker エントリポイント、設計文書を作る。
- Phase 1
  D1 スキーマ、インスタンス情報、アカウント作成、ローカル投稿作成を実装する。
- Phase 2
  R2 メディア、WebFinger、ActivityPub actor/object 表現を実装する。
- Phase 3
  inbox/outbox、配送再試行、フォロー関係、ホームタイムラインを実装する。
- Phase 4
  Mastodon API 互換性の穴埋め、運用監視、管理機能を追加する。
- 各 phase の仕様差分確認には `../rustresort/tests/e2e_mastodon_api.rs`, `../rustresort/tests/e2e_activitypub.rs`, `../rustresort/tests/e2e_wellknown.rs` を参照して抜け漏れを潰す。

## Alternatives Considered
- 単一 crate で全部実装する案
  立ち上がりは速いが、Mastodon 互換 API と ActivityPub を同時に広げる段階で境界が崩れやすい。
- 最初から 6 以上の crate に細分化する案
  依存関係は綺麗だが、初期段階では保守コストが高い。
- Cloudflare 固有型をドメイン層まで流す案
  実装は楽になるが、テスト性と移植性が悪化する。
- OAuth をアプリ内実装する案
  Mastodon クライアント互換性は上がるが、今回の前提では Cloudflare 側で認証を完結させた方が構成が単純で責務も明確になる。

## Risks and Mitigations
- Workers 制約により重い連合処理が同期リクエストに乗りやすい
  `waitUntil` と Queue 前提で application 層を設計する。
- D1 の SQL 制約でタイムライン集約が高コストになる
  fan-out 戦略を早い段階で評価する。
- delivery expansion と target queue 挿入の間に完全な transaction がない
  `activity_id + target_inbox` の unique index と `INSERT OR IGNORE` で重複配送を抑える。
- GoToSocial を参考にしすぎて Go 向け設計を持ち込む
  責務だけ借りて、実装詳細は Workers の実行モデルに合わせる。

## Open Questions
- タイムラインの fan-out は push 方式にするか pull 方式にするか。
- 非同期ジョブを Queue 中心にするか `waitUntil` 中心にするか。
- NodeInfo, WebPush, custom emoji をどの段階で入れるか。

## Appendix
- 参照: GoToSocial repository
  https://codeberg.org/superseriousbusiness/gotosocial
- 参照: Cloudflare Workers Rust support
  https://developers.cloudflare.com/workers/languages/rust/
- ローカル参照: RustResort repository
  `../rustresort`
