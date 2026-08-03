<script lang="ts">
  import {
    fetchDashboard,
    type AdminDashboard,
    type AdminSession,
  } from "../lib/api";

  export let session: AdminSession;

  let stats: AdminDashboard | null = null;
  let error = "";
  let loading = true;

  async function load() {
    loading = true;
    error = "";
    try {
      stats = await fetchDashboard();
    } catch (err) {
      stats = null;
      error = err instanceof Error ? err.message : "failed to load dashboard";
    } finally {
      loading = false;
    }
  }

  load();
</script>

<section class="panel">
  <h2>ダッシュボード</h2>
  <p class="muted">{session.instance_name} の運用サマリー</p>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if stats}
    <div class="stats">
      <div class="stat">
        <span class="label">未対応レポート</span>
        <span class="value">{stats.pending_reports}</span>
      </div>
      <div class="stat">
        <span class="label">配信失敗</span>
        <span class="value">{stats.failed_deliveries}</span>
      </div>
      <div class="stat">
        <span class="label">配信キュー</span>
        <span class="value">{stats.queued_deliveries}</span>
      </div>
      <div class="stat">
        <span class="label">バックグラウンドジョブ</span>
        <span class="value">{stats.pending_background_jobs}</span>
      </div>
      <div class="stat" class:warn={stats.stuck_inbox_activities > 0}>
        <span class="label">詰まった inbox</span>
        <span class="value">{stats.stuck_inbox_activities}</span>
      </div>
      <div class="stat">
        <span class="label">直近7日の新規登録</span>
        <span class="value">{stats.recent_signups}</span>
      </div>
    </div>
  {/if}
</section>

<style>
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
    gap: 0.75rem;
    margin-top: 1rem;
  }

  .stat {
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    padding: 0.85rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .stat.warn {
    border-color: rgba(255, 180, 80, 0.55);
    background: rgba(255, 180, 80, 0.08);
  }

  .label {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .value {
    font-size: 1.5rem;
    font-weight: 600;
  }
</style>
