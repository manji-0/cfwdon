/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountFollowersPage } from "@/ui/pages/AccountCollectionPage";
import { cleanupPage, renderPage } from "@/ui/test/render-page";
import { jsonResponse, stubFetch } from "@/ui/test/stub-fetch";
import { accountProfileApi } from "@/ui/test/mastodon-fixtures";

const follower = (id: string) => ({
  ...accountProfileApi,
  id,
  username: `user${id}`,
  acct: `user${id}`,
  display_name: `User ${id}`,
});

const jsonResponseWithLink = (body: unknown, link: string): Response =>
  new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json", Link: link },
  });

describe("AccountCollectionPage paging", () => {
  afterEach(() => {
    cleanupPage();
  });

  it("requests the next page with the Link cursor, not the last account id", async () => {
    const user = userEvent.setup();
    // Random hex ids that the server's numeric cursor cast would reject.
    const firstPage = Array.from({ length: 20 }, (_, i) => follower(`ab${i}f`));
    const secondPage = [follower("cd01"), follower("cd02")];
    const seenMaxIds: Array<string | null> = [];
    const { restore } = stubFetch({
      "GET /api/v1/accounts/acct-1": accountProfileApi,
      "GET /api/v1/announcements": [],
      "GET /api/v1/accounts/acct-1/followers": (request: Request) => {
        const maxId = new URL(request.url).searchParams.get("max_id");
        seenMaxIds.push(maxId);
        if (maxId === null) {
          return jsonResponseWithLink(
            firstPage,
            '<https://example.test/api/v1/accounts/acct-1/followers?limit=20&max_id=42>; rel="next"',
          );
        }
        return jsonResponse(secondPage);
      },
    });
    try {
      // No :accountId route param, so the page falls back to the signed-in account.
      await renderPage(<AccountFollowersPage />);
      await screen.findByText("User ab0f");
      await user.click(await screen.findByRole("button", { name: "もっと見る" }));
      await screen.findByText("User cd02");
      expect(seenMaxIds.filter((id) => id !== null)).toEqual(["42"]);
      expect(seenMaxIds).not.toContain("ab19f");
      expect(screen.queryByRole("button", { name: "もっと見る" })).toBeNull();
    } finally {
      restore();
    }
  });
});
