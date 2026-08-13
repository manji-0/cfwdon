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

export type SessionState = SessionAnonymous | SessionAuthenticated | SessionLoading | SessionFailed;

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

  isAuthenticated: (state: SessionState): state is SessionAuthenticated =>
    state.kind === "Authenticated",
} as const;
