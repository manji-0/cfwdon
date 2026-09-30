import { ok, type Result } from "neverthrow";
import type { HttpError, ValidationError } from "@/domain/errors/http-error";
import type { AccountSummary } from "@/domain/session/account";
import { SessionState, type SessionResolved } from "@/domain/session/session";

export type LoadSessionError = HttpError | ValidationError;

/** Port implemented by `infrastructure/api/web-session`; `null` means signed out. */
export type FetchSession = () => PromiseLike<Result<AccountSummary | null, LoadSessionError>>;

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

export const loadSession = async (
  fetchSession: FetchSession,
): Promise<Result<SessionResolved, never>> => {
  const result = await fetchSession();
  if (result.isErr()) {
    return ok(SessionState.failed(toFailureMessage(result.error)));
  }
  const account = result.value;
  if (account === null) {
    return ok(SessionState.anonymous());
  }
  return ok(SessionState.authenticated(account));
};
