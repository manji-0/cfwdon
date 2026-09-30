import { writable } from "svelte/store";

export type Page =
  | "dashboard"
  | "reports"
  | "emojis"
  | "deliveries"
  | "relays"
  | "domain-blocks"
  | "system";

export type NavItem = Readonly<{ page: Page; href: string; label: string }>;

export const navItems: ReadonlyArray<NavItem> = [
  { page: "dashboard", href: "/admin", label: "ダッシュボード" },
  { page: "reports", href: "/admin/reports", label: "レポート" },
  { page: "emojis", href: "/admin/emojis", label: "カスタム絵文字" },
  { page: "deliveries", href: "/admin/deliveries", label: "配信キュー" },
  { page: "relays", href: "/admin/relays", label: "リレー" },
  { page: "domain-blocks", href: "/admin/domain-blocks", label: "ドメインブロック" },
  { page: "system", href: "/admin/system", label: "システム" },
];

export function pageFromPath(pathname: string): Page {
  const normalized = pathname.replace(/\/+$/, "") || "/admin";
  return navItems.find((item) => item.href === normalized)?.page ?? "dashboard";
}

export const hrefFor = (page: Page): string =>
  navItems.find((item) => item.page === page)?.href ?? "/admin";

export const labelFor = (page: Page): string =>
  navItems.find((item) => item.page === page)?.label ?? "管理画面";

/** The page shown in the content area; kept in sync with the URL. */
export const currentPage = writable<Page>(pageFromPath(window.location.pathname));

/**
 * Client-side navigation for `<a href>` links. Modified clicks fall through so
 * "open in new tab" keeps working (the worker serves the SPA for any /admin path).
 */
export function navigate(event: MouseEvent, page: Page): void {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return;
  }
  event.preventDefault();
  currentPage.update((current) => {
    if (current !== page) {
      history.pushState(null, "", hrefFor(page));
    }
    return page;
  });
}

export function syncFromLocation(): void {
  currentPage.set(pageFromPath(window.location.pathname));
}
