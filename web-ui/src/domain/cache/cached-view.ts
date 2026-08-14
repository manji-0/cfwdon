export type AbsentView = Readonly<{
  kind: "Absent";
}>;

export type PresentView<T> = Readonly<{
  kind: "Present";
  value: T;
}>;

export type CachedView<T> = AbsentView | PresentView<T>;

export const CachedView = {
  absent: (): AbsentView => ({ kind: "Absent" }),

  present: <T>(value: T): PresentView<T> => ({ kind: "Present", value }),

  isPresent: <T>(view: CachedView<T>) => view.kind === "Present",

  isAbsent: <T>(view: CachedView<T>) => view.kind === "Absent",

  map: <T>(view: CachedView<T>, update: (value: T) => T): CachedView<T> => {
    switch (view.kind) {
      case "Absent":
        return view;
      case "Present":
        return CachedView.present(update(view.value));
    }
  },
} as const;
