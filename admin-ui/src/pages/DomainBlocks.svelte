<script lang="ts">
  import {
    createDomainBlock,
    deleteDomainBlock,
    fetchDomainBlocks,
    type AdminDomainBlock,
    type DomainBlockSeverity,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";
  import Time from "../lib/Time.svelte";

  let blocks: AdminDomainBlock[] = [];
  let domain = "";
  let severity: DomainBlockSeverity = "suspend";
  let rejectMedia = false;
  let rejectReports = false;
  let obfuscate = false;
  let publicComment = "";

  const severityLabels: Record<DomainBlockSeverity, string> = {
    suspend: "停止",
    silence: "サイレンス",
    noop: "なし",
  };
  let loading = true;
  let error = "";
  let actionError = "";
  let notice = "";
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
      actionError = "ドメインを入力してください。";
      return;
    }
    saving = true;
    actionError = "";
    notice = "";
    const target = domain.trim();
    try {
      await createDomainBlock({
        domain: target,
        severity,
        reject_media: rejectMedia,
        reject_reports: rejectReports,
        obfuscate,
        public_comment: publicComment.trim(),
      });
      domain = "";
      publicComment = "";
      notice = `${target} をブロックしました。`;
      await loadBlocks();
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to create domain block";
    } finally {
      saving = false;
    }
  }

  async function removeBlock(block: AdminDomainBlock) {
    const ok = await confirmAction(`${block.domain} のブロックを解除しますか？解除すると連合配信が再開されます。`, {
      title: "ドメインブロックの解除",
      confirmLabel: "解除",
    });
    if (!ok) {
      return;
    }
    actionError = "";
    notice = "";
    try {
      await deleteDomainBlock(block.domain);
      blocks = blocks.filter((entry) => entry.id !== block.id);
      notice = `${block.domain} のブロックを解除しました。`;
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to delete domain block";
    }
  }

  loadBlocks();
</script>

<section class="panel">
  <h2>ドメインブロック</h2>
  <p class="muted">
    停止は連合を拒否します。サイレンスは公開タイムラインから投稿を隠し、フォローを承認制にします。既存のドメインを追加すると設定を上書きします。
  </p>

  <form class="inline-form" on:submit|preventDefault={submitBlock}>
    <label class="field">
      ブロックするドメイン
      <input
        bind:value={domain}
        placeholder="example.com"
        inputmode="url"
        autocapitalize="off"
        autocomplete="off"
        spellcheck="false"
      />
    </label>
    <label class="field">
      重大度
      <select bind:value={severity}>
        <option value="suspend">{severityLabels.suspend}</option>
        <option value="silence">{severityLabels.silence}</option>
        <option value="noop">{severityLabels.noop}</option>
      </select>
    </label>
    <label class="field">
      公開コメント
      <input bind:value={publicComment} autocomplete="off" />
    </label>
    <label><input type="checkbox" bind:checked={rejectMedia} /> メディアを拒否</label>
    <label><input type="checkbox" bind:checked={rejectReports} /> 通報を拒否</label>
    <label><input type="checkbox" bind:checked={obfuscate} /> 公開一覧でドメインを伏せる</label>
    <button class="btn btn-primary" type="submit" disabled={saving}>
      {saving ? "追加中…" : "ブロック"}
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
  {:else if blocks.length === 0}
    <div class="empty">ブロック中のドメインはありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>ドメイン</th>
            <th>重大度</th>
            <th>制限</th>
            <th>公開コメント</th>
            <th>追加日時</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each blocks as block (block.id)}
            <tr>
              <td class="mono">{block.domain}</td>
              <td>{severityLabels[block.severity] ?? block.severity}</td>
              <td>
                {[
                  block.reject_media ? "メディア拒否" : "",
                  block.reject_reports ? "通報拒否" : "",
                  block.obfuscate ? "伏せ字" : "",
                ]
                  .filter(Boolean)
                  .join("、") || "なし"}
              </td>
              <td>{block.public_comment ?? ""}</td>
              <td><Time value={block.created_at} /></td>
              <td>
                <button type="button" class="btn btn-danger" on:click={() => removeBlock(block)}>解除</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
