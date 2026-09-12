/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountRow } from "@/ui/components/AccountRow";
import { accountProfileFixture } from "@/ui/test/mastodon-fixtures";
import { renderWithRouter } from "@/ui/test/render-page";

describe("AccountRow", () => {
  afterEach(() => {
    cleanup();
  });

  it("links to the profile and omits an action until one is provided", async () => {
    await renderWithRouter(<AccountRow account={accountProfileFixture} />);
    expect(screen.getByRole("link").getAttribute("href")).toBe("/profile/acct-1");
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("invokes the optional dismiss action", async () => {
    const user = userEvent.setup();
    const clicks: string[] = [];
    await renderWithRouter(
      <AccountRow
        account={accountProfileFixture}
        actionLabel="非表示"
        onAction={() => {
          clicks.push("dismiss");
        }}
      />,
    );
    await user.click(screen.getByRole("button", { name: "非表示" }));
    expect(clicks).toEqual(["dismiss"]);
  });
});
