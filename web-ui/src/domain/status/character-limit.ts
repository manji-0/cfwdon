/** Matches `configuration.statuses.max_characters` advertised by the instance. */
export const STATUS_MAX_CHARACTERS = 500;

export const StatusCharacters = {
  count: (text: string): number => [...text].length,

  remaining: (text: string, max = STATUS_MAX_CHARACTERS): number =>
    max - StatusCharacters.count(text),

  isWithinLimit: (text: string, max = STATUS_MAX_CHARACTERS): boolean =>
    StatusCharacters.remaining(text, max) >= 0,
} as const;
