/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { useForegroundCatchUp } from "@/ui/hooks/useForegroundCatchUp";

const subscribeResume = vi.fn((_onResume?: unknown) => ({ close: vi.fn() }));

vi.mock("@/infrastructure/streaming/mastodon-stream", () => ({
  StreamingUser: {
    subscribeResume: (onResume: unknown) => subscribeResume(onResume),
  },
}));

const Probe = ({ onCatchUp }: Readonly<{ onCatchUp: () => void }>) => {
  useForegroundCatchUp(onCatchUp);
  return null;
};

describe("useForegroundCatchUp", () => {
  afterEach(() => {
    cleanup();
    subscribeResume.mockClear();
  });

  it("subscribes on mount and closes on unmount", () => {
    const onCatchUp = vi.fn();
    const close = vi.fn();
    subscribeResume.mockReturnValue({ close });
    const view = render(<Probe onCatchUp={onCatchUp} />);
    expect(subscribeResume).toHaveBeenCalledWith(onCatchUp);
    view.unmount();
    expect(close).toHaveBeenCalledTimes(1);
  });
});
