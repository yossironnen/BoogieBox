/**
 * Tests Eq.Test behavior for BoogieBox regressions.
 */

import { describe, expect, it } from 'vitest';
import {
  BUILTIN_PARAMETRIC_PRESETS,
  DEFAULT_PARAMETRIC_BANDS,
  applyGraphicEqToFilters,
  applyParametricBandToFilter,
  applyParametricEqToFilters,
  clampDb,
  clampFreq,
  clampQ,
  computeCombinedResponseDb,
  isBuiltinEqProfileName,
  isBuiltinParametricPresetName,
  mapGraphicProfileToParametricPreset,
  migrateGraphicGainsToParametricBands,
  migrateGraphicProfileToParametricProfile,
  normalizeEqGains,
  normalizeParametricBand,
  parseStoredEqGains,
  parseStoredEqProfiles,
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

  it('rejects malformed graphic storage and normalizes valid values', () => {
    expect(normalizeEqGains(null)).toBeNull();
    expect(normalizeEqGains([1, 2])).toBeNull();
    expect(normalizeEqGains(Array(10).fill('bad'))).toBeNull();
    expect(normalizeEqGains([99, -99, 1.4, 2.6, 0, 0, 0, 0, 0, 0]))
      .toEqual([12, -12, 1, 3, 0, 0, 0, 0, 0, 0]);
    expect(parseStoredEqGains(undefined)).toBeNull();
    expect(parseStoredEqGains('{')).toBeNull();
    expect(parseStoredEqProfiles(undefined)).toEqual([]);
    expect(parseStoredEqProfiles('{}')).toEqual([]);
    expect(parseStoredEqProfiles('{')).toEqual([]);
    expect(parseStoredEqProfiles(JSON.stringify([
      null,
      {},
      { name: 'Rock', gains: Array(10).fill(0) },
      { name: 'Drive', gains: Array(10).fill(1) },
      { name: 'drive', gains: Array(10).fill(2) },
      { name: 'Broken', gains: [] },
    ]))).toEqual([{ name: 'Drive', gains: Array(10).fill(1) }]);
    expect(isBuiltinEqProfileName('Rock')).toBe(true);
    expect(isBuiltinEqProfileName('Nope')).toBe(false);
  });

  it('normalizes parametric defaults, types, clamps, and invalid inputs', () => {
    const defaults = DEFAULT_PARAMETRIC_BANDS[0];
    expect(normalizeParametricBand(null)).toBeNull();
    expect(normalizeParametricBand({ id: '', label: '' })).toBeNull();
    expect(normalizeParametricBand({ id: 'x', label: 'X', frequencyHz: 'bad', gainDb: 0, q: 1 })).toBeNull();
    expect(normalizeParametricBand({
      frequencyHz: 99999,
      gainDb: -99,
      q: 99,
      type: 'invalid',
      enabled: false,
    }, defaults)).toEqual({
      ...defaults,
      enabled: false,
      frequencyHz: 20000,
      gainDb: -12,
      q: 10,
    });
    expect(clampDb(20)).toBe(12);
    expect(clampFreq(1)).toBe(20);
    expect(clampQ(0)).toBe(0.2);
    expect(parseStoredParametricBands(undefined)).toBeNull();
    expect(parseStoredParametricBands('{')).toBeNull();
    expect(parseStoredParametricProfiles(undefined)).toEqual([]);
    expect(parseStoredParametricProfiles('{}')).toEqual([]);
    expect(parseStoredParametricProfiles('{')).toEqual([]);
    expect(isBuiltinParametricPresetName('Warm')).toBe(true);
    expect(isBuiltinParametricPresetName('Custom')).toBe(false);
  });

  it('computes every biquad type and ignores disabled or invalid coefficients', () => {
    const bands = DEFAULT_PARAMETRIC_BANDS.map((band, index) => ({
      ...band,
      type: ['peaking', 'low_shelf', 'high_shelf', 'highpass', 'lowpass', 'peaking', 'peaking'][index] as typeof band.type,
      frequencyHz: index === 6 ? 0 : band.frequencyHz,
      q: index === 5 ? 0 : band.q,
      gainDb: 3,
      enabled: index !== 4,
    }));
    const response = computeCombinedResponseDb(bands, new Float32Array([20, 100, 1000, 10000]));
    expect(Array.from(response).every(Number.isFinite)).toBe(true);
    expect(mapGraphicProfileToParametricPreset('Unknown' as never)).toBe('Manual');
    expect(migrateGraphicProfileToParametricProfile({ name: ' ', gains: Array(10).fill(0) })).toBeNull();
    expect(migrateGraphicProfileToParametricProfile({ name: 'Custom', gains: [] })).toBeNull();
  });

  it('applies graphic and parametric bands to Web Audio filter-like objects', () => {
    const makeFilter = () => ({
      type: 'peaking',
      frequency: { value: 0 },
      Q: { value: 0 },
      gain: { value: 0 },
    }) as unknown as BiquadFilterNode;
    const graphic = [makeFilter(), makeFilter()];
    applyGraphicEqToFilters(graphic, [4]);
    expect(graphic[0].gain.value).toBe(4);
    expect(graphic[1].gain.value).toBe(0);

    for (const type of ['peaking', 'low_shelf', 'high_shelf', 'highpass', 'lowpass'] as const) {
      const filter = makeFilter();
      applyParametricBandToFilter(filter, { ...DEFAULT_PARAMETRIC_BANDS[0], type, gainDb: 5 });
      expect(filter.gain.value).toBe(type === 'highpass' || type === 'lowpass' ? 0 : 5);
    }
    const filters = [makeFilter(), makeFilter()];
    applyParametricEqToFilters(filters, [
      { ...DEFAULT_PARAMETRIC_BANDS[0], enabled: false },
      { ...DEFAULT_PARAMETRIC_BANDS[1], enabled: true, gainDb: 3 },
    ]);
    expect(filters[0].gain.value).toBe(0);
    expect(filters[0].Q.value).toBe(1);
    expect(filters[1].gain.value).toBe(3);
  });
});
