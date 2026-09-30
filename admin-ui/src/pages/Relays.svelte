<script lang="ts">
  import {
    createRelay,
    deleteRelay,
    disableRelay,
    fetchRelays,
    type AdminRelay,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";
  import { relayState } from "../lib/format";
  import StatusBadge from "../lib/StatusBadge.svelte";
  import Time from "../lib/Time.svelte";

  let relays: AdminRelay[] = [];
  let inboxUrl = "";
  let loading = true;
  let error = "";
  let actionError = "";
  let notice = "";
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
      actionError = "リレーの inbox URL を入力してください。";
      return;
    }
    saving = true;
    actionError = "";
    notice = "";
    try {
      await createRelay(inboxUrl.trim());
      inboxUrl = "";
      notice = "リレーに購読リクエストを送りました。承認されると「有効」になります。";
      await loadRelays();
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to create relay";
    } finally {
      saving = false;
    }
  }

  async function disable(relay: AdminRelay) {
    const ok = await confirmAction(`${relay.inbox_url} の購読を停止しますか？`, {
      title: "リレーの無効化",
      confirmLabel: "無効化",
    });
    if (!ok) {
      return;
    }
    actingId = relay.id;
    actionError = "";
    try {
      await disableRelay(relay.id);
      await loadRelays();
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to disable relay";
    } finally {
      actingId = "";
    }
  }

  async function remove(relay: AdminRelay) {
    const ok = await confirmAction(`${relay.inbox_url} を削除しますか？この操作は取り消せません。`, {
      title: "リレーの削除",
      confirmLabel: "削除",
    });
    if (!ok) {
      return;
    }
    actingId = relay.id;
    actionError = "";
    try {
      await deleteRelay(relay.id);
      relays = relays.filter((entry) => entry.id !== relay.id);
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to delete relay";
    } finally {
      actingId = "";
    }
  }

  loadRelays();
</script>

<section class="panel">
  <h2>連合リレー</h2>
  <p class="muted">
    Mastodon 互換リレーに購読し、公開投稿の送受信を行います。リレー由来の public remote
    投稿は 7 日で自動削除されます（フォロー/フォロワー関係のあるアカウントは除く）。
  </p>

  <form class="inline-form" on:submit|preventDefault={submitRelay}>
    <label class="field">
      リレーの inbox URL
      <input
        type="url"
        bind:value={inboxUrl}
        placeholder="https://relay.example/inbox"
        autocomplete="off"
        spellcheck="false"
      />
    </label>
    <button class="btn btn-primary" type="submit" disabled={saving}>
      {saving ? "追加中…" : "追加して有効化"}
    </button>
  </form>

  {#if actionError}
    <p class="notice error" role="alert">{actionError}</p>
  {/if}
  {#if notice}
    <p class="notice ok" role="status">{notice}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if relays.length === 0}
    <div class="empty">接続中のリレーはありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>inbox URL</th>
            <th>状態</th>
            <th>更新</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each relays as relay (relay.id)}
            <tr>
              <td class="mono">{relay.inbox_url}</td>
              <td><StatusBadge status={relayState(relay.state)} /></td>
              <td><Time value={relay.updated_at} /></td>
              <td>
                <div class="row-actions">
                  {#if relay.state === "accepted" || relay.state === "pending"}
                    <button
                      type="button"
                      class="btn"
                      disabled={actingId === relay.id}
                      on:click={() => disable(relay)}
                    >
                      無効化
                    </button>
                  {/if}
                  <button
                    type="button"
                    class="btn btn-danger"
                    disabled={actingId === relay.id}
                    on:click={() => remove(relay)}
                  >
                    削除
                  </button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
