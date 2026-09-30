<script lang="ts">
  import {
    fetchReports,
    resolveReport,
    type AdminReport,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";
  import Time from "../lib/Time.svelte";

  const categoryLabels: Record<string, string> = {
    spam: "スパム",
    legal: "法令違反",
    violation: "ルール違反",
    other: "その他",
  };

  let reports: AdminReport[] = [];
  let loading = true;
  let error = "";
  let actionError = "";
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

  async function markResolved(report: AdminReport) {
    const reportId = report.id;
    const ok = await confirmAction(`@${report.target_account.acct} へのレポートを対応済みにしますか？未対応一覧から外れます。`, {
      title: "レポートの対応",
      confirmLabel: "対応済みにする",
      danger: false,
    });
    if (!ok) {
      return;
    }
    resolvingId = reportId;
    actionError = "";
    try {
      const updated = await resolveReport(reportId);
      reports = reports.map((report) =>
        report.id === reportId ? updated : report,
      );
      if (filter === "pending") {
        reports = reports.filter((report) => !report.action_taken);
      }
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to resolve report";
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
    <div class="filters" role="group" aria-label="表示するレポート">
      <button
        type="button"
        class="filter-btn"
        aria-pressed={filter === "pending"}
        on:click={() => setFilter("pending")}
      >
        未対応
      </button>
      <button
        type="button"
        class="filter-btn"
        aria-pressed={filter === "all"}
        on:click={() => setFilter("all")}
      >
        すべて
      </button>
    </div>
  </div>

  {#if actionError}
    <p class="notice error" role="alert">{actionError}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if reports.length === 0}
    <div class="empty">{filter === "pending" ? "未対応のレポートはありません。" : "レポートはありません。"}</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>日時</th>
            <th>対象</th>
            <th>カテゴリ</th>
            <th>コメント</th>
            <th>状態</th>
          </tr>
        </thead>
        <tbody>
          {#each reports as report (report.id)}
            <tr>
              <td><Time value={report.created_at} /></td>
              <td class="target">
                <a href={`/app/profile/${report.target_account.id}`} target="_blank" rel="noreferrer">
                  <strong>{report.target_account.display_name || report.target_account.username}</strong>
                </a>
                <div class="muted mono">@{report.target_account.acct}</div>
              </td>
              <td><span class="badge neutral">{categoryLabels[report.category] ?? report.category}</span></td>
              <td class="comment">
                {#if report.comment}
                  {report.comment}
                {:else}
                  <span class="muted">コメントなし</span>
                {/if}
                {#if report.status_ids.length > 0}
                  <div class="statuses">
                    <span class="muted">対象の投稿:</span>
                    {#each report.status_ids as statusId (statusId)}
                      <a class="mono" href={`/app/status/${statusId}`} target="_blank" rel="noreferrer">{statusId}</a>
                    {/each}
                  </div>
                {/if}
              </td>
              <td>
                <div class="state">
                  {#if report.action_taken}
                    <span class="badge ok">対応済み</span>
                    {#if report.action_taken_at}
                      <div class="muted"><Time value={report.action_taken_at} /></div>
                    {/if}
                  {:else}
                    <span class="badge warn">未対応</span>
                    <button
                      type="button"
                      class="btn btn-primary"
                      disabled={resolvingId === report.id}
                      on:click={() => markResolved(report)}
                    >
                      {resolvingId === report.id ? "処理中…" : "対応済みにする"}
                    </button>
                  {/if}
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .comment {
    min-width: 14rem;
    overflow-wrap: anywhere;
  }

  .statuses {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.5rem;
    margin-top: 0.35rem;
    font-size: 0.85rem;
  }

  .target {
    min-width: 11rem;
    overflow-wrap: anywhere;
  }

  .state {
    display: grid;
    justify-items: start;
    gap: 0.4rem;
  }
</style>
