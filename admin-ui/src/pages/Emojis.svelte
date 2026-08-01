<script lang="ts">
  import {
    createEmoji,
    deleteEmoji,
    fetchEmojis,
    updateEmoji,
    type AdminEmoji,
  } from "../lib/api";

  let emojis: AdminEmoji[] = [];
  let loading = true;
  let error = "";
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
      error = "shortcode と画像は必須です。";
      return;
    }
    uploading = true;
    error = "";
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
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to upload emoji";
    } finally {
      uploading = false;
    }
  }

  async function toggleVisibility(emoji: AdminEmoji) {
    error = "";
    try {
      const updated = await updateEmoji(emoji.id, {
        visible_in_picker: !emoji.visible_in_picker,
      });
      emojis = emojis.map((entry) => (entry.id === emoji.id ? updated : entry));
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to update emoji";
    }
  }

  async function removeEmoji(emoji: AdminEmoji) {
    if (!confirm(`:${emoji.shortcode}: を削除しますか？`)) {
      return;
    }
    error = "";
    try {
      await deleteEmoji(emoji.id);
      emojis = emojis.filter((entry) => entry.id !== emoji.id);
    } catch (err) {
      error = err instanceof Error ? err.message : "failed to delete emoji";
    }
  }

  loadEmojis();
</script>

<section class="panel">
  <h2>カスタム絵文字</h2>

  <form class="upload-form" on:submit|preventDefault={submitEmoji}>
    <label>
      shortcode
      <input bind:value={shortcode} placeholder="example" required />
    </label>
    <label>
      カテゴリ
      <input bind:value={category} placeholder="optional" />
    </label>
    <label>
      画像
      <input type="file" accept="image/*" on:change={onImageChange} required />
    </label>
    <button class="primary" type="submit" disabled={uploading}>
      {uploading ? "アップロード中…" : "追加"}
    </button>
  </form>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if emojis.length === 0}
    <div class="empty">カスタム絵文字はまだありません。</div>
  {:else}
    <div class="emoji-grid">
      {#each emojis as emoji}
        <article class="emoji-card">
          <img src={emoji.url} alt={emoji.shortcode} />
          <div>
            <strong>:{emoji.shortcode}:</strong>
            {#if emoji.category}
              <div class="muted">{emoji.category}</div>
            {/if}
            <div class="muted">
              {emoji.visible_in_picker ? "ピッカー表示" : "非表示"}
            </div>
          </div>
          <div class="actions">
            <button class="filter-btn" on:click={() => toggleVisibility(emoji)}>
              {emoji.visible_in_picker ? "非表示にする" : "表示する"}
            </button>
            <button class="danger" on:click={() => removeEmoji(emoji)}>
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
    gap: 0.75rem;
    margin-bottom: 1rem;
  }

  label {
    display: grid;
    gap: 0.35rem;
    font-size: 0.9rem;
  }

  input {
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--bg);
    color: var(--text);
    padding: 0.55rem 0.7rem;
  }

  .emoji-grid {
    display: grid;
    gap: 0.75rem;
  }

  .emoji-card {
    display: grid;
    grid-template-columns: 48px 1fr auto;
    gap: 0.75rem;
    align-items: center;
    border-top: 1px solid var(--border);
    padding-top: 0.75rem;
  }

  .emoji-card img {
    width: 48px;
    height: 48px;
    object-fit: contain;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .filter-btn,
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
    border-color: rgba(255, 107, 107, 0.45);
    color: var(--danger);
  }
</style>
