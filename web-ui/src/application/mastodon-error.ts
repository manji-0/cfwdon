import type { MastodonFetchError } from "@/domain/errors/http-error";

/** Mastodon error bodies are `{"error": "..."}`; fall back to the raw text. */
const httpErrorText = (body: string): string => {
  const text = body.trim();
  try {
    const parsed: unknown = JSON.parse(text);
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      "error" in parsed &&
      typeof parsed.error === "string"
    ) {
      return parsed.error.trim();
    }
  } catch {
    // Plain-text body.
  }
  return text;
};

export const mastodonErrorMessage = (error: MastodonFetchError): string => {
  switch (error.kind) {
    case "HttpStatus":
      return httpErrorText(error.body) || `リクエストに失敗しました (${error.status})`;
    case "NetworkError":
      return "ネットワークエラーが発生しました";
    case "ValidationError":
      return "サーバー応答の形式が不正です";
  }
};
