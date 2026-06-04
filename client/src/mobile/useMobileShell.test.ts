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
});
