import { describe, expect, it } from "vitest";
import { linkifyMentionsInHtml, normalizeHttpsUrl } from "@/ui/lib/linkify-mentions";

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

  it("linkifies https URLs that contain /@user path segments", () => {
    const result = linkifyMentionsInHtml("<p>https://misskey.example/@natsuneko</p>");
    expect(result).toContain('href="https://misskey.example/@natsuneko"');
    expect(result).not.toContain('class="mention"');
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

  it("turns a plain https URL into an external link", () => {
    const result = linkifyMentionsInHtml(
      "<p>see https://github.com/rust-lang/rust/pull/161106 for details</p>",
    );
    expect(result).toContain(
      'href="https://github.com/rust-lang/rust/pull/161106"',
    );
    expect(result).toContain('rel="nofollow noopener noreferrer"');
    expect(result).toContain('target="_blank"');
    expect(result).toContain(">https://github.com/rust-lang/rust/pull/161106</a> for details");
  });

  it("stops glued latin text after a numeric path segment", () => {
    const result = linkifyMentionsInHtml(
      "<p>https://github.com/rust-lang/rust/pull/161106rust 1.100だそうで。</p>",
    );
    expect(result).toContain(
      'href="https://github.com/rust-lang/rust/pull/161106"',
    );
    expect(result).toContain("</a>rust 1.100だそうで。");
  });

  it("does not autolink http URLs", () => {
    const html = "<p>http://example.com</p>";
    expect(linkifyMentionsInHtml(html)).toBe(html);
  });

  it("does not rewrite URLs already inside an anchor", () => {
    const html = '<p><a href="https://example.com">https://example.com</a></p>';
    expect(linkifyMentionsInHtml(html)).toBe(html);
  });

  it("does not split mentions inside a https URL query string", () => {
    const result = linkifyMentionsInHtml("<p>https://example.com?q=%40bob</p>");
    expect(result).toContain('href="https://example.com?q=%40bob"');
    expect(result).not.toContain('class="mention"');
  });
});

describe("normalizeHttpsUrl", () => {
  it("removes trailing punctuation", () => {
    expect(normalizeHttpsUrl("https://example.com/path.")).toBe("https://example.com/path");
  });

  it("removes glued word suffix after digits", () => {
    expect(normalizeHttpsUrl("https://github.com/rust-lang/rust/issues/160895next")).toBe(
      "https://github.com/rust-lang/rust/issues/160895",
    );
  });
});
