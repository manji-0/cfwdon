import { describe, expect, it } from "vitest";
import { parseUploadedMedia } from "@/infrastructure/mastodon/parsers/media";
import { isArkError } from "@/infrastructure/mastodon/parse";

describe("parseUploadedMedia", () => {
  it("parses worker media upload responses", () => {
    const result = parseUploadedMedia({
      id: "media-1",
      type: "image",
      url: "https://example.test/media/media-1",
      preview_url: "https://example.test/media/media-1",
    });

    if (isArkError(result)) {
      throw new Error(result.summary);
    }

    expect(result.id).toBe("media-1");
    expect(result.kind).toBe("Image");
  });
});
