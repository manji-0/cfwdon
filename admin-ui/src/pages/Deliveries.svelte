<script lang="ts">
  import {
    fetchDeliveries,
    retryDelivery,
    type AdminDelivery,
  } from "../lib/api";
  import { deliveryState } from "../lib/format";
  import StatusBadge from "../lib/StatusBadge.svelte";
  import Time from "../lib/Time.svelte";
  import TruncationNote from "../lib/TruncationNote.svelte";

  const filters = [
    { value: "", label: "要対応" },
    { value: "failed", label: "失敗" },
    { value: "queued", label: "待機" },
    { value: "in_flight", label: "送信中" },
  ];
  const sourceLabels: Record<AdminDelivery["source"], string> = {
    outbound: "個別配信",
    outbox: "outbox",
  };

  let deliveries: AdminDelivery[] = [];
  let loading = true;
  let error = "";
  let actionError = "";
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
    actionError = "";
    try {
      await retryDelivery(delivery.id, delivery.source);
      deliveries = deliveries.map((entry) =>
        entry.id === delivery.id && entry.source === delivery.source
          ? { ...entry, state: "queued" }
          : entry,
      );
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to retry delivery";
    } finally {
      retryingKey = "";
    }
  }

  function setStateFilter(next: string) {
    stateFilter = next;
    loadDeliveries();
  }

  // Each source is capped separately by the API, so check the larger one.
  $: largestSource = Math.max(
    deliveries.filter((entry) => entry.source === "outbound").length,
    deliveries.filter((entry) => entry.source === "outbox").length,
  );

  loadDeliveries();
</script>

<section class="panel">
  <div class="toolbar">
    <h2>配信キュー</h2>
    <div class="filters" role="group" aria-label="配信の状態">
      {#each filters as option (option.value)}
        <button
          type="button"
          class="filter-btn"
          aria-pressed={stateFilter === option.value}
          on:click={() => setStateFilter(option.value)}
        >
          {option.label}
        </button>
      {/each}
    </div>
  </div>

  {#if actionError}
    <p class="notice error" role="alert">{actionError}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if deliveries.length === 0}
    <div class="empty">該当する配信はありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>更新</th>
            <th>種別</th>
            <th>状態</th>
            <th>Activity</th>
            <th>宛先</th>
            <th>試行</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each deliveries as delivery (`${delivery.source}:${delivery.id}`)}
            {@const key = `${delivery.source}:${delivery.id}`}
            <tr>
              <td><Time value={delivery.updated_at} /></td>
              <td><span class="badge neutral">{sourceLabels[delivery.source] ?? delivery.source}</span></td>
              <td><StatusBadge status={deliveryState(delivery.state)} /></td>
              <td>{delivery.activity_type}</td>
              <td class="mono">{delivery.target_inbox ?? "—"}</td>
              <td>{delivery.attempt_count}</td>
              <td>
                {#if delivery.state === "failed"}
                  <button
                    type="button"
                    class="btn btn-primary"
                    disabled={retryingKey === key}
                    on:click={() => retry(delivery)}
                  >
                    {retryingKey === key ? "再試行中…" : "再試行"}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <TruncationNote count={largestSource} />
  {/if}
</section>
