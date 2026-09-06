/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { ProfilePage } from "@/ui/pages/ProfilePage";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { stubFetch } from "@/ui/test/stub-fetch";
import { accountProfileApi, featuredTagApi } from "@/ui/test/mastodon-fixtures";

describe("ProfilePage", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("renders profile fields and featured tags for the signed-in account", async () => {
    const { restore } = stubFetch({
      "GET /api/v1/accounts/acct-1": accountProfileApi,
      "GET /api/v1/accounts/acct-1/statuses": [],
      "GET /api/v1/accounts/acct-1/featured_tags": [featuredTagApi],
      "GET /api/v1/announcements": [],
    });
    try {
      renderPage(<ProfilePage />, { path: "/profile" });
      await screen.findByRole("heading", { name: "Alice" });
      expect(screen.getByText("site")).toBeTruthy();
      expect(screen.getByText("https://example.test")).toBeTruthy();
      const tag = screen.getByRole("link", { name: "#cfwdon" });
      expect(tag.getAttribute("href")).toBe("/tags/cfwdon");
      expect(screen.getByText("4")).toBeTruthy();
      expect(screen.getByRole("button", { name: "プロフィールを編集" })).toBeTruthy();
    } finally {
      restore();
    }
  });
});
