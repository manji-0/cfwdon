/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConfirmProvider, useConfirm } from "@/ui/context/ConfirmContext";

const ConfirmProbe = () => {
  const { confirm, prompt, alert } = useConfirm();
  return (
    <div>
      <button
        type="button"
        onClick={() => {
          void confirm("この投稿を削除しますか？", { title: "削除", confirmLabel: "削除", danger: true }).then(
            (ok) => {
              document.body.dataset.confirmResult = String(ok);
            },
          );
        }}
      >
        ask-confirm
      </button>
      <button
        type="button"
        onClick={() => {
          void prompt("理由", { title: "通報", defaultValue: "spam" }).then((value) => {
            document.body.dataset.promptResult = value === null ? "null" : value;
          });
        }}
      >
        ask-prompt
      </button>
      <button
        type="button"
        onClick={() => {
          void alert("完了しました").then(() => {
            document.body.dataset.alertResult = "done";
          });
        }}
      >
        ask-alert
      </button>
    </div>
  );
};

const renderConfirm = () =>
  render(
    <ConfirmProvider>
      <ConfirmProbe />
    </ConfirmProvider>,
  );

describe("ConfirmProvider", () => {
  afterEach(() => {
    cleanup();
    delete document.body.dataset.confirmResult;
    delete document.body.dataset.promptResult;
    delete document.body.dataset.alertResult;
  });

  it("resolves confirm through OK and cancel", async () => {
    const user = userEvent.setup();
    renderConfirm();
    await user.click(screen.getByRole("button", { name: "ask-confirm" }));
    expect(screen.getByRole("dialog").textContent).toContain("この投稿を削除しますか？");
    await user.click(screen.getByRole("button", { name: "キャンセル" }));
    expect(document.body.dataset.confirmResult).toBe("false");
    expect(screen.queryByRole("dialog")).toBeNull();

    await user.click(screen.getByRole("button", { name: "ask-confirm" }));
    await user.click(screen.getByRole("button", { name: "削除" }));
    expect(document.body.dataset.confirmResult).toBe("true");
  });

  it("cancels confirm with Escape", async () => {
    const user = userEvent.setup();
    renderConfirm();
    await user.click(screen.getByRole("button", { name: "ask-confirm" }));
    await user.keyboard("{Escape}");
    expect(document.body.dataset.confirmResult).toBe("false");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("returns prompt input or null when cancelled", async () => {
    const user = userEvent.setup();
    renderConfirm();
    await user.click(screen.getByRole("button", { name: "ask-prompt" }));
    const field = screen.getByLabelText("内容");
    expect((field as HTMLInputElement).value).toBe("spam");
    await user.clear(field);
    await user.type(field, "harassment");
    await user.click(screen.getByRole("button", { name: "OK" }));
    expect(document.body.dataset.promptResult).toBe("harassment");

    await user.click(screen.getByRole("button", { name: "ask-prompt" }));
    await user.click(screen.getByRole("button", { name: "キャンセル" }));
    expect(document.body.dataset.promptResult).toBe("null");
  });

  it("resolves alert with a single OK", async () => {
    const user = userEvent.setup();
    renderConfirm();
    await user.click(screen.getByRole("button", { name: "ask-alert" }));
    expect(screen.queryByRole("button", { name: "キャンセル" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "OK" }));
    expect(document.body.dataset.alertResult).toBe("done");
  });
});
