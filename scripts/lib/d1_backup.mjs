// Naming and retention rules for the scheduled D1 backups. The command line
// is scripts/d1_backup.mjs; the workflow is .github/workflows/d1-backup.yml.

export const BACKUP_SUFFIX = ".sql.gz.age";

const DAY_MS = 24 * 60 * 60 * 1000;

const pad = (value, width = 2) => String(value).padStart(width, "0");

/** `20261109T034312Z` for a date (UTC, second precision). */
export const backupStamp = (date) =>
  `${pad(date.getUTCFullYear(), 4)}${pad(date.getUTCMonth() + 1)}${pad(date.getUTCDate())}` +
  `T${pad(date.getUTCHours())}${pad(date.getUTCMinutes())}${pad(date.getUTCSeconds())}Z`;

/** The object key of the backup of `database` taken at `date`. */
export const backupKey = (prefix, database, date) =>
  `${prefix}${database}-${backupStamp(date)}${BACKUP_SUFFIX}`;

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * When the backup under `key` was taken, or `undefined` when `key` is not a
 * backup of `database` under `prefix` (another file, another database, a
 * malformed or impossible stamp).
 */
export const parseBackupKey = (prefix, database, key) => {
  const pattern = new RegExp(
    `^${escapeRegExp(prefix)}${escapeRegExp(database)}-(\\d{4})(\\d{2})(\\d{2})T(\\d{2})(\\d{2})(\\d{2})Z${escapeRegExp(BACKUP_SUFFIX)}$`,
  );
  const match = pattern.exec(key);
  if (!match) {
    return undefined;
  }
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number);
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  // Date.UTC rolls 20261340 over into another month; refuse it instead.
  return backupStamp(date) === match.slice(1, 4).join("") + "T" + match.slice(4).join("") + "Z"
    ? date
    : undefined;
};

const monthIndex = (date) => date.getUTCFullYear() * 12 + date.getUTCMonth();

/**
 * The backups the retention policy lets go. Kept:
 * - every backup younger than `keepDays` days;
 * - the first backup of each calendar month (UTC) for the current month and
 *   the `keepMonths - 1` before it;
 * - the newest `keepLatest` backups whatever their age, so a schedule that
 *   stopped running never prunes the last good copies away.
 * Keys that are not backups of `database` under `prefix` are never returned.
 */
export const selectPrunable = (
  keys,
  { prefix, database, now, keepDays = 35, keepMonths = 12, keepLatest = 7 },
) => {
  const backups = [...new Set(keys)]
    .map((key) => ({ key, takenAt: parseBackupKey(prefix, database, key) }))
    .filter((backup) => backup.takenAt !== undefined)
    .sort((a, b) => a.takenAt.getTime() - b.takenAt.getTime() || a.key.localeCompare(b.key));
  const keep = new Set();
  for (const backup of backups.slice(Math.max(0, backups.length - keepLatest))) {
    keep.add(backup.key);
  }
  const seenMonths = new Set();
  for (const backup of backups) {
    const month = monthIndex(backup.takenAt);
    if (!seenMonths.has(month)) {
      seenMonths.add(month);
      if (monthIndex(now) - month < keepMonths) {
        keep.add(backup.key);
      }
    }
    if (now.getTime() - backup.takenAt.getTime() < keepDays * DAY_MS) {
      keep.add(backup.key);
    }
  }
  return backups.filter((backup) => !keep.has(backup.key)).map((backup) => backup.key);
};
