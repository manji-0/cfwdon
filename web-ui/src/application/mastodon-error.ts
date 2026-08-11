import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

export const mastodonErrorMessage = (error: MastodonFetchError): string => {
  switch (error.kind) {
    case "HttpStatus":
      return error.body.trim() || `リクエストに失敗しました (${error.status})`;
    case "NetworkError":
      return "ネットワークエラーが発生しました";
    case "ValidationError":
      return "サーバー応答の形式が不正です";
  }
};
