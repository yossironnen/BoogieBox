/**
 * Shared Subsonic-app helpers: the credentials shape returned by `/api/subsonic/credentials` and the
 * Subsonic password rules (mirrors server-rs `subsonic::crypto::check_password_shape`; the server decides).
 */

/** Status of the signed-in user's Subsonic password. Never contains the password itself. */
export interface SubsonicCredentials {
  /** The admin has turned the Subsonic API on. */
  enabled: boolean;
  hasPassword: boolean;
  updatedAt: string | null;
  lastUsedAt: string | null;
  lastClient: string | null;
  /** LAN addresses apps can use as the server URL. */
  serverUrls: string[];
}

export const SUBSONIC_MIN_PASSWORD_CHARS = 8;
export const SUBSONIC_MAX_PASSWORD_CHARS = 128;

/** Per-rule pass/fail for live feedback. */
export function subsonicPasswordChecks(password: string): { length: boolean; symbol: boolean } {
  const length = Array.from(password).length;
  return {
    length: length >= SUBSONIC_MIN_PASSWORD_CHARS && length <= SUBSONIC_MAX_PASSWORD_CHARS,
    symbol: /[^\p{L}\p{N}\s]/u.test(password),
  };
}

/** First problem with a new password and its confirmation, or null when it can be submitted. */
export function subsonicPasswordProblem(password: string, confirmation: string): string | null {
  const length = Array.from(password).length;
  if (/\p{Cc}/u.test(password)) return 'Password must not contain control characters';
  if (length < SUBSONIC_MIN_PASSWORD_CHARS) return 'Password must be at least 8 characters';
  if (length > SUBSONIC_MAX_PASSWORD_CHARS) return 'Password must be at most 128 characters';
  if (!subsonicPasswordChecks(password).symbol) return 'Password must include at least one symbol';
  if (password !== confirmation) return 'Passwords do not match';
  return null;
}
