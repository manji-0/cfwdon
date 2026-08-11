import { describe, expect, it } from "vitest";
import { Visibility } from "@/domain/status/visibility";

describe("Visibility", () => {
  it("round-trips through API values", () => {
    const visibility = Visibility.unlisted();
    expect(Visibility.toApi(visibility)).toBe("unlisted");
    expect(Visibility.fromApi("unlisted")).toEqual(visibility);
  });
});
