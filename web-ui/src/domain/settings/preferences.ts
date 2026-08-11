import { z } from "zod";

/** TODO(Phase 3): Map Mastodon preference payloads for account and notification policy. */
export type AccountPreferences = Readonly<{
  defaultVisibility: string;
  defaultSensitive: boolean;
  defaultLanguage: string | null;
  defaultQuotePolicy: string;
  expandMedia: string;
  expandSpoilers: boolean;
}>;

export const AccountPreferences = {
  schema: z
    .object({
      "posting:default:visibility": z.string(),
      "posting:default:sensitive": z.boolean(),
      "posting:default:language": z.string().nullable(),
      "posting:default:quote_policy": z.string(),
      "reading:expand:media": z.string(),
      "reading:expand:spoilers": z.boolean(),
    })
    .transform(
      (value): AccountPreferences => ({
        defaultVisibility: value["posting:default:visibility"],
        defaultSensitive: value["posting:default:sensitive"],
        defaultLanguage: value["posting:default:language"],
        defaultQuotePolicy: value["posting:default:quote_policy"],
        expandMedia: value["reading:expand:media"],
        expandSpoilers: value["reading:expand:spoilers"],
      }),
    ),
} as const;
