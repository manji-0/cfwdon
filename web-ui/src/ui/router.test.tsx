/** @vitest-environment happy-dom */
import { describe, expect, it } from "vitest";
import { RouterProvider, createMemoryHistory, useSearch } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import { AppRoute } from "@/domain/navigation/route";
import { useAppParams } from "@/ui/hooks/useAppParams";
import { AppLink } from "@/ui/lib/app-link";
import { createAppRouter } from "@/ui/router";

const StatusProbe = () => {
  const { statusId } = useAppParams();
  return <div data-testid="status">{statusId}</div>;
};

const SearchProbe = () => {
  const search = useSearch({ strict: false }) as Readonly<{ q?: string; type?: string }>;
  return <div data-testid="search">{`${search.q ?? ""}:${search.type ?? ""}`}</div>;
};

const HomeLinkProbe = () => (
  <AppLink to={AppRoute.status("s1")}>thread</AppLink>
);

describe("TanStack spike routes", () => {
  it("reads typed status params under the /app basepath", async () => {
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: [`${AppRoute.basename}/status/s1`] }),
      basepath: AppRoute.basename,
      RootComponent: StatusProbe,
    });
    await router.load();
    render(<RouterProvider router={router} />);
    await waitFor(() => {
      expect(screen.getByTestId("status").textContent).toBe("s1");
    });
  });

  it("keeps search query state on /search", async () => {
    const href = AppRoute.absoluteHref(AppRoute.search("hello", "accounts"));
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: [href] }),
      basepath: AppRoute.basename,
      RootComponent: SearchProbe,
    });
    await router.load();
    render(<RouterProvider router={router} />);
    await waitFor(() => {
      expect(screen.getByTestId("search").textContent).toBe("hello:accounts");
    });
  });

  it("renders AppLink hrefs under /app", async () => {
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: [`${AppRoute.basename}/`] }),
      basepath: AppRoute.basename,
      RootComponent: HomeLinkProbe,
    });
    await router.load();
    render(<RouterProvider router={router} />);
    await waitFor(() => {
      expect(screen.getByRole("link").getAttribute("href")).toBe("/app/status/s1");
    });
  });

  it("maps home to /", () => {
    expect(AppRoute.toPath(AppRoute.home())).toBe("/");
    expect(AppRoute.toLink(AppRoute.home())).toEqual({ to: AppRoute.path.home });
  });
});
