import { useEffect, useState } from "react";
import { isHelpShortcut, isTypingTarget, modKeyLabel } from "@/ui/lib/keyboard";

const SHORTCUTS = [
  { keys: `${modKeyLabel()} + Enter`, description: "投稿 / 返信を送信" },
  { keys: "n", description: "投稿欄にフォーカス" },
  { keys: "r", description: "ホームタイムラインを更新" },
  { keys: "j / k", description: "投稿を次 / 前へ" },
  { keys: "o / Enter", description: "フォーカス中の投稿を開く" },
  { keys: "g のあと", description: "h ホーム / n 通知 / s 検索 / e 探索 / p プロフィール" },
  { keys: "?", description: "ショートカット一覧を表示" },
  { keys: "Esc", description: "投稿欄のフォーカスを外す / この一覧を閉じる" },
] as const;

export const KeyboardShortcutsHelp = () => {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isHelpShortcut(event) && !isTypingTarget(event.target)) {
        event.preventDefault();
        setOpen((current) => !current);
        return;
      }
      if (event.key === "Escape" && open) {
        event.preventDefault();
        setOpen(false);
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  if (!open) {
    return null;
  }

  return (
    <div className="shortcut-overlay" data-app-overlay="true" role="presentation" onClick={() => setOpen(false)}>
      <section
        className="shortcut-dialog app-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcut-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="shortcut-dialog-header">
          <h2 id="shortcut-dialog-title">キーボードショートカット</h2>
          <button type="button" className="app-button app-button-secondary" onClick={() => setOpen(false)}>
            閉じる
          </button>
        </header>
        <dl className="shortcut-list">
          {SHORTCUTS.map((shortcut) => (
            <div key={shortcut.keys} className="shortcut-item">
              <dt>
                <kbd>{shortcut.keys}</kbd>
              </dt>
              <dd>{shortcut.description}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
};
