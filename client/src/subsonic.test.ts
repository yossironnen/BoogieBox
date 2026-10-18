import { describe, expect, it } from 'vitest';
import { subsonicPasswordChecks, subsonicPasswordProblem } from './subsonic';

describe('Subsonic password rules', () => {
  it('reports each rule for live feedback', () => {
    expect(subsonicPasswordChecks('')).toEqual({ length: false, symbol: false });
    expect(subsonicPasswordChecks('abcdefgh')).toEqual({ length: true, symbol: false });
    expect(subsonicPasswordChecks('abc!')).toEqual({ length: false, symbol: true });
    expect(subsonicPasswordChecks('price€€€€')).toEqual({ length: true, symbol: true });
    // Accented letters are letters; spaces are not symbols.
    expect(subsonicPasswordChecks('éééé éééé').symbol).toBe(false);
    expect(subsonicPasswordChecks(`!${'a'.repeat(128)}`).length).toBe(false);
  });

  it('returns the first problem, matching the server rules', () => {
    expect(subsonicPasswordProblem('Ab1!', 'Ab1!')).toBe('Password must be at least 8 characters');
    expect(subsonicPasswordProblem('Abcdefg1', 'Abcdefg1')).toBe('Password must include at least one symbol');
    expect(subsonicPasswordProblem(`!${'a'.repeat(128)}`, '')).toBe('Password must be at most 128 characters');
    expect(subsonicPasswordProblem('abc\u0007defgh!', 'abc\u0007defgh!')).toBe('Password must not contain control characters');
    expect(subsonicPasswordProblem('Groove!26', 'Groove!27')).toBe('Passwords do not match');
    expect(subsonicPasswordProblem('Groove!26', 'Groove!26')).toBeNull();
    // Length counts characters, not UTF-16 units.
    expect(subsonicPasswordProblem('🎵🎵🎵🎵🎵🎵🎵', '🎵🎵🎵🎵🎵🎵🎵')).toBe('Password must be at least 8 characters');
  });
});
