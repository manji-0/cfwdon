<script lang="ts">
  import {
    createEmoji,
    deleteEmoji,
    fetchEmojis,
    updateEmoji,
    type AdminEmoji,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";

  let emojis: AdminEmoji[] = [];
  let loading = true;
  let error = "";
  let actionError = "";
  let notice = "";
  let form: HTMLFormElement;
  let shortcode = "";
  let category = "";
  let imageFile: File | null = null;
  let uploading = false;

  async function loadEmojis() {
    loading = true;
    error = "";
    try {
      emojis = await fetchEmojis();
    } catch (err) {
      emojis = [];
      error = err instanceof Error ? err.message : "failed to load emojis";
    } finally {
      loading = false;
    }
  }

  function onImageChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    imageFile = input.files?.[0] ?? null;
  }

  async function submitEmoji() {
    if (!shortcode.trim() || !imageFile) {
      actionError = "shortcode と画像は必須です。";
      return;
    }
    uploading = true;
    actionError = "";
    notice = "";
    try {
      const formData = new FormData();
      formData.set("shortcode", shortcode.trim());
      formData.set("image", imageFile);
      if (category.trim()) {
        formData.set("category", category.trim());
      }
      const created = await createEmoji(formData);
      emojis = [...emojis, created].sort((left, right) =>
        left.shortcode.localeCompare(right.shortcode),
      );
      shortcode = "";
      category = "";
      imageFile = null;
      // Clears the native file input, which bind:value cannot reach.
      form.reset();
      notice = `:${created.shortcode}: を追加しました。`;
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to upload emoji";
    } finally {
      uploading = false;
    }
  }

  async function toggleVisibility(emoji: AdminEmoji) {
    actionError = "";
    notice = "";
    try {
      const updated = await updateEmoji(emoji.id, {
        visible_in_picker: !emoji.visible_in_picker,
      });
      emojis = emojis.map((entry) => (entry.id === emoji.id ? updated : entry));
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to update emoji";
    }
  }

  async function removeEmoji(emoji: AdminEmoji) {
    const ok = await confirmAction(`:${emoji.shortcode}: を削除しますか？この操作は取り消せません。`, {
      title: "カスタム絵文字の削除",
      confirmLabel: "削除",
    });
    if (!ok) {
      return;
    }
    actionError = "";
    notice = "";
    try {
      await deleteEmoji(emoji.id);
      emojis = emojis.filter((entry) => entry.id !== emoji.id);
    } catch (err) {
      actionError = err instanceof Error ? err.message : "failed to delete emoji";
    }
  }

  loadEmojis();
</script>

<section class="panel">
  <h2>カスタム絵文字</h2>

  <form class="upload-form" bind:this={form} on:submit|preventDefault={submitEmoji}>
    <label class="field">
      shortcode
      <input
        bind:value={shortcode}
        placeholder="example"
        pattern="[A-Za-z0-9_]+"
        title="英数字とアンダースコアのみ"
        autocomplete="off"
        required
      />
    </label>
    <label class="field">
      カテゴリ（任意）
      <input bind:value={category} />
    </label>
    <label class="field">
      画像
      <input type="file" accept="image/*" on:change={onImageChange} required />
    </label>
    <div>
      <button class="btn btn-primary" type="submit" disabled={uploading}>
        {uploading ? "アップロード中…" : "追加"}
      </button>
    </div>
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
  {:else if emojis.length === 0}
    <div class="empty">カスタム絵文字はまだありません。</div>
  {:else}
    <p class="muted count">{emojis.length} 件</p>
    <div class="emoji-grid">
      {#each emojis as emoji (emoji.id)}
        <article class="emoji-card" class:hidden-emoji={!emoji.visible_in_picker}>
          <img src={emoji.url} alt={`:${emoji.shortcode}:`} loading="lazy" />
          <div class="meta">
            <strong class="mono">:{emoji.shortcode}:</strong>
            <div class="muted">
              {emoji.category ?? "カテゴリなし"} ·
              {emoji.visible_in_picker ? "ピッカーに表示" : "ピッカーでは非表示"}
            </div>
          </div>
          <div class="row-actions">
            <button type="button" class="btn" on:click={() => toggleVisibility(emoji)}>
              {emoji.visible_in_picker ? "非表示にする" : "表示する"}
            </button>
            <button type="button" class="btn btn-danger" on:click={() => removeEmoji(emoji)}>
              削除
            </button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .upload-form {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
    align-items: end;
    gap: 0.75rem;
    margin-bottom: 1rem;
  }

  .upload-form input[type="file"] {
    padding: 0.35rem 0.5rem;
  }

  .count {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
  }

  .emoji-grid {
    display: grid;
  }

  .emoji-card {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) auto;
    gap: 0.75rem;
    align-items: center;
    border-top: 1px solid var(--border);
    padding: 0.75rem 0;
  }

  .emoji-card.hidden-emoji img {
    opacity: 0.5;
  }

  .emoji-card img {
    width: 48px;
    height: 48px;
    object-fit: contain;
  }

  .meta {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  @media (max-width: 560px) {
    .emoji-card {
      grid-template-columns: 48px minmax(0, 1fr);
    }

    .emoji-card .row-actions {
      grid-column: 1 / -1;
    }
  }
</style>
