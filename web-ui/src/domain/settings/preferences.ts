/** Map Mastodon preference payloads for account and notification policy. */
export type AccountPreferences = Readonly<{
  defaultVisibility: string;
  defaultSensitive: boolean;
  defaultLanguage: string | null;
  defaultQuotePolicy: string;
  expandMedia: string;
  expandSpoilers: boolean;
}>;
