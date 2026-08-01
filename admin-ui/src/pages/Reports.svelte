<script lang="ts">
  import {
    fetchReports,
    resolveReport,
    type AdminReport,
  } from "../lib/api";

  let reports: AdminReport[] = [];
  let loading = true;
  let error = "";
  let filter: "all" | "pending" = "pending";
  let resolvingId = "";

  async function loadReports() {
    loading = true;
    error = "";
    try {
      reports = await fetchReports(filter);
    } catch (err) {
      reports = [];
      error = err instanceof Error ? err.message : "failed to load reports";
    } finally {
      loading = false;
    }
  }

  async function markResolved(reportId: string) {
    resolvingId = reportId;
    error = "";
    try {
      const updated = await resolveReport(reportId);
      reports = reports.map((report) =>
        report.id === reportId ? updated : report,
      );
      if (filter === "pending") {
        reports = reports.filter((report) => !report.action_taken);
      }
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to resolve report";
    } finally {
      resolvingId = "";
    }
  }

  function setFilter(next: "all" | "pending") {
    filter = next;
    loadReports();
  }

  loadReports();
</script>

<section class="panel">
  <div class="toolbar">
    <h2>レポート</h2>
    <div class="filters">
      <button
        class="filter-btn"
        class:active={filter === "pending"}
        on:click={() => setFilter("pending")}
      >
        未対応
      </button>
      <button
        class="filter-btn"
        class:active={filter === "all"}
        on:click={() => setFilter("all")}
      >
        すべて
      </button>
    </div>
  </div>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if reports.length === 0}
    <div class="empty">レポートはありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>日時</th>
          <th>対象</th>
          <th>カテゴリ</th>
          <th>コメント</th>
          <th>状態</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each reports as report}
          <tr>
            <td>{report.created_at}</td>
            <td>
              <strong>{report.target_account.display_name}</strong>
              <div class="muted">@{report.target_account.acct}</div>
            </td>
            <td><span class="badge">{report.category}</span></td>
            <td>
              {report.comment || "—"}
              {#if report.status_ids.length > 0}
                <div class="muted">
                  対象ステータス: {report.status_ids.join(", ")}
                </div>
              {/if}
            </td>
            <td>
              {#if report.action_taken}
                <span class="badge ok">対応済み</span>
                {#if report.action_taken_at}
                  <div class="muted">{report.action_taken_at}</div>
                {/if}
              {:else}
                <span class="badge warn">未対応</span>
              {/if}
            </td>
            <td>
              {#if !report.action_taken}
                <button
                  class="primary"
                  disabled={resolvingId === report.id}
                  on:click={() => markResolved(report.id)}
                >
                  {resolvingId === report.id ? "処理中…" : "対応済みにする"}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }

  .toolbar h2 {
    margin: 0;
  }

  .filters {
    display: flex;
    gap: 0.5rem;
  }

  .filter-btn,
  .primary {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    border-radius: 0.5rem;
    padding: 0.45rem 0.75rem;
  }

  .filter-btn.active,
  .primary {
    background: rgba(79, 140, 255, 0.18);
    border-color: rgba(79, 140, 255, 0.45);
  }

  .badge.ok {
    background: rgba(62, 207, 142, 0.18);
  }

  .badge.warn {
    background: rgba(255, 193, 7, 0.18);
  }
</style>
