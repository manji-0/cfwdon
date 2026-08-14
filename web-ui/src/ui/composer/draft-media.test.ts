import { err, ok } from "neverthrow";
import { describe, expect, it } from "vitest";
import { ComposerMedia } from "@/ui/composer/draft-media";

const file = new File(["x"], "photo.png", { type: "image/png" });

describe("ComposerMedia", () => {
  it("marks an uploading item ready or failed", () => {
    const uploading = ComposerMedia.uploading("local-1", file, "blob:preview");
    expect(ComposerMedia.markReady(uploading, "media-1")).toEqual({
      kind: "Ready",
      localId: "local-1",
      file,
      previewUrl: "blob:preview",
      mediaId: "media-1",
    });
    expect(ComposerMedia.markFailed(uploading, "失敗")).toEqual({
      kind: "Failed",
      localId: "local-1",
      file,
      previewUrl: "blob:preview",
      message: "失敗",
    });
  });

  it("completes only the matching uploading item", () => {
    const uploading = ComposerMedia.uploading("local-1", file, "blob:preview");
    const ready = ComposerMedia.markReady(ComposerMedia.uploading("local-2", file, "blob:other"), "already");
    const next = ComposerMedia.complete([uploading, ready], "local-1", ok("media-1"));
    expect(next).toEqual([
      ComposerMedia.markReady(uploading, "media-1"),
      ready,
    ]);
    expect(ComposerMedia.complete([ready], "local-2", err("失敗"))).toEqual([ready]);
  });

  it("caps append at four attachments and collects ready ids", () => {
    const items = Array.from({ length: 4 }, (_, index) =>
      ComposerMedia.uploading(`local-${index}`, file, `blob:${index}`),
    );
    const extra = ComposerMedia.uploading("local-4", file, "blob:4");
    expect(ComposerMedia.append(items, [extra])).toEqual(items);

    const mixed = [
      ComposerMedia.markReady(items[0]!, "id-0"),
      items[1]!,
    ];
    expect(ComposerMedia.readyIds(mixed)).toEqual(["id-0"]);
    expect(ComposerMedia.hasUploading(mixed)).toBe(true);
  });
});
