<script lang="ts">
  import {
    createRelay,
    deleteRelay,
    disableRelay,
    fetchRelays,
    type AdminRelay,
  } from "../lib/api";

  let relays: AdminRelay[] = [];
  let inboxUrl = "";
  let loading = true;
  let error = "";
  let saving = false;
  let actingId = "";

  async function loadRelays() {
    loading = true;
    error = "";
    try {
      relays = await fetchRelays();
    } catch (err) {
      relays = [];
      error = err instanceof Error ? err.message : "failed to load relays";
    } finally {
      loading = false;
    }
  }

  async function submitRelay() {
    if (!inboxUrl.trim()) {
      error = "リレーの inbox URL を入力してください。";
      return;
    }
    saving = true;
    error = "";
    try {
      await createRelay(inboxUrl.trim());
      inboxUrl = "";
      await loadRelays();
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to create relay";
    } finally {
      saving = false;
    }
  }

  async function disable(relay: AdminRelay) {
    actingId = relay.id;
    error = "";
    try {
      await disableRelay(relay.id);
      await loadRelays();
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to disable relay";
    } finally {
      actingId = "";
    }
  }

  async function remove(relay: AdminRelay) {
    actingId = relay.id;
    error = "";
    try {
      await deleteRelay(relay.id);
      relays = relays.filter((entry) => entry.id !== relay.id);
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to delete relay";
    } finally {
      actingId = "";
    }
  }

  function stateLabel(state: string): string {
    switch (state) {
      case "accepted":
        return "有効";
      case "pending":
        return "承認待ち";
      case "rejected":
        return "拒否";
      default:
        return "無効";
    }
  }

  loadRelays();
</script>

<section class="panel">
  <h2>連合リレー</h2>
  <p class="muted">
    Mastodon 互換リレーに購読し、公開投稿の送受信を行います。URL は
    <code>https://relay.example/inbox</code> 形式を指定してください。リレー由来の
    public remote 投稿は 7 日で自動削除されます（フォロー/フォロワー関係のあるアカウントは除く）。
  </p>

  <form class="form" on:submit|preventDefault={submitRelay}>
    <input bind:value={inboxUrl} placeholder="https://relay.example/inbox" />
    <button class="primary" type="submit" disabled={saving}>
      {saving ? "追加中…" : "追加して有効化"}
    </button>
  </form>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if relays.length === 0}
    <div class="empty">接続中のリレーはありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>inbox URL</th>
          <th>状態</th>
          <th>更新</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each relays as relay}
          <tr>
            <td><code>{relay.inbox_url}</code></td>
            <td>{stateLabel(relay.state)}</td>
            <td>{relay.updated_at}</td>
            <td class="actions">
              {#if relay.state === "accepted" || relay.state === "pending"}
                <button
                  type="button"
                  disabled={actingId === relay.id}
                  on:click={() => disable(relay)}
                >
                  無効化
                </button>
              {/if}
              <button
                type="button"
                class="danger"
                disabled={actingId === relay.id}
                on:click={() => remove(relay)}
              >
                削除
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  code {
    word-break: break-all;
  }
</style>
