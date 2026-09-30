<script lang="ts">
  import {
    fetchDashboard,
    type AdminDashboard,
    type AdminSession,
  } from "../lib/api";
  import { hrefFor, navigate, type Page } from "../lib/nav";

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

  type Card = { label: string; value: number; page: Page | null; warn: boolean };

  // Highlight counts that need an admin's attention.
  const cards = (value: AdminDashboard): Card[] => [
    { label: "未対応レポート", value: value.pending_reports, page: "reports", warn: value.pending_reports > 0 },
    { label: "配信失敗", value: value.failed_deliveries, page: "deliveries", warn: value.failed_deliveries > 0 },
    { label: "配信キュー", value: value.queued_deliveries, page: "deliveries", warn: false },
    { label: "バックグラウンドジョブ", value: value.pending_background_jobs, page: "system", warn: false },
    { label: "詰まった inbox", value: value.stuck_inbox_activities, page: "system", warn: value.stuck_inbox_activities > 0 },
    { label: "直近7日の新規登録", value: value.recent_signups, page: null, warn: false },
  ];

  load();
</script>

<section class="panel">
  <h2>ダッシュボード</h2>
  <p class="muted">{session.instance_name} の運用サマリー</p>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
    <button type="button" class="btn" on:click={load}>再試行</button>
  {:else if stats}
    <div class="stats">
      {#each cards(stats) as card (card.label)}
        {#if card.page}
          {@const target = card.page}
          <a
            class="stat is-link"
            class:warn={card.warn}
            href={hrefFor(target)}
            on:click={(event) => navigate(event, target)}
          >
            <span class="label">{card.label}</span>
            <span class="value">{card.value.toLocaleString("ja-JP")}</span>
          </a>
        {:else}
          <div class="stat" class:warn={card.warn}>
            <span class="label">{card.label}</span>
            <span class="value">{card.value.toLocaleString("ja-JP")}</span>
          </div>
        {/if}
      {/each}
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
    border-radius: var(--radius-lg);
    padding: 0.85rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    color: var(--text);
  }

  .stat.is-link:hover {
    background: var(--panel-hover);
    text-decoration: none;
  }

  .stat.warn {
    border-color: var(--warn-border);
    background: var(--warn-soft);
  }

  .stat.warn .value {
    color: var(--warn);
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
