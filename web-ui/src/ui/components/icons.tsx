import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

/** cfwdon brand mark: cloud edge with federated nodes. */
export const IconBrand = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
    <path
      d="M6.75 14.25h9.75a3.5 3.5 0 1 0-.78-6.94A4.75 4.75 0 0 0 6.1 6.55a3.5 3.5 0 0 0 .65 7.7Z"
      fill="currentColor"
    />
    <circle cx="8.75" cy="18.35" r="1.3" fill="currentColor" />
    <circle cx="12" cy="18.35" r="1.3" fill="currentColor" />
    <circle cx="15.25" cy="18.35" r="1.3" fill="currentColor" />
  </svg>
);

export const IconHome = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
    <path d="M4 10.5 12 4l8 6.5V20a1 1 0 0 1-1 1h-5v-6H10v6H5a1 1 0 0 1-1-1v-9.5Z" />
  </svg>
);

export const IconBell = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
    <path d="M18 16v-5a6 6 0 1 0-12 0v5l-2 2h16l-2-2Z" />
    <path d="M10 20a2 2 0 0 0 4 0" />
  </svg>
);

export const IconSearch = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
    <circle cx="11" cy="11" r="6" />
    <path d="m20 20-3.5-3.5" />
  </svg>
);

export const IconUser = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
    <circle cx="12" cy="8" r="4" />
    <path d="M5 20a7 7 0 0 1 14 0" />
  </svg>
);

export const IconPen = (props: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5Z" />
  </svg>
);

