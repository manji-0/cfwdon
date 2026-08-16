/** Must match `BrowserRouter` basename in `App`. */
const APP_BASENAME = "/app";

const TAG_OR_TEXT = /(<[^>]+>)/;

/** `@user` or `@user@host.tld`, not email and not `/@user` URL paths. */
const MENTION_PATTERN =
  "(?<![A-Za-z0-9_./=%?&])@([A-Za-z0-9_]+)(?:@((?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\\.)+[A-Za-z]{2,}))?";

/** RFC 3986 unreserved/reserved characters allowed in https URLs. */
const HTTPS_URL_PATTERN = /https:\/\/[\w\-._~:/?#[\]@!$&'()*+,;=%]+/gi;

const mentionHref = (acct: string): string =>
  `${APP_BASENAME}/search?q=${encodeURIComponent(`@${acct}`)}`;

const escapeHtmlAttr = (value: string): string =>
  value.replace(/&/g, "&amp;").replace(/"/g, "&quot;");

const escapeHtmlText = (value: string): string =>
  value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const wrapMention = (raw: string, username: string, domain: string | undefined): string => {
  const acct = domain ? `${username}@${domain}` : username;
  return `<a class="mention" href="${mentionHref(acct)}">${raw}</a>`;
};

const isAnchorOpen = (tag: string): boolean => /^<a(\s|>|\/)/i.test(tag);

const isAnchorClose = (tag: string): boolean => /^<\/a\b/i.test(tag);

/** Trim trailing punctuation and latin text glued after a numeric path segment. */
export const normalizeHttpsUrl = (raw: string): string => {
  const url = raw.replace(/[.,;:!?)}\]'"]+$/, "");
  const hashIndex = url.indexOf("#");
  const hash = hashIndex === -1 ? "" : url.slice(hashIndex);
  const beforeHash = hashIndex === -1 ? url : url.slice(0, hashIndex);
  const queryIndex = beforeHash.indexOf("?");
  const query = queryIndex === -1 ? "" : beforeHash.slice(queryIndex);
  const pathAndOrigin = queryIndex === -1 ? beforeHash : beforeHash.slice(0, queryIndex);
  const degluedPath = pathAndOrigin.replace(/(\/\d+)([a-z][a-z]{1,})$/i, "$1");
  return `${degluedPath}${query}${hash}`;
};

const wrapHttpsUrl = (raw: string): string => {
  const url = normalizeHttpsUrl(raw);
  if (url.length <= "https://".length) {
    return raw;
  }
  const trailing = raw.slice(url.length);
  const link = `<a class="status-link" href="${escapeHtmlAttr(url)}" rel="nofollow noopener noreferrer" target="_blank">${escapeHtmlText(url)}</a>`;
  return trailing.length > 0 ? `${link}${escapeHtmlText(trailing)}` : link;
};

const linkifyHttpsUrls = (text: string): string =>
  text.replace(HTTPS_URL_PATTERN, (raw) => wrapHttpsUrl(raw));

const linkifyMentionsInText = (text: string): string =>
  text.replace(new RegExp(MENTION_PATTERN, "g"), (raw, username: string, domain?: string) =>
    wrapMention(raw, username, domain),
  );

const linkifyText = (text: string): string => linkifyMentionsInText(linkifyHttpsUrls(text));

/** Linkify `https://` URLs and `@user` mentions in sanitized status HTML. */
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
