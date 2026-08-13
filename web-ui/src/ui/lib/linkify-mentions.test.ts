import { describe, expect, it } from "vitest";
import { linkifyMentionsInHtml } from "@/ui/lib/linkify-mentions";

describe("linkifyMentionsInHtml", () => {
  it("turns a remote acct into an in-app search link", () => {
    const html = "<p>hi @natsuneko@misskey.resonite.love there</p>";
    const result = linkifyMentionsInHtml(html);
    expect(result).toContain(
      'href="/app/search?q=%40natsuneko%40misskey.resonite.love"',
    );
    expect(result).toContain(">@natsuneko@misskey.resonite.love</a>");
    expect(result.startsWith("<p>")).toBe(true);
  });

  it("turns a local mention into a search link", () => {
    const result = linkifyMentionsInHtml("<p>hey @alice</p>");
    expect(result).toContain('href="/app/search?q=%40alice"');
    expect(result).toContain(">@alice</a>");
  });

  it("does not rewrite mentions already inside an anchor", () => {
    const html = '<p><a href="https://remote.example/@bob">@bob@remote.example</a></p>';
    expect(linkifyMentionsInHtml(html)).toBe(html);
  });

  it("does not treat /@user URL paths as mentions", () => {
    const html = "<p>https://misskey.example/@natsuneko</p>";
    expect(linkifyMentionsInHtml(html)).toBe(html);
  });

  it("leaves email-like text without a leading @ alone", () => {
    const html = "<p>write to natsuneko@misskey.example</p>";
    expect(linkifyMentionsInHtml(html)).toBe(html);
  });

  it("linkifies multiple mentions in one paragraph", () => {
    const result = linkifyMentionsInHtml("<p>@alice and @bob@remote.example</p>");
    expect(result).toContain('href="/app/search?q=%40alice"');
    expect(result).toContain('href="/app/search?q=%40bob%40remote.example"');
  });
});
