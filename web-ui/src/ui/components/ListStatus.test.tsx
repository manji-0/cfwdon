/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ListStatus } from "@/ui/components/ListStatus";

describe("ListStatus", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows the empty message only after a successful empty load", () => {
    const { rerender } = render(<ListStatus loading error="" isEmpty empty="空です" />);
    expect(screen.getByText("読み込み中…")).toBeTruthy();
    expect(screen.queryByText("空です")).toBeNull();

    rerender(<ListStatus loading={false} error="" isEmpty empty="空です" />);
    expect(screen.getByText("空です")).toBeTruthy();
  });

  it("shows an error with retry instead of the empty message", async () => {
    const user = userEvent.setup();
    const retries: string[] = [];
    render(
      <ListStatus
        loading={false}
        error="読み込みに失敗しました"
        isEmpty
        empty="空です"
        onRetry={() => retries.push("retry")}
      />,
    );
    expect(screen.getByRole("alert").textContent).toContain("読み込みに失敗しました");
    expect(screen.queryByText("空です")).toBeNull();
    await user.click(screen.getByRole("button", { name: "再試行" }));
    expect(retries).toEqual(["retry"]);
  });
});
