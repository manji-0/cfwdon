export const formatRelativeTime = (iso: string): string => {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  const diffMs = date.getTime() - Date.now();
  const diffSec = Math.round(diffMs / 1000);
  const formatter = new Intl.RelativeTimeFormat("ja", { numeric: "auto" });
  const absSec = Math.abs(diffSec);
  if (absSec < 60) {
    return formatter.format(diffSec, "second");
  }
  const diffMin = Math.round(diffSec / 60);
  if (Math.abs(diffMin) < 60) {
    return formatter.format(diffMin, "minute");
  }
  const diffHour = Math.round(diffMin / 60);
  if (Math.abs(diffHour) < 24) {
    return formatter.format(diffHour, "hour");
  }
  const diffDay = Math.round(diffHour / 24);
  if (Math.abs(diffDay) < 7) {
    return formatter.format(diffDay, "day");
  }
  return date.toLocaleDateString("ja-JP", {
    month: "short",
    day: "numeric",
  });
};
