import { ok, type Result } from "neverthrow";
import { SessionState, type SessionResolved } from "@/domain/session/session";
import type { FetchSessionError } from "@/infrastructure/api/web-session";
import { fetchWebSession } from "@/infrastructure/api/web-session";

export type LoadSessionError = FetchSessionError;

const toFailureMessage = (error: LoadSessionError): string => {
  switch (error.kind) {
    case "HttpStatus":
      return error.body.trim() || `セッションの取得に失敗しました (${error.status})`;
    case "NetworkError":
      return "ネットワークエラーが発生しました";
    case "ValidationError":
      return "サーバー応答の形式が不正です";
  }
};

export const loadSession = async (): Promise<Result<SessionResolved, never>> => {
  const result = await fetchWebSession();
  if (result.isErr()) {
    return ok(SessionState.failed(toFailureMessage(result.error)));
  }
  const account = result.value;
  if (account === null) {
    return ok(SessionState.anonymous());
  }
  return ok(SessionState.authenticated(account));
};
