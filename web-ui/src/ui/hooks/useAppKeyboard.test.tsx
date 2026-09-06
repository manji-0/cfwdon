/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it } from "vitest";
import { MemoryRouter, useLocation } from "react-router";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { AppKeyboard } from "@/ui/hooks/useAppKeyboard";

const LocationProbe = () => {
  const location = useLocation();
  return <div data-testid="location">{location.pathname}</div>;
};

const renderKeyboard = (overlay = false) =>
  render(
    <MemoryRouter>
      <AppKeyboard />
      <LocationProbe />
      {overlay ? <div data-app-overlay="true">overlay</div> : null}
      <article data-status-id="s1">one</article>
      <article data-status-id="s2">two</article>
      <input data-testid="composer" />
    </MemoryRouter>,
  );

const press = (key: string, target: EventTarget = window): void => {
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
};

describe("useAppKeyboard", () => {
  afterEach(() => {
    cleanup();
  });

  it("moves j/k focus across status cards and opens the focused status", async () => {
    renderKeyboard();
    press("j");
    expect(document.querySelector('[data-status-id="s1"]')?.classList.contains("is-focused")).toBe(true);
    press("j");
    expect(document.querySelector('[data-status-id="s2"]')?.classList.contains("is-focused")).toBe(true);
    press("k");
    expect(document.querySelector('[data-status-id="s1"]')?.classList.contains("is-focused")).toBe(true);
    press("o");
    await waitFor(() => {
      expect(screen.getByTestId("location").textContent).toBe("/status/s1");
    });
  });

  it("navigates with a g chord", async () => {
    renderKeyboard();
    press("g");
    press("n");
    await waitFor(() => {
      expect(screen.getByTestId("location").textContent).toBe("/notifications");
    });
  });

  it("ignores j while an overlay is open", () => {
    renderKeyboard(true);
    press("j");
    expect(document.querySelector('[data-status-id="s1"]')?.classList.contains("is-focused")).toBe(false);
  });

  it("ignores j while the user is typing", () => {
    renderKeyboard();
    press("j", screen.getByTestId("composer"));
    expect(document.querySelector('[data-status-id="s1"]')?.classList.contains("is-focused")).toBe(false);
  });
});
