/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ListsPage } from "@/ui/pages/ListsPage";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { emptyResponse, stubFetch } from "@/ui/test/stub-fetch";
import { accountListApi } from "@/ui/test/mastodon-fixtures";

const listRoutes = {
  "GET /api/v1/lists": [accountListApi],
  "GET /api/v1/timelines/list/list-1": [],
  "GET /api/v1/lists/list-1/accounts": [],
  "GET /api/v1/announcements": [],
};

describe("ListsPage", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("asks before deleting a list and keeps it on cancel", async () => {
    const user = userEvent.setup();
    const { recorded, restore } = stubFetch(listRoutes);
    try {
      renderPage(<ListsPage />, { path: "/lists" });
      await screen.findByRole("button", { name: "Friends" });
      await user.click(screen.getByRole("button", { name: "削除" }));
      const dialog = screen.getByRole("dialog");
      expect(dialog.textContent).toContain("このリストを削除しますか？");
      await user.click(within(dialog).getByRole("button", { name: "キャンセル" }));
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(screen.getByRole("button", { name: "Friends" })).toBeTruthy();
      expect(recorded.some((item) => item.method === "DELETE")).toBe(false);
    } finally {
      restore();
    }
  });

  it("deletes the list after confirming", async () => {
    const user = userEvent.setup();
    const { recorded, restore } = stubFetch({
      ...listRoutes,
      "DELETE /api/v1/lists/list-1": () => emptyResponse(),
    });
    try {
      renderPage(<ListsPage />, { path: "/lists" });
      await screen.findByRole("button", { name: "Friends" });
      await user.click(screen.getByRole("button", { name: "削除" }));
      await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "削除" }));
      await waitFor(() => {
        expect(screen.queryByRole("button", { name: "Friends" })).toBeNull();
      });
      expect(recorded.some((item) => item.method === "DELETE" && item.path === "/api/v1/lists/list-1")).toBe(
        true,
      );
      expect(screen.getByText("リストはまだありません。")).toBeTruthy();
    } finally {
      restore();
    }
  });
});
