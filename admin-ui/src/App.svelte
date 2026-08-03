<script lang="ts">
  import Dashboard from "./pages/Dashboard.svelte";
  import Deliveries from "./pages/Deliveries.svelte";
  import DomainBlocks from "./pages/DomainBlocks.svelte";
  import Emojis from "./pages/Emojis.svelte";
  import Relays from "./pages/Relays.svelte";
  import Reports from "./pages/Reports.svelte";
  import System from "./pages/System.svelte";
  import { fetchSession, type AdminSession } from "./lib/api";

  type Page =
    | "dashboard"
    | "reports"
    | "emojis"
    | "deliveries"
    | "relays"
    | "domain-blocks"
    | "system";

  let page: Page = "dashboard";
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

  function selectPage(next: Page) {
    page = next;
  }

  loadSession();
</script>

{#if loading}
  <div class="loading">読み込み中…</div>
{:else if error}
  <div class="content">
    <div class="panel">
      <h2>管理画面</h2>
      <p class="error">{error}</p>
      <p class="muted">管理者としてログインしているか確認してください。</p>
    </div>
  </div>
{:else if session}
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">cfwdon Admin</div>
      <div class="subtitle">{session.instance_name}</div>
      <nav>
        <a
          class="nav-link"
          class:active={page === "dashboard"}
          href="/admin"
          on:click|preventDefault={() => selectPage("dashboard")}
        >
          ダッシュボード
        </a>
        <a
          class="nav-link"
          class:active={page === "reports"}
          href="/admin/reports"
          on:click|preventDefault={() => selectPage("reports")}
        >
          レポート
        </a>
        <a
          class="nav-link"
          class:active={page === "emojis"}
          href="/admin/emojis"
          on:click|preventDefault={() => selectPage("emojis")}
        >
          カスタム絵文字
        </a>
        <a
          class="nav-link"
          class:active={page === "deliveries"}
          href="/admin/deliveries"
          on:click|preventDefault={() => selectPage("deliveries")}
        >
          配信キュー
        </a>
        <a
          class="nav-link"
          class:active={page === "relays"}
          href="/admin/relays"
          on:click|preventDefault={() => selectPage("relays")}
        >
          リレー
        </a>
        <a
          class="nav-link"
          class:active={page === "domain-blocks"}
          href="/admin/domain-blocks"
          on:click|preventDefault={() => selectPage("domain-blocks")}
        >
          ドメインブロック
        </a>
        <a
          class="nav-link"
          class:active={page === "system"}
          href="/admin/system"
          on:click|preventDefault={() => selectPage("system")}
        >
          システム
        </a>
      </nav>
      <p class="muted" style="margin-top: 1.5rem; font-size: 0.85rem;">
        {session.username} ({session.email})
      </p>
    </aside>
    <main class="content">
      {#if page === "dashboard"}
        <Dashboard {session} />
      {:else if page === "reports"}
        <Reports />
      {:else if page === "emojis"}
        <Emojis />
      {:else if page === "relays"}
        <Relays />
      {:else if page === "domain-blocks"}
        <DomainBlocks />
      {:else if page === "system"}
        <System />
      {:else}
        <Deliveries />
      {/if}
    </main>
  </div>
{/if}
