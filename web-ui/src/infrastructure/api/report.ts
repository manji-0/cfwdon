import { type ResultAsync } from "neverthrow";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";

export type CreateReportInput = Readonly<{
  accountId: string;
  statusIds?: ReadonlyArray<string>;
  comment?: string;
}>;

export const createReport = (
  input: CreateReportInput,
): ResultAsync<void, MastodonFetchError> => {
  const body: Record<string, unknown> = {
    account_id: input.accountId,
  };
  if (input.statusIds && input.statusIds.length > 0) {
    body.status_ids = input.statusIds;
  }
  if (input.comment && input.comment.trim().length > 0) {
    body.comment = input.comment.trim();
  }
  return mastodonPostJson("/api/v1/reports", body).map(() => undefined);
};
