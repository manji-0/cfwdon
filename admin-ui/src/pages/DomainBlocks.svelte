<script lang="ts">
  import {
    createDomainBlock,
    deleteDomainBlock,
    fetchDomainBlocks,
    type AdminDomainBlock,
  } from "../lib/api";

  let blocks: AdminDomainBlock[] = [];
  let domain = "";
  let loading = true;
  let error = "";
  let saving = false;

  async function loadBlocks() {
    loading = true;
    error = "";
    try {
      blocks = await fetchDomainBlocks();
    } catch (err) {
      blocks = [];
      error = err instanceof Error ? err.message : "failed to load domain blocks";
    } finally {
      loading = false;
    }
  }

  async function submitBlock() {
    if (!domain.trim()) {
      error = "ドメインを入力してください。";
      return;
    }
    saving = true;
    error = "";
    try {
      await createDomainBlock(domain.trim());
      domain = "";
      await loadBlocks();
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to create domain block";
    } finally {
      saving = false;
    }
  }

  async function removeBlock(block: AdminDomainBlock) {
    error = "";
    try {
      await deleteDomainBlock(block.domain);
      blocks = blocks.filter((entry) => entry.id !== block.id);
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to delete domain block";
    }
  }

  loadBlocks();
</script>

<section class="panel">
  <h2>ドメインブロック</h2>
  <p class="muted">インスタンス全体で連合配信を拒否するドメインです。</p>

  <form class="form" on:submit|preventDefault={submitBlock}>
    <input bind:value={domain} placeholder="example.com" />
    <button class="primary" type="submit" disabled={saving}>
      {saving ? "追加中…" : "追加"}
    </button>
  </form>

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="error">{error}</p>
  {:else if blocks.length === 0}
    <div class="empty">ブロック中のドメインはありません。</div>
  {:else}
    <table>
      <thead>
        <tr>
          <th>ドメイン</th>
          <th>追加日時</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each blocks as block}
          <tr>
            <td class="mono">{block.domain}</td>
            <td>{block.created_at}</td>
            <td>
              <button class="danger" on:click={() => removeBlock(block)}>解除</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .form {
    display: flex;
    gap: 0.5rem;
    margin: 1rem 0;
  }

  input {
    flex: 1;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    border-radius: 0.5rem;
    padding: 0.55rem 0.75rem;
  }

  .primary,
  .danger {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    border-radius: 0.5rem;
    padding: 0.45rem 0.75rem;
  }

  .primary {
    background: rgba(79, 140, 255, 0.18);
    border-color: rgba(79, 140, 255, 0.45);
  }

  .danger {
    background: rgba(255, 107, 107, 0.12);
    border-color: rgba(255, 107, 107, 0.35);
  }

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
</style>
