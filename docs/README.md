# cfwdon Documentation

このディレクトリは、`cfwdon` の設計、開発、運用、Mastodon API 互換性を追うためのドキュメントをまとめる。

## Start Here

- [Development Workflow](development.md)
  ローカル開発環境、検証コマンド、CI と互換性ドキュメント更新手順。
- [Configuration Reference](configuration.md)
  Cloudflare Worker bindings、`wrangler.toml` の vars、secret に置くべき値、実行時設定。
- [Cloudflare Deploy Checklist](cloudflare-deploy.md)
  D1 / R2 / Worker を Cloudflare に配備するためのチェックリスト。
- [cfwdon Architecture](design-doc-cfwdon-architecture.md)
  Workers + D1 + R2 上で Mastodon 互換サーバーを構成する設計方針。
- [Mastodon API Compatibility](mastodon-api-compat/README.md)
  upstream Mastodon routes と `cfwdon` route 実装の対応表。

## Planning Documents

- [Full TODO](full-todo.md)
  既に完了した機能と残タスクの長期トラッカー。
- [Initial Roadmap](initial-roadmap.md)
  初期実装の進行記録と compatibility slice のメモ。

## Generated Documents

`docs/mastodon-api-compat/` 配下の次のファイルは `scripts/generate_mastodon_api_compat.py` で生成する。

- `mastodon-api-compat/README.md`
- `mastodon-api-compat/inventory.md`
- `mastodon-api-compat/todo-unimplemented.md`
- `mastodon-api-compat/todo-compat.md`

再生成後は差分を確認し、route 追加や互換性メモの変更が意図通りか確認する。

