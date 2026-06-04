/**
 * Tests Eq.Test behavior for BoogieBox regressions.
 */

import { describe, expect, it } from 'vitest';
import {
  BUILTIN_PARAMETRIC_PRESETS,
  DEFAULT_PARAMETRIC_BANDS,
  computeCombinedResponseDb,
  mapGraphicProfileToParametricPreset,
  migrateGraphicGainsToParametricBands,
  migrateGraphicProfileToParametricProfile,
  parseStoredParametricBands,
  parseStoredParametricProfiles,
} from '../audio/eq';

describe('parametric EQ helpers', () => {
  it('parses and clamps stored 7-band state', () => {
    const raw = DEFAULT_PARAMETRIC_BANDS.map((band, index) => ({
      ...band,
      frequencyHz: index === 0 ? 2 : 440,
      gainDb: index === 1 ? 99 : -99,
      q: index === 2 ? 0.01 : 99,
    }));

    const parsed = parseStoredParametricBands(JSON.stringify(raw));

    expect(parsed).not.toBeNull();
    expect(parsed?.[0].frequencyHz).toBe(20);
    expect(parsed?.[1].gainDb).toBe(12);
    expect(parsed?.[2].q).toBe(0.2);
    expect(parsed?.[3].gainDb).toBe(-12);
    expect(parseStoredParametricBands(JSON.stringify(raw.slice(0, 6)))).toBeNull();
  });

  it('drops invalid, duplicate, and built-in custom parametric profiles', () => {
    const bands = DEFAULT_PARAMETRIC_BANDS.map((band) => ({ ...band }));
    const parsed = parseStoredParametricProfiles(JSON.stringify([
      { name: 'Warm', bands },
      { name: 'Studio', bands },
      { name: 'studio', bands },
      { name: 'Broken', bands: bands.slice(0, 4) },
    ]));

    expect(parsed).toEqual([{ name: 'Studio', bands }]);
  });

  it('keeps the manual response neutral and maps graphic presets conservatively', () => {
    const response = computeCombinedResponseDb(BUILTIN_PARAMETRIC_PRESETS.Manual, new Float32Array([60, 1000, 12000]));

    expect(Array.from(response)).toEqual([0, 0, 0]);
    expect(mapGraphicProfileToParametricPreset('Hip-Hop')).toBe('Bass Tighten');
    expect(mapGraphicProfileToParametricPreset('Vintage')).toBe('Vinyl Soft');
  });

  it('migrates legacy graphic gains into the canonical 7 parametric bands', () => {
    const migrated = migrateGraphicGainsToParametricBands([2, 4, 6, 8, 10, 12, -2, -4, -6, -8]);

    expect(migrated).not.toBeNull();
    expect(migrated?.map((band) => band.gainDb)).toEqual([3, 5, 9, 11, -3, -5, -7]);
    expect(migrated?.[0]).toMatchObject({
      id: DEFAULT_PARAMETRIC_BANDS[0].id,
      frequencyHz: DEFAULT_PARAMETRIC_BANDS[0].frequencyHz,
      type: DEFAULT_PARAMETRIC_BANDS[0].type,
    });
    expect(migrateGraphicGainsToParametricBands([1, 2, 3])).toBeNull();
  });

  it('migrates legacy custom graphic profiles unless they collide with built-in parametric names', () => {
    const migrated = migrateGraphicProfileToParametricProfile({
      name: 'Road Trip',
      gains: [0, 2, 4, 6, 8, 10, 12, -2, -4, -6],
    });

    expect(migrated?.name).toBe('Road Trip');
    expect(migrated?.bands).toHaveLength(DEFAULT_PARAMETRIC_BANDS.length);
    expect(migrateGraphicProfileToParametricProfile({
      name: 'Warm',
      gains: [0, 2, 4, 6, 8, 10, 12, -2, -4, -6],
    })).toBeNull();
  });
});
