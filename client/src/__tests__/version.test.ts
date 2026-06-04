/**
 * Tests Version.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import { APP_VERSION, bumpPatchVersion } from '../version';

describe('version', () => {
  it('uses a valid semver value', () => {
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it('bumps patch version by one', () => {
    expect(bumpPatchVersion('0.1.0')).toBe('0.1.1');
    expect(bumpPatchVersion('2.9.99')).toBe('2.9.100');
  });

  it('throws for invalid version format', () => {
    expect(() => bumpPatchVersion('1.0')).toThrow(/Invalid semver/);
    expect(() => bumpPatchVersion('abc')).toThrow(/Invalid semver/);
  });
});
