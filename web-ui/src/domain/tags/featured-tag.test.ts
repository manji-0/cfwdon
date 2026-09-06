import { describe, expect, it } from "vitest";
import { FeaturedTag } from "@/domain/tags/featured-tag";

describe("FeaturedTag", () => {
  it("allows adding until the max count", () => {
    expect(FeaturedTag.canAdd(0)).toBe(true);
    expect(FeaturedTag.canAdd(FeaturedTag.maxCount - 1)).toBe(true);
    expect(FeaturedTag.canAdd(FeaturedTag.maxCount)).toBe(false);
  });
});
