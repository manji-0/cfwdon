/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AccountRef } from "@/domain/account/account";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { StatusCard } from "@/ui/components/StatusCard";
import { renderWithRouter } from "@/ui/test/render-page";

const account: AccountRef = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
};

const status = (overrides: Readonly<{ spoilerText?: string; sensitive?: boolean }>) =>
  Status.original({
    id: "s1",
    createdAt: "2026-08-13T00:00:00.000Z",
    content: "<p>本文です</p>",
    spoilerText: overrides.spoilerText ?? "",
    sensitive: overrides.sensitive ?? false,
    visibility: Visibility.public(),
    inReplyToId: null,
    repliesCount: 0,
    reblogsCount: 0,
    favouritesCount: 0,
    favourited: false,
    reblogged: false,
    bookmarked: false,
    account,
    mediaAttachments: [
      {
        kind: "Image",
        id: "m1",
        url: "https://example.test/m1.png",
        previewUrl: "https://example.test/m1-small.png",
        description: "猫の写真",
      },
    ],
    card: null,
  });

describe("StatusCard", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows the text of a sensitive status without CW and hides only its media", async () => {
    const user = userEvent.setup();
    await renderWithRouter(<StatusCard status={status({ sensitive: true })} />);
    expect(screen.getByText("本文です")).toBeTruthy();
    expect(screen.queryByAltText("猫の写真")).toBeNull();
    await user.click(screen.getByRole("button", { name: /センシティブなメディア/ }));
    expect(screen.getByAltText("猫の写真")).toBeTruthy();
  });

  it("keeps sensitive media hidden after expanding a CW", async () => {
    const user = userEvent.setup();
    await renderWithRouter(<StatusCard status={status({ spoilerText: "ネタバレ", sensitive: true })} />);
    expect(screen.queryByText("本文です")).toBeNull();
    await user.click(screen.getByRole("button", { name: "CW: ネタバレ" }));
    expect(screen.getByText("本文です")).toBeTruthy();
    expect(screen.queryByAltText("猫の写真")).toBeNull();
    expect(screen.getByRole("button", { name: /センシティブなメディア/ })).toBeTruthy();
  });
});
