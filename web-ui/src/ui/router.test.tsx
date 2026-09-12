/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { AppRoute } from "@/domain/navigation/route";
import { useAppParams } from "@/ui/hooks/useAppParams";
import { useAppSearch } from "@/ui/hooks/useAppSearch";
import { AppLink } from "@/ui/lib/app-link";
import { createAppRouter } from "@/ui/router";

const StatusProbe = () => {
  const { statusId } = useAppParams();
  return <div data-testid="status">{statusId}</div>;
};

const SearchProbe = () => {
  const search = useAppSearch();
  return <div data-testid="search">{`${search.q}:${search.type}`}</div>;
};

const HomeLinkProbe = () => (
  <AppLink to={AppRoute.status("s1")}>thread</AppLink>
);

describe("TanStack spike routes", () => {
  afterEach(() => {
    cleanup();
  });
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

  it("maps function className onto TanStack activeProps", async () => {
    const Probe = () => (
      <AppLink
        to={AppRoute.home()}
        end
        className={({ isActive }) => (isActive ? "is-active" : "idle")}
      >
        home
      </AppLink>
    );
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: [`${AppRoute.basename}/`] }),
      basepath: AppRoute.basename,
      RootComponent: Probe,
    });
    await router.load();
    render(<RouterProvider router={router} />);
    await waitFor(() => {
      expect(screen.getByRole("link", { name: "home" }).className).toBe("is-active");
    });
  });

  it("maps home to /", () => {
    expect(AppRoute.toPath(AppRoute.home())).toBe("/");
    expect(AppRoute.toLink(AppRoute.home())).toEqual({ to: AppRoute.path.home });
  });
});
