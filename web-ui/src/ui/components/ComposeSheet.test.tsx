/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCompose } from "@/ui/context/ComposeContext";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { stubFetch } from "@/ui/test/stub-fetch";
import { preferencesApi } from "@/ui/test/mastodon-fixtures";

const OpenComposer = () => {
  const { openNew } = useCompose();
  return (
    <button type="button" onClick={openNew}>
      開く
    </button>
  );
};

const routes = {
  "GET /api/v1/preferences": preferencesApi,
  "GET /api/v1/custom_emojis": [],
  "GET /api/v1/announcements": [],
};

describe("ComposeSheet", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("closes an empty draft on Escape without asking", async () => {
    const user = userEvent.setup();
    const { restore } = stubFetch(routes);
    try {
      await renderPage(<OpenComposer />);
      await user.click(screen.getByRole("button", { name: "開く" }));
      await screen.findByRole("dialog", { name: "新規投稿" });
      await user.keyboard("{Escape}");
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "新規投稿" })).toBeNull());
    } finally {
      restore();
    }
  });

  it("asks before discarding a draft with text", async () => {
    const user = userEvent.setup();
    const { restore } = stubFetch(routes);
    try {
      await renderPage(<OpenComposer />);
      await user.click(screen.getByRole("button", { name: "開く" }));
      const sheet = await screen.findByRole("dialog", { name: "新規投稿" });
      await user.type(sheet.querySelector("textarea")!, "書きかけ");
      await user.keyboard("{Escape}");
      await user.click(
        within(await screen.findByRole("dialog", { name: "下書きを破棄" })).getByRole("button", { name: "キャンセル" }),
      );
      expect(screen.getByRole("dialog", { name: "新規投稿" })).toBeTruthy();
      expect(document.activeElement).toBe(sheet.querySelector("textarea"));

      await user.keyboard("{Escape}");
      await user.click(
        within(await screen.findByRole("dialog", { name: "下書きを破棄" })).getByRole("button", { name: "破棄" }),
      );
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "新規投稿" })).toBeNull());
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "開く" }));
    } finally {
      restore();
    }
  });
});
