import { describe, expect, it } from "vitest";
import { ProfileField } from "@/domain/account/profile-field";

describe("ProfileField", () => {
  it("pads to four slots and compact drops blanks", () => {
    const padded = ProfileField.pad([{ name: "site", value: "https://example.test", verifiedAt: null }]);
    expect(padded).toHaveLength(4);
    expect(ProfileField.compact(padded)).toEqual([
      { name: "site", value: "https://example.test", verifiedAt: null },
    ]);
  });

  it("updates a slot without mutating the rest", () => {
    const fields = ProfileField.pad([]);
    const next = ProfileField.set(fields, 0, { name: "Pronouns", value: "they" });
    expect(next[0]).toEqual({ name: "Pronouns", value: "they", verifiedAt: null });
    expect(next[1]).toEqual(ProfileField.empty());
  });
});
