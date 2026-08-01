<script lang="ts">
  import {
    fetchDeliveries,
    retryDelivery,
    type AdminDelivery,
  } from "../lib/api";

  let deliveries: AdminDelivery[] = [];
  let loading = true;
  let error = "";
  let stateFilter = "";
  let retryingKey = "";

  async function loadDeliveries() {
    loading = true;
    error = "";
    try {
      deliveries = await fetchDeliveries(stateFilter || undefined);
    } catch (err) {
      deliveries = [];
      error = err instanceof Error ? err.message : "failed to load deliveries";
    } finally {
      loading = false;
    }
  }

  async function retry(delivery: AdminDelivery) {
    const key = `${delivery.source}:${delivery.id}`;
    retryingKey = key;
    error = "";
    try {
      await retryDelivery(delivery.id, delivery.source);
      deliveries = deliveries.map((entry) =>
        entry.id === delivery.id && entry.source === delivery.source
          ? { ...entry, state: "queued" }
          : entry,
      );
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to retry delivery";
    } finally {
      retryingKey = "";
    }
  }

  function setStateFilter(next: string) {
    stateFilter = next;
    loadDeliveries();
  }

  loadDeliveries();
</script>

<section class="panel">
  <div class="toolbar">
    <h2>配信キュー</h2>
    <div class="filters">
      <button
        class="filter-btn"
        class:active={stateFilter === ""}
        on:click={() => setStateFilter("")}
      >
        要対応
      </button>
      <button
        class="filter-btn"
        class:active={stateFilter === "failed"}
        on:click={() => setStateFilter("failed")}
      >
        失敗
      </button>
      <button
        class="filter-btn"
        class:active={stateFilter === "queued"}
        on:click={() => setStateFilter("queued")}
      >
        待機
      </button>
      <button
        class="filter-btn"
        class:active={stateFilter === "in_flight"}
        on:click={() => setStateFilter("in_flight")}
      >
        実行中
      </button>
    </div>
  </div>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if deliveries.length === 0}
    <div class="empty">該当する配信はありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>更新</th>
          <th>種別</th>
          <th>状態</th>
          <th>Activity</th>
          <th>宛先</th>
          <th>試行</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each deliveries as delivery}
          <tr>
            <td>{delivery.updated_at}</td>
            <td>
              <span class="badge">{delivery.source}</span>
            </td>
            <td>
              <span
                class="badge"
                class:warn={delivery.state === "failed"}
                class:ok={delivery.state === "delivered"}
              >
                {delivery.state}
              </span>
            </td>
            <td>{delivery.activity_type}</td>
            <td class="mono">{delivery.target_inbox ?? "—"}</td>
            <td>{delivery.attempt_count}</td>
            <td>
              {#if delivery.state === "failed"}
                <button
                  class="primary"
                  disabled={retryingKey === `${delivery.source}:${delivery.id}`}
                  on:click={() => retry(delivery)}
                >
                  {retryingKey === `${delivery.source}:${delivery.id}`
                    ? "再試行中…"
                    : "再試行"}
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
    flex-wrap: wrap;
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

  .badge.warn {
    background: rgba(255, 107, 107, 0.18);
  }

  .badge.ok {
    background: rgba(62, 207, 142, 0.18);
  }

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.82rem;
    word-break: break-all;
  }
</style>
