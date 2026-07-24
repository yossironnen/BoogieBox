/**
 * Tests Use Mobile Shell.Test behavior for BoogieBox regressions.
 */

import { describe, expect, it } from 'vitest';
import { shouldUseMobileShell } from './useMobileShell';

function createWindowLike({
  width,
  height,
  userAgent,
  coarse,
}: {
  width: number;
  height: number;
  userAgent: string;
  coarse: boolean;
}) {
  return {
    innerWidth: width,
    innerHeight: height,
    navigator: { userAgent },
    matchMedia: (query: string) => ({ matches: query === '(pointer: coarse)' ? coarse : false }),
  } as unknown as Window;
}

describe('shouldUseMobileShell', () => {
  it('enables the mobile shell on narrow iphone-sized screens', () => {
    expect(shouldUseMobileShell(createWindowLike({ width: 390, height: 844, userAgent: 'iPhone', coarse: true }))).toBe(true);
  });

  it('keeps the desktop shell on wide layouts', () => {
    expect(shouldUseMobileShell(createWindowLike({ width: 1024, height: 768, userAgent: 'Macintosh', coarse: false }))).toBe(false);
  });

  it('supports mobile user agents and coarse pointers at their independent boundaries', () => {
    expect(shouldUseMobileShell(createWindowLike({ width: 900, height: 1200, userAgent: 'Mobile Safari', coarse: false }))).toBe(true);
    expect(shouldUseMobileShell(createWindowLike({ width: 600, height: 1000, userAgent: 'Desktop', coarse: true }))).toBe(true);
    expect(shouldUseMobileShell(createWindowLike({ width: 933, height: 800, userAgent: 'iPod', coarse: false }))).toBe(false);
    expect(shouldUseMobileShell(createWindowLike({ width: 641, height: 1000, userAgent: 'Desktop', coarse: true }))).toBe(false);
    expect(shouldUseMobileShell(createWindowLike({ width: 600, height: 1101, userAgent: 'Desktop', coarse: true }))).toBe(false);
  });

  it('handles environments without matchMedia', () => {
    const target = createWindowLike({ width: 800, height: 800, userAgent: 'Desktop', coarse: false });
    Object.defineProperty(target, 'matchMedia', { configurable: true, value: undefined });
    expect(shouldUseMobileShell(target)).toBe(false);
  });
});
