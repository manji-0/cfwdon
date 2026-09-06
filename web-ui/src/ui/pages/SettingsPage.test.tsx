/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPage } from "@/ui/pages/SettingsPage";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { stubFetch } from "@/ui/test/stub-fetch";
import {
  credentialsApi,
  featuredTagApi,
  keywordFilterApi,
  notificationPolicyApi,
  preferencesApi,
} from "@/ui/test/mastodon-fixtures";

const settingsRoutes = {
  "GET /api/v1/accounts/verify_credentials": credentialsApi,
  "GET /api/v1/preferences": preferencesApi,
  "GET /api/v1/notifications/policy": notificationPolicyApi,
  "GET /api/v1/mutes": [],
  "GET /api/v1/blocks": [],
  "GET /api/v2/filters": [keywordFilterApi],
  "GET /api/v1/domain_blocks": [],
  "GET /api/v1/followed_tags": [],
  "GET /api/v1/featured_tags": [featuredTagApi],
  "GET /api/v1/featured_tags/suggestions": [{ name: "rust" }],
  "GET /api/v1/announcements": [],
};

describe("SettingsPage", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("shows filter expiry, existing expiry copy, and featured tags", async () => {
    const { restore } = stubFetch(settingsRoutes);
    try {
      renderPage(<SettingsPage />, { path: "/settings" });
      await screen.findByRole("heading", { name: "キーワードフィルター" });
      expect(screen.getByText("期限切れ")).toBeTruthy();
      const filterSection = screen.getByRole("heading", { name: "キーワードフィルター" }).closest("section");
      expect(filterSection).not.toBeNull();
      const expiry = within(filterSection!).getByLabelText("期限") as HTMLSelectElement;
      expect([...expiry.options].map((option) => option.textContent)).toEqual([
        "期限なし",
        "30分",
        "1時間",
        "6時間",
        "12時間",
        "1日",
        "7日",
      ]);
      expect(screen.getByRole("heading", { name: "注目タグ" })).toBeTruthy();
      expect(screen.getByRole("link", { name: /#cfwdon/ })).toBeTruthy();
      expect(screen.getByRole("button", { name: "#rust" })).toBeTruthy();
    } finally {
      restore();
    }
  });

  it("posts expires_in when creating a filter with a duration", async () => {
    const user = userEvent.setup();
    const { recorded, restore } = stubFetch({
      ...settingsRoutes,
      "POST /api/v2/filters": {
        id: "filter-2",
        title: "bots",
        context: ["home", "notifications", "public", "thread", "account"],
        expires_at: "2026-09-14T00:00:00.000Z",
        filter_action: "warn",
        keywords: [{ id: "kw-2", keyword: "bot", whole_word: false }],
        statuses: [],
      },
    });
    try {
      renderPage(<SettingsPage />, { path: "/settings" });
      const filterSection = (await screen.findByRole("heading", { name: "キーワードフィルター" })).closest(
        "section",
      );
      expect(filterSection).not.toBeNull();
      const section = within(filterSection!);
      await user.type(section.getByLabelText("タイトル"), "bots");
      await user.type(section.getByLabelText("キーワード（カンマ区切り）"), "bot");
      await user.selectOptions(section.getByLabelText("期限"), "7d");
      await user.click(section.getByRole("button", { name: "追加" }));
      await waitFor(() => {
        expect(recorded.some((item) => item.method === "POST" && item.path === "/api/v2/filters")).toBe(true);
      });
      const created = recorded.find((item) => item.method === "POST" && item.path === "/api/v2/filters");
      expect(created?.body).toEqual({
        title: "bots",
        context: ["home", "notifications", "public", "thread", "account"],
        filter_action: "warn",
        expires_in: 604800,
        keywords: [{ keyword: "bot", whole_word: false }],
      });
      expect(section.getByText("bots")).toBeTruthy();
    } finally {
      restore();
    }
  });

  it("switches the expiry select to keep-current when editing a filter", async () => {
    const user = userEvent.setup();
    const { restore } = stubFetch(settingsRoutes);
    try {
      renderPage(<SettingsPage />, { path: "/settings" });
      const filterSection = (await screen.findByRole("heading", { name: "キーワードフィルター" })).closest(
        "section",
      );
      await user.click(within(filterSection!).getByRole("button", { name: "編集" }));
      const expiry = within(filterSection!).getByLabelText("期限") as HTMLSelectElement;
      expect([...expiry.options].map((option) => option.value)).toContain("keep");
      expect([...expiry.options].map((option) => option.value)).not.toContain("never");
      expect(expiry.value).toBe("keep");
    } finally {
      restore();
    }
  });

  it("posts a featured tag from the add form", async () => {
    const user = userEvent.setup();
    const { recorded, restore } = stubFetch({
      ...settingsRoutes,
      "POST /api/v1/featured_tags": {
        id: "tag-2",
        name: "rust",
        statuses_count: 0,
        last_status_at: null,
      },
    });
    try {
      renderPage(<SettingsPage />, { path: "/settings" });
      const featuredSection = (await screen.findByRole("heading", { name: "注目タグ" })).closest("section");
      const section = within(featuredSection!);
      await user.type(section.getByLabelText("ハッシュタグ"), "rust");
      await user.click(section.getByRole("button", { name: "追加" }));
      await waitFor(() => {
        expect(recorded.some((item) => item.method === "POST" && item.path === "/api/v1/featured_tags")).toBe(
          true,
        );
      });
      expect(
        recorded.find((item) => item.method === "POST" && item.path === "/api/v1/featured_tags")?.body,
      ).toEqual({ name: "rust" });
      expect(section.getByRole("link", { name: /#rust/ })).toBeTruthy();
    } finally {
      restore();
    }
  });
});
