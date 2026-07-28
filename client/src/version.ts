/**
 * Exposes the current BoogieBox app version and version-test helper.
 */

export const APP_VERSION = '0.8.152';

/** Returns the next patch version for a valid semantic version string. */
export function bumpPatchVersion(version: string): string {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (!m) throw new Error(`Invalid semver: ${version}`);
  const major = Number(m[1]);
  const minor = Number(m[2]);
  const patch = Number(m[3]) + 1;
  return `${major}.${minor}.${patch}`;
}
