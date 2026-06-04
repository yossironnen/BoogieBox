/** Parse a server datetime string as UTC.
 * SQLite datetime('now') produces "YYYY-MM-DD HH:MM:SS" with no timezone.
 * Without a 'Z' suffix, V8 interprets the string as local time — wrong.
 * This helper appends 'Z' so it is correctly treated as UTC before converting. */
/** Parse Server Date is part of this module's public API. */
export function parseServerDate(s: string | null | undefined): Date | null {
  if (!s) return null;
  // Already has timezone info — parse as-is
  if (s.endsWith('Z') || s.includes('+') || /[0-9][T ][0-9].*[-+][0-9]{2}:?[0-9]{2}$/.test(s)) {
    return new Date(s);
  }
  // Bare datetime from SQLite — treat as UTC
  return new Date(s.replace(' ', 'T') + 'Z');
}
