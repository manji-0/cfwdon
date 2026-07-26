/** Strip HTML tags and decode a small set of entities for tweet text. */
export function htmlToPlainText(html: string): string {
  let text = html
    .replace(/<\s*br\s*\/?\s*>/gi, "\n")
    .replace(/<\/\s*p\s*>/gi, "\n\n")
    .replace(/<\/\s*div\s*>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/\u00a0/g, " ");

  text = text
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&quot;/gi, '"')
    .replace(/&#39;/gi, "'")
    .replace(/&#x27;/gi, "'")
    .replace(/&#(\d+);/g, (_, digits: string) => {
      const code = Number.parseInt(digits, 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : "";
    });

  return text
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function buildTweetText(options: {
  contentHtml: string;
  sourceUrl: string;
  appendSourceUrl: boolean;
  maxChars: number;
}): string {
  const plain = htmlToPlainText(options.contentHtml);
  if (!options.appendSourceUrl || !options.sourceUrl) {
    return truncate(plain, options.maxChars);
  }

  const suffix = `\n\n${options.sourceUrl}`;
  if (plain.length + suffix.length <= options.maxChars) {
    return `${plain}${suffix}`;
  }

  const budget = options.maxChars - suffix.length;
  if (budget <= 1) {
    return truncate(options.sourceUrl, options.maxChars);
  }
  return `${truncate(plain, budget)}${suffix}`;
}

function truncate(text: string, maxChars: number): string {
  if (text.length <= maxChars) {
    return text;
  }
  if (maxChars <= 1) {
    return text.slice(0, maxChars);
  }
  return `${text.slice(0, maxChars - 1)}…`;
}
