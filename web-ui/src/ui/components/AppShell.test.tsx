/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppShell } from "@/ui/components/AppShell";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { stubFetch } from "@/ui/test/stub-fetch";

describe("AppShell navigation", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("keeps five hubs and opens the compose sheet", async () => {
    const user = userEvent.setup();
    const { restore } = stubFetch({
      "GET /api/v1/announcements": [],
    });
    try {
      renderPage(
        <AppShell title="ホーム">
          <p>feed</p>
        </AppShell>,
      );
      expect(screen.getByRole("navigation", { name: "メイン" })).toBeTruthy();
      expect(screen.getAllByRole("link", { name: "ホーム" }).length).toBeGreaterThan(0);
      expect(screen.getAllByRole("link", { name: "受信" }).length).toBeGreaterThan(0);
      expect(screen.getAllByRole("link", { name: "検索" }).length).toBeGreaterThan(0);
      expect(screen.getAllByRole("link", { name: "自分" }).length).toBeGreaterThan(0);
      expect(screen.queryByRole("link", { name: "設定" })).toBeNull();
      expect(screen.queryByRole("link", { name: "探索" })).toBeNull();
      const composeButtons = screen.getAllByRole("button", { name: "投稿" });
      await user.click(composeButtons[0]!);
      expect(screen.getByRole("dialog", { name: "新規投稿" })).toBeTruthy();
    } finally {
      restore();
    }
  });
});
