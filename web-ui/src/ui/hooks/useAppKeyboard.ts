import { useEffect, useRef } from "react";
import { useNavigate } from "react-router";
import { GoChord } from "@/domain/navigation/go-chord";
import { isOverlayOpen, isTypingTarget } from "@/ui/lib/keyboard";

const statusCards = (): HTMLElement[] =>
  [...document.querySelectorAll<HTMLElement>("[data-status-id]")];

const applyFocus = (cards: ReadonlyArray<HTMLElement>, index: number) => {
  cards.forEach((card, cardIndex) => {
    const focused = cardIndex === index;
    card.classList.toggle("is-focused", focused);
    if (focused) {
      card.setAttribute("aria-current", "true");
      card.scrollIntoView({ block: "nearest" });
    } else {
      card.removeAttribute("aria-current");
    }
  });
};

const focusedStatusId = (cards: ReadonlyArray<HTMLElement>, index: number): string | null =>
  cards[index]?.dataset.statusId ?? null;

export const useAppKeyboard = () => {
  const navigate = useNavigate();
  const indexRef = useRef(-1);
  const pendingGAtRef = useRef(0);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isOverlayOpen() || isTypingTarget(event.target)) {
        pendingGAtRef.current = 0;
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }

      const now = Date.now();
      if (pendingGAtRef.current > 0 && now - pendingGAtRef.current <= GoChord.timeoutMs) {
        const path = GoChord.pathFor(event.key);
        pendingGAtRef.current = 0;
        if (path) {
          event.preventDefault();
          event.stopImmediatePropagation();
          navigate(path);
        }
        return;
      }
      pendingGAtRef.current = 0;

      if (event.key === "g" || event.key === "G") {
        event.preventDefault();
        event.stopImmediatePropagation();
        pendingGAtRef.current = now;
        return;
      }

      const cards = statusCards();
      if (cards.length === 0) {
        return;
      }

      if (event.key === "j" || event.key === "J") {
        event.preventDefault();
        event.stopImmediatePropagation();
        const next = Math.min(cards.length - 1, Math.max(0, indexRef.current + 1));
        indexRef.current = next;
        applyFocus(cards, next);
        return;
      }

      if (event.key === "k" || event.key === "K") {
        event.preventDefault();
        event.stopImmediatePropagation();
        const next =
          indexRef.current < 0 ? 0 : Math.max(0, Math.min(cards.length - 1, indexRef.current - 1));
        indexRef.current = next;
        applyFocus(cards, next);
        return;
      }

      if (event.key === "o" || event.key === "O") {
        const statusId = focusedStatusId(cards, indexRef.current);
        if (!statusId) {
          return;
        }
        event.preventDefault();
        event.stopImmediatePropagation();
        navigate(`/status/${statusId}`);
        return;
      }

      if (event.key === "Enter") {
        const active = document.activeElement;
        if (
          active instanceof HTMLElement &&
          active !== document.body &&
          !active.hasAttribute("data-status-id")
        ) {
          return;
        }
        const statusId = focusedStatusId(cards, indexRef.current);
        if (!statusId) {
          return;
        }
        event.preventDefault();
        event.stopImmediatePropagation();
        navigate(`/status/${statusId}`);
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [navigate]);
};

export const AppKeyboard = () => {
  useAppKeyboard();
  return null;
};
