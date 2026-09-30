import { ok, type Result } from "neverthrow";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { AccountSummary } from "@/domain/session/account";
import { SessionState, type SessionResolved } from "@/domain/session/session";

/** Port implemented by `infrastructure/api/web-session`; `null` means signed out. */
export type FetchSession = () => PromiseLike<Result<AccountSummary | null, MastodonFetchError>>;

const toFailureMessage = (error: MastodonFetchError): string =>
  error.kind === "HttpStatus" && !error.body.trim()
    ? `セッションの取得に失敗しました (${error.status})`
    : mastodonErrorMessage(error);

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
