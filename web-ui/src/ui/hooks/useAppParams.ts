import { useParams } from "@tanstack/react-router";

export const useAppParams = (): Readonly<{
  statusId?: string;
  accountId?: string;
  tagName?: string;
  conversationId?: string;
}> => useParams({ strict: false });
