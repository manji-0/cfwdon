<script lang="ts">
  import {
    createIpBlock,
    deleteIpBlock,
    fetchIpBlocks,
    updateIpBlock,
    type AdminIpBlock,
    type IpBlockForm,
    type IpBlockSeverity,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";
  import Time from "../lib/Time.svelte";

  const severityLabels: Record<IpBlockSeverity, string> = {
    no_access: "アクセス拒否",
    sign_up_block: "新規登録を拒否",
    sign_up_requires_approval: "新規登録を承認制に",
  };
  const severities = Object.keys(severityLabels) as IpBlockSeverity[];

  const KEEP_EXPIRY = "keep";
  const expiryOptions: ReadonlyArray<{ label: string; value: string }> = [
    { label: "無期限", value: "" },
    { label: "1日", value: "86400" },
    { label: "1週間", value: "604800" },
    { label: "30日", value: "2592000" },
  ];

  let blocks: AdminIpBlock[] = [];
  let editingId: string | null = null;
  let ip = "";
  let severity: IpBlockSeverity = "no_access";
  let comment = "";
  let expiryChoice = "";
  let loading = true;
  let error = "";
  let actionError = "";
  let notice = "";
  let saving = false;

  async function loadBlocks() {
    loading = true;
    error = "";
    try {
      blocks = await fetchIpBlocks();
    } catch (err) {
      blocks = [];
      error = err instanceof Error ? err.message : "failed to load IP blocks";
    } finally {
      loading = false;
    }
  }

  function resetForm() {
    editingId = null;
    ip = "";
    severity = "no_access";
    comment = "";
    expiryChoice = "";
  }

  function startEdit(block: AdminIpBlock) {
    editingId = block.id;
    ip = block.ip;
    severity = block.severity;
    comment = block.comment;
    expiryChoice = KEEP_EXPIRY;
    actionError = "";
    notice = "";
  }

  async function submitBlock() {
    if (!ip.trim()) {
      actionError = "IPアドレスまたはCIDR範囲を入力してください。";
      return;
    }
    saving = true;
    actionError = "";
    notice = "";
    const target = ip.trim();
    const form: IpBlockForm = {
      ip: target,
      severity,
      comment: comment.trim(),
    };
    if (expiryChoice !== KEEP_EXPIRY) {
      form.expires_in = expiryChoice === "" ? null : Number(expiryChoice);
    }
    try {
      if (editingId) {
        await updateIpBlock(editingId, form);
        notice = `${target} のブロックを更新しました。`;
      } else {
        await createIpBlock(form);
        notice = `${target} をブロックしました。`;
      }
      resetForm();
      await loadBlocks();
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to save IP block";
    } finally {
      saving = false;
    }
  }

  async function removeBlock(block: AdminIpBlock) {
    const ok = await confirmAction(`${block.ip} のブロックを解除しますか？`, {
      title: "IPブロックの解除",
      confirmLabel: "解除",
    });
    if (!ok) {
      return;
    }
    actionError = "";
    notice = "";
    try {
      await deleteIpBlock(block.id);
      blocks = blocks.filter((entry) => entry.id !== block.id);
      if (editingId === block.id) {
        resetForm();
      }
      notice = `${block.ip} のブロックを解除しました。`;
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to delete IP block";
    }
  }

  loadBlocks();
</script>

<section class="panel">
  <h2>IPブロック</h2>
  <p class="muted">
    IPv4 または IPv6 の CIDR 範囲に重大度を設定します。アクセス拒否はその範囲からのすべてのリクエストを拒否します。
    編集では有効期限を「現在のまま」にすると期限を変えません。
  </p>

  <form class="inline-form" on:submit|preventDefault={submitBlock}>
    <label class="field">
      IP / CIDR
      <input
        bind:value={ip}
        placeholder="203.0.113.0/24"
        autocapitalize="off"
        autocomplete="off"
        spellcheck="false"
      />
    </label>
    <label class="field">
      重大度
      <select bind:value={severity}>
        {#each severities as value (value)}
          <option {value}>{severityLabels[value]}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      有効期限
      <select bind:value={expiryChoice}>
        {#if editingId}
          <option value={KEEP_EXPIRY}>現在のまま</option>
        {/if}
        {#each expiryOptions as option (option.label)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      コメント
      <input bind:value={comment} autocomplete="off" />
    </label>
    <button class="btn btn-primary" type="submit" disabled={saving}>
      {saving ? "保存中…" : editingId ? "更新" : "ブロック"}
    </button>
    {#if editingId}
      <button class="btn" type="button" on:click={resetForm}>キャンセル</button>
    {/if}
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
  {:else if blocks.length === 0}
    <div class="empty">ブロック中のIP範囲はありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>IP / CIDR</th>
            <th>重大度</th>
            <th>コメント</th>
            <th>期限</th>
            <th>追加日時</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each blocks as block (block.id)}
            <tr>
              <td class="mono">{block.ip}</td>
              <td>{severityLabels[block.severity] ?? block.severity}</td>
              <td>{block.comment}</td>
              <td>
                {#if block.expires_at}
                  <Time value={block.expires_at} />
                {:else}
                  無期限
                {/if}
              </td>
              <td><Time value={block.created_at} /></td>
              <td class="row-actions">
                <button type="button" class="btn" on:click={() => startEdit(block)}>編集</button>
                <button type="button" class="btn btn-danger" on:click={() => removeBlock(block)}>解除</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
