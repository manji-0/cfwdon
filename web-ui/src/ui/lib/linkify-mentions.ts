/** Must match `BrowserRouter` basename in `App`. */
const APP_BASENAME = "/app";

const TAG_OR_TEXT = /(<[^>]+>)/;

/** `@user` or `@user@host.tld`, not email and not `/@user` URL paths. */
const MENTION_PATTERN =
  "(?<![A-Za-z0-9_./])@([A-Za-z0-9_]+)(?:@((?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\\.)+[A-Za-z]{2,}))?";

const mentionHref = (acct: string): string =>
  `${APP_BASENAME}/search?q=${encodeURIComponent(`@${acct}`)}`;

const wrapMention = (raw: string, username: string, domain: string | undefined): string => {
  const acct = domain ? `${username}@${domain}` : username;
  return `<a class="mention" href="${mentionHref(acct)}">${raw}</a>`;
};

const isAnchorOpen = (tag: string): boolean => /^<a(\s|>|\/)/i.test(tag);

const isAnchorClose = (tag: string): boolean => /^<\/a\b/i.test(tag);

const linkifyText = (text: string): string =>
  text.replace(new RegExp(MENTION_PATTERN, "g"), (raw, username: string, domain?: string) =>
    wrapMention(raw, username, domain),
  );

/** Wrap `@user` / `@user@host` in sanitized status HTML without nesting inside existing links. */
export const linkifyMentionsInHtml = (html: string): string => {
  let inAnchor = 0;
  return html
    .split(TAG_OR_TEXT)
    .map((part) => {
      if (part.startsWith("<")) {
        if (isAnchorOpen(part)) {
          inAnchor += 1;
        } else if (isAnchorClose(part)) {
          inAnchor = Math.max(0, inAnchor - 1);
        }
        return part;
      }
      if (inAnchor > 0 || part.length === 0) {
        return part;
      }
      return linkifyText(part);
    })
    .join("");
};
