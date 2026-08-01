<script lang="ts">
  import type { AdminSession } from "../lib/api";

  export let session: AdminSession;
</script>

<section class="panel">
  <h2>ダッシュボード</h2>
  <p class="muted">
    {session.instance_name} の管理画面へようこそ。
  </p>
  <ul>
    <li>レポートの確認と対応</li>
    <li>カスタム絵文字の追加・削除</li>
    <li>失敗した ActivityPub 配信の再試行</li>
  </ul>
</section>

<style>
  ul {
    margin: 0.75rem 0 0;
    padding-left: 1.25rem;
    color: var(--muted);
  }
</style>
