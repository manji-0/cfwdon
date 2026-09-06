/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { MemoryRouter } from "react-router";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AccountRow } from "@/ui/components/AccountRow";
import { accountProfileFixture } from "@/ui/test/mastodon-fixtures";

describe("AccountRow", () => {
  afterEach(() => {
    cleanup();
  });

  it("links to the profile and omits an action until one is provided", () => {
    render(
      <MemoryRouter>
        <AccountRow account={accountProfileFixture} />
      </MemoryRouter>,
    );
    expect(screen.getByRole("link").getAttribute("href")).toBe("/profile/acct-1");
    expect(screen.getByText("Alice")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("invokes the optional dismiss action", async () => {
    const user = userEvent.setup();
    const clicks: string[] = [];
    render(
      <MemoryRouter>
        <AccountRow
          account={accountProfileFixture}
          actionLabel="非表示"
          onAction={() => {
            clicks.push("dismiss");
          }}
        />
      </MemoryRouter>,
    );
    await user.click(screen.getByRole("button", { name: "非表示" }));
    expect(clicks).toEqual(["dismiss"]);
  });
});
