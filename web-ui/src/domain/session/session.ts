import type { AccountSummary } from "./account";

export type SessionAnonymous = Readonly<{
  kind: "Anonymous";
}>;

export type SessionAuthenticated = Readonly<{
  kind: "Authenticated";
  account: AccountSummary;
}>;

export type SessionLoading = Readonly<{
  kind: "Loading";
}>;

export type SessionFailed = Readonly<{
  kind: "Failed";
  message: string;
}>;

export type SessionResolved = SessionAnonymous | SessionAuthenticated | SessionFailed;

export type SessionState = SessionResolved | SessionLoading;

export const SessionState = {
  anonymous: (): SessionAnonymous => ({ kind: "Anonymous" }),

  authenticated: (account: AccountSummary): SessionAuthenticated => ({
    kind: "Authenticated",
    account,
  }),

  loading: (): SessionLoading => ({ kind: "Loading" }),

  failed: (message: string): SessionFailed => ({
    kind: "Failed",
    message,
  }),

  resolve: (_loading: SessionLoading, outcome: SessionResolved): SessionResolved => outcome,

  logout: (_session: SessionAuthenticated): SessionAnonymous => SessionState.anonymous(),

  updateAccount: (
    session: SessionAuthenticated,
    account: AccountSummary,
  ): SessionAuthenticated => ({
    ...session,
    account,
  }),

  isAuthenticated: (state: SessionState): state is SessionAuthenticated =>
    state.kind === "Authenticated",
} as const;
