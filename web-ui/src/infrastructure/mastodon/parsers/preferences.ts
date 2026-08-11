import { type } from "arktype";
import type { AccountPreferences } from "@/domain/settings/preferences";

export const parseAccountPreferences = type({
  "posting:default:visibility": "string",
  "posting:default:sensitive": "boolean",
  "posting:default:language": "string | null",
  "posting:default:quote_policy": "string",
  "reading:expand:media": "string",
  "reading:expand:spoilers": "boolean",
}).pipe(
  (value): AccountPreferences => ({
    defaultVisibility: value["posting:default:visibility"],
    defaultSensitive: value["posting:default:sensitive"],
    defaultLanguage: value["posting:default:language"],
    defaultQuotePolicy: value["posting:default:quote_policy"],
    expandMedia: value["reading:expand:media"],
    expandSpoilers: value["reading:expand:spoilers"],
  }),
);
