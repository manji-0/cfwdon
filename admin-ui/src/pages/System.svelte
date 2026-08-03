<script lang="ts">
  import {
    fetchBackgroundJobs,
    fetchInboxActivities,
    reclaimInboxActivities,
    retryBackgroundJob,
    type AdminBackgroundJob,
    type AdminInboxActivity,
  } from "../lib/api";

  let jobs: AdminBackgroundJob[] = [];
  let inbox: AdminInboxActivity[] = [];
  let loading = true;
  let error = "";
  let jobFilter = "";
  let inboxPendingOnly = false;
  let retryingJobId = "";
  let reclaiming = false;

  const completionLabels: Record<AdminInboxActivity["completion_state"], string> = {
    completed: "完了",
    effect_applied: "副作用済み",
    in_flight: "処理中",
    stuck: "要確認",
  };

  async function loadAll() {
    loading = true;
    error = "";
    try {
      [jobs, inbox] = await Promise.all([
        fetchBackgroundJobs(jobFilter || undefined),
        fetchInboxActivities(inboxPendingOnly),
      ]);
    } catch (err) {
      jobs = [];
      inbox = [];
      error = err instanceof Error ? err.message : "failed to load system data";
    } finally {
      loading = false;
    }
  }

  async function retry(job: AdminBackgroundJob) {
    retryingJobId = job.id;
    error = "";
    try {
      await retryBackgroundJob(job.id);
      jobs = jobs.map((entry) =>
        entry.id === job.id ? { ...entry, status: "pending" } : entry,
      );
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to retry job";
    } finally {
      retryingJobId = "";
    }
  }

  async function reclaim() {
    reclaiming = true;
    error = "";
    try {
      const result = await reclaimInboxActivities();
      await loadAll();
      if (result.marked_processed === 0 && result.released === 0) {
        error = "回収対象の inbox はありませんでした。";
      }
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to reclaim inbox";
    } finally {
      reclaiming = false;
    }
  }

  function setJobFilter(next: string) {
    jobFilter = next;
    loadAll();
  }

  function toggleInboxPending() {
    inboxPendingOnly = !inboxPendingOnly;
    loadAll();
  }

  loadAll();
</script>

<section class="panel">
  <h2>バックグラウンドジョブ</h2>
  <div class="filters">
    <button class="filter-btn" class:active={jobFilter === ""} on:click={() => setJobFilter("")}>
      要対応
    </button>
    <button
      class="filter-btn"
      class:active={jobFilter === "failed"}
      on:click={() => setJobFilter("failed")}
    >
      失敗
    </button>
    <button
      class="filter-btn"
      class:active={jobFilter === "pending"}
      on:click={() => setJobFilter("pending")}
    >
      待機
    </button>
  </div>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if jobs.length === 0}
    <div class="empty">該当するジョブはありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>更新</th>
          <th>種別</th>
          <th>状態</th>
          <th>試行</th>
          <th>エラー</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each jobs as job}
          <tr>
            <td>{job.updated_at}</td>
            <td><span class="badge">{job.job_type}</span></td>
            <td>{job.status}</td>
            <td>{job.attempts}</td>
            <td class="mono">{job.last_error ?? "—"}</td>
            <td>
              {#if job.status === "failed"}
                <button
                  class="primary"
                  disabled={retryingJobId === job.id}
                  on:click={() => retry(job)}
                >
                  {retryingJobId === job.id ? "再試行中…" : "再試行"}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<section class="panel" style="margin-top: 1rem;">
  <div class="toolbar">
    <h2>受信 inbox</h2>
    <button class="filter-btn" class:active={inboxPendingOnly} on:click={toggleInboxPending}>
      未処理のみ
    </button>
    <button class="primary" disabled={reclaiming} on:click={reclaim}>
      {reclaiming ? "回収中…" : "stale を回収"}
    </button>
  </div>
  <p class="muted">
    「副作用済み」は投稿などは取り込み済みで dedup 行だけ残っている状態です。
  </p>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if inbox.length === 0}
    <div class="empty">受信 Activity はありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>受信</th>
          <th>Actor</th>
          <th>Activity</th>
          <th>種別</th>
          <th>状態</th>
        </tr>
      </thead>
      <tbody>
        {#each inbox as activity}
          <tr class:warn={activity.completion_state === "stuck"}>
            <td>{activity.created_at}</td>
            <td class="mono">{activity.actor_uri}</td>
            <td class="mono">{activity.activity_id}</td>
            <td>{activity.activity_type}</td>
            <td>{completionLabels[activity.completion_state]}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .toolbar,
  .filters {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-bottom: 0.75rem;
  }

  .toolbar h2 {
    margin: 0;
    flex: 1;
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

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.82rem;
    word-break: break-all;
  }

  tr.warn td {
    background: rgba(255, 180, 80, 0.08);
  }
</style>
