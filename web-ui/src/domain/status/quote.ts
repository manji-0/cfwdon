import type { AccountRef } from "@/domain/account/account";

export type QuotedStatusPreview = Readonly<{
  id: string;
  content: string;
  spoilerText: string;
  account: AccountRef;
}>;

export type StatusQuote = Readonly<{
  state: string;
  quotedStatus: QuotedStatusPreview | null;
}>;

export const StatusQuote = {
  isVisible: (quote: StatusQuote): boolean =>
    quote.state === "accepted" && quote.quotedStatus !== null,
} as const;
