<script lang="ts">
  import Dashboard from "./pages/Dashboard.svelte";
  import Deliveries from "./pages/Deliveries.svelte";
  import DomainBlocks from "./pages/DomainBlocks.svelte";
  import Emojis from "./pages/Emojis.svelte";
  import Relays from "./pages/Relays.svelte";
  import Reports from "./pages/Reports.svelte";
  import System from "./pages/System.svelte";
  import { fetchSession, type AdminSession } from "./lib/api";
  import ConfirmDialog from "./lib/ConfirmDialog.svelte";
  import { currentPage as page, labelFor, navigate, navItems, syncFromLocation } from "./lib/nav";

  let session: AdminSession | null = null;
  let error = "";
  let loading = true;

  async function loadSession() {
    loading = true;
    error = "";
    try {
      session = await fetchSession();
    } catch (err) {
      session = null;
      error = err instanceof Error ? err.message : "failed to load session";
    } finally {
      loading = false;
    }
  }

  $: document.title = `${labelFor($page)} - cfwdon Admin`;

  loadSession();
</script>

<svelte:window on:popstate={syncFromLocation} />

{#if loading}
  <div class="loading">読み込み中…</div>
{:else if error}
  <main class="content session-error">
    <div class="panel">
      <h2>管理画面を開けませんでした</h2>
      <p>管理者アカウントでログインしているか確認してください。セッションが切れている場合は再ログインで復帰できます。</p>
      <div class="row-actions">
        <button type="button" class="btn btn-primary" on:click={loadSession}>再試行</button>
        <a class="btn" href="/admin/relogin">再ログイン</a>
      </div>
      <details>
        <summary class="muted">エラーの詳細</summary>
        <pre class="mono">{error}</pre>
      </details>
    </div>
  </main>
{:else if session}
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">cfwdon Admin</div>
      <div class="subtitle">{session.instance_name}</div>
      <nav>
        {#each navItems as item (item.page)}
          <a
            class="nav-link"
            aria-current={$page === item.page ? "page" : undefined}
            href={item.href}
            on:click={(event) => navigate(event, item.page)}
          >
            {item.label}
          </a>
        {/each}
      </nav>
      <div class="account">
        <div>{session.username}</div>
        <div>{session.email}</div>
        <a href="/admin/logout">ログアウト</a>
      </div>
    </aside>
    <main class="content">
      {#if $page === "dashboard"}
        <Dashboard {session} />
      {:else if $page === "reports"}
        <Reports />
      {:else if $page === "emojis"}
        <Emojis />
      {:else if $page === "relays"}
        <Relays />
      {:else if $page === "domain-blocks"}
        <DomainBlocks />
      {:else if $page === "system"}
        <System />
      {:else}
        <Deliveries />
      {/if}
    </main>
  </div>
{/if}

<ConfirmDialog />

<style>
  .session-error {
    max-width: 40rem;
    margin: 3rem auto;
  }

  .session-error details {
    margin-top: 1rem;
  }

  .session-error pre {
    white-space: pre-wrap;
    margin: 0.5rem 0 0;
  }
</style>
