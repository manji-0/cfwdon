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

  const navItems: ReadonlyArray<{ page: Page; href: string; label: string }> = [
    { page: "dashboard", href: "/admin", label: "ダッシュボード" },
    { page: "reports", href: "/admin/reports", label: "レポート" },
    { page: "emojis", href: "/admin/emojis", label: "カスタム絵文字" },
    { page: "deliveries", href: "/admin/deliveries", label: "配信キュー" },
    { page: "relays", href: "/admin/relays", label: "リレー" },
    { page: "domain-blocks", href: "/admin/domain-blocks", label: "ドメインブロック" },
    { page: "system", href: "/admin/system", label: "システム" },
  ];

  function pageFromPath(pathname: string): Page {
    const normalized = pathname.replace(/\/+$/, "") || "/admin";
    return navItems.find((item) => item.href === normalized)?.page ?? "dashboard";
  }

  let page: Page = pageFromPath(window.location.pathname);
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

  function navigate(event: MouseEvent, item: (typeof navItems)[number]) {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    if (page !== item.page) {
      history.pushState(null, "", item.href);
      page = item.page;
    }
  }

  function onPopState() {
    page = pageFromPath(window.location.pathname);
  }

  loadSession();
</script>

<svelte:window on:popstate={onPopState} />

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
        {#each navItems as item (item.page)}
          <a
            class="nav-link"
            class:active={page === item.page}
            aria-current={page === item.page ? "page" : undefined}
            href={item.href}
            on:click={(event) => navigate(event, item)}
          >
            {item.label}
          </a>
        {/each}
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
