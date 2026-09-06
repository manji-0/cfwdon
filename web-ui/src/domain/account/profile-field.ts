export const PROFILE_FIELD_MAX_COUNT = 4;

export type ProfileField = Readonly<{
  name: string;
  value: string;
  verifiedAt: string | null;
}>;

export const ProfileField = {
  maxCount: PROFILE_FIELD_MAX_COUNT,

  empty: (): ProfileField => ({
    name: "",
    value: "",
    verifiedAt: null,
  }),

  pad: (fields: ReadonlyArray<ProfileField>): ReadonlyArray<ProfileField> => {
    const next = fields.slice(0, PROFILE_FIELD_MAX_COUNT).map((field) => ({ ...field }));
    while (next.length < PROFILE_FIELD_MAX_COUNT) {
      next.push(ProfileField.empty());
    }
    return next;
  },

  compact: (fields: ReadonlyArray<ProfileField>): ReadonlyArray<ProfileField> =>
    fields
      .map((field) => ({
        name: field.name.trim(),
        value: field.value.trim(),
        verifiedAt: field.verifiedAt,
      }))
      .filter((field) => field.name.length > 0 || field.value.length > 0),

  set: (
    fields: ReadonlyArray<ProfileField>,
    index: number,
    patch: Partial<Pick<ProfileField, "name" | "value">>,
  ): ReadonlyArray<ProfileField> =>
    fields.map((field, current) => (current === index ? { ...field, ...patch } : field)),
} as const;
