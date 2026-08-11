import { describe, expect, it } from "vitest";
import { UploadedMedia } from "@/domain/media/attachment";

describe("UploadedMedia", () => {
  it("parses worker media upload responses", () => {
    const parsed = UploadedMedia.schema.parse({
      id: "media-1",
      type: "image",
      url: "https://example.test/media/media-1",
      preview_url: "https://example.test/media/media-1",
    });

    expect(parsed.id).toBe("media-1");
    expect(parsed.type).toBe("image");
  });
});
