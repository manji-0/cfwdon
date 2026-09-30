<script lang="ts">
  import {
    fetchBackgroundJobs,
    fetchInboxActivities,
    reclaimInboxActivities,
    retryBackgroundJob,
    type AdminBackgroundJob,
    type AdminInboxActivity,
  } from "../lib/api";
  import { jobStatus, type StatusLabel } from "../lib/format";
  import StatusBadge from "../lib/StatusBadge.svelte";
  import Time from "../lib/Time.svelte";
  import TruncationNote from "../lib/TruncationNote.svelte";

  const jobFilters = [
    { value: "", label: "要対応" },
    { value: "failed", label: "失敗" },
    { value: "pending", label: "待機" },
  ];

  let jobs: AdminBackgroundJob[] = [];
  let inbox: AdminInboxActivity[] = [];
  let loading = true;
  let error = "";
  let jobActionError = "";
  let inboxActionError = "";
  let inboxNotice = "";
  let jobFilter = "";
  let inboxPendingOnly = false;
  let retryingJobId = "";
  let reclaiming = false;

  const completionLabels: Record<AdminInboxActivity["completion_state"], StatusLabel> = {
    completed: { label: "完了", tone: "ok" },
    effect_applied: { label: "副作用済み", tone: "neutral" },
    in_flight: { label: "処理中", tone: "" },
    stuck: { label: "要確認", tone: "warn" },
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
    jobActionError = "";
    try {
      await retryBackgroundJob(job.id);
      jobs = jobs.map((entry) =>
        entry.id === job.id ? { ...entry, status: "pending" } : entry,
      );
    } catch (err) {
      jobActionError = err instanceof Error ? err.message : "failed to retry job";
    } finally {
      retryingJobId = "";
    }
  }

  async function reclaim() {
    reclaiming = true;
    inboxActionError = "";
    inboxNotice = "";
    try {
      const result = await reclaimInboxActivities();
      await loadAll();
      inboxNotice =
        result.marked_processed === 0 && result.released === 0
          ? "回収対象の inbox はありませんでした。"
          : `${result.marked_processed} 件を処理済みにし、${result.released} 件を再受信できるよう解放しました。`;
    } catch (err) {
      inboxActionError = err instanceof Error ? err.message : "failed to reclaim inbox";
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
  <div class="toolbar">
    <h2>バックグラウンドジョブ</h2>
    <div class="filters" role="group" aria-label="ジョブの状態">
      {#each jobFilters as option (option.value)}
        <button
          type="button"
          class="filter-btn"
          aria-pressed={jobFilter === option.value}
          on:click={() => setJobFilter(option.value)}
        >
          {option.label}
        </button>
      {/each}
    </div>
  </div>

  {#if jobActionError}
    <p class="notice error" role="alert">{jobActionError}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if jobs.length === 0}
    <div class="empty">該当するジョブはありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>更新</th>
            <th>種別</th>
            <th>状態</th>
            <th>試行</th>
            <th>エラー</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each jobs as job (job.id)}
            <tr>
              <td><Time value={job.updated_at} /></td>
              <td><span class="badge neutral mono">{job.job_type}</span></td>
              <td><StatusBadge status={jobStatus(job.status)} /></td>
              <td>{job.attempts}</td>
              <td class="mono error-cell">{job.last_error ?? "—"}</td>
              <td>
                {#if job.status === "failed"}
                  <button
                    type="button"
                    class="btn btn-primary"
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
    </div>
    <TruncationNote count={jobs.length} />
  {/if}
</section>

<section class="panel">
  <div class="toolbar">
    <h2>受信 inbox</h2>
    <div class="row-actions">
      <button
        type="button"
        class="filter-btn"
        aria-pressed={inboxPendingOnly}
        on:click={toggleInboxPending}
      >
        未処理のみ
      </button>
      <button type="button" class="btn btn-primary" disabled={reclaiming} on:click={reclaim}>
        {reclaiming ? "回収中…" : "stale を回収"}
      </button>
    </div>
  </div>
  <p class="muted">
    「副作用済み」は投稿などは取り込み済みで dedup 行だけ残っている状態です。「stale を回収」は処理が止まった行を片付けます。
  </p>

  {#if inboxActionError}
    <p class="notice error" role="alert">{inboxActionError}</p>
  {/if}
  {#if inboxNotice}
    <p class="notice ok" role="status">{inboxNotice}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if inbox.length === 0}
    <div class="empty">受信 Activity はありません。</div>
  {:else}
    <div class="table-wrap">
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
          {#each inbox as activity (`${activity.actor_uri} ${activity.activity_id}`)}
            <tr class:warn={activity.completion_state === "stuck"}>
              <td><Time value={activity.created_at} /></td>
              <td class="mono">{activity.actor_uri}</td>
              <td class="mono">{activity.activity_id}</td>
              <td>{activity.activity_type}</td>
              <td><StatusBadge status={completionLabels[activity.completion_state]} /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <TruncationNote count={inbox.length} />
  {/if}
</section>

<style>
  .error-cell {
    min-width: 16rem;
    max-width: 32rem;
  }
</style>
