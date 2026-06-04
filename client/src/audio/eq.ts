/**
 * Defines Eq behavior for BoogieBox.
 */

// Shared EQ model, constants, helpers, and Web Audio utilities for both
// graphic (10-band fixed) and parametric (7-band configurable) EQ modes.

// ─── Types ────────────────────────────────────────────────────────────────────

/** Eq Mode is part of this module's public API. */
export type EqMode = 'graphic' | 'parametric';

/** Parametric Eq Band Type is part of this module's public API. */
export type ParametricEqBandType = 'peaking' | 'low_shelf' | 'high_shelf' | 'highpass' | 'lowpass';

/** Parametric Eq Band is part of this module's public API. */
export interface ParametricEqBand {
  id: string;
  label: string;
  enabled: boolean;
  type: ParametricEqBandType;
  frequencyHz: number;
  gainDb: number;
  q: number;
}

/** Parametric Eq Profile is part of this module's public API. */
export interface ParametricEqProfile {
  name: string;
  bands: ParametricEqBand[];
}

// ─── Graphic EQ constants ─────────────────────────────────────────────────────

/** EQ BANDS is part of this module's public API. */
export const EQ_BANDS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
/** EQ MIN DB is part of this module's public API. */
export const EQ_MIN_DB = -12;
/** EQ MAX DB is part of this module's public API. */
export const EQ_MAX_DB = 12;
/** EQ Q is part of this module's public API. */
export const EQ_Q = 1.1;

/** Builtin Eq Profile Name is part of this module's public API. */
export type BuiltinEqProfileName =
  | 'Manual'
  | 'Rock'
  | 'Metal'
  | 'Pop'
  | 'Punk'
  | 'Electronic'
  | 'Club'
  | 'Hip-Hop'
  | 'Soul'
  | 'Acoustic'
  | 'Atmosphere'
  | 'Classical'
  | 'Vintage';

/** BUILTIN EQ PROFILE GAINS is part of this module's public API. */
export const BUILTIN_EQ_PROFILE_GAINS: Record<BuiltinEqProfileName, number[]> = {
  Manual:     [0,  0,  0,  0,  0,  0,  0,  0,  0,  0],
  Rock:       [4,  3,  2,  0, -1,  0,  2,  3,  4,  3],
  Metal:      [5,  4,  2,  0, -2,  0,  3,  4,  3,  2],
  Pop:        [-1, 2,  3,  4,  2,  0, -1,  2,  3,  4],
  Punk:       [3,  3,  2,  0, -1,  0,  3,  4,  4,  3],
  Electronic: [5,  4,  2,  0, -1,  0,  2,  3,  4,  3],
  Club:       [6,  5,  3,  1, -1,  0,  2,  3,  4,  3],
  'Hip-Hop':  [6,  5,  4,  2,  0,  0, -1, -1,  0,  0],
  Soul:       [2,  2,  1,  0,  1,  2,  3,  2,  1,  0],
  Acoustic:   [3,  2,  1,  0,  0,  1,  2,  3,  2,  1],
  Atmosphere: [1,  1,  0, -1, -1,  0,  1,  2,  2,  3],
  Classical:  [2,  1,  0, -1, -1,  0,  2,  3,  4,  4],
  Vintage:    [4,  3,  2,  0,  1,  2,  2,  1,  0, -1],
};

/** BUILTIN EQ PROFILE NAMES is part of this module's public API. */
export const BUILTIN_EQ_PROFILE_NAMES = Object.keys(BUILTIN_EQ_PROFILE_GAINS) as BuiltinEqProfileName[];

/** Is Builtin Eq Profile Name is part of this module's public API. */
export function isBuiltinEqProfileName(name: string): name is BuiltinEqProfileName {
  return Object.prototype.hasOwnProperty.call(BUILTIN_EQ_PROFILE_GAINS, name);
}

/** User Eq Profile is part of this module's public API. */
export interface UserEqProfile {
  name: string;
  gains: number[];
}

/** Normalize Eq Gains is part of this module's public API. */
export function normalizeEqGains(values: unknown): number[] | null {
  if (!Array.isArray(values) || values.length !== EQ_BANDS.length) return null;
  const out: number[] = [];
  for (const value of values) {
    const n = Number(value);
    if (!Number.isFinite(n)) return null;
    out.push(Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, Math.round(n))));
  }
  return out;
}

/** Parse Stored Eq Profiles is part of this module's public API. */
export function parseStoredEqProfiles(raw: string | undefined): UserEqProfile[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    const seen = new Set<string>();
    const out: UserEqProfile[] = [];
    for (const item of parsed) {
      if (!item || typeof item !== 'object') continue;
      const name = String((item as { name?: unknown }).name ?? '').trim();
      const lower = name.toLowerCase();
      if (!name || isBuiltinEqProfileName(name) || seen.has(lower)) continue;
      const gains = normalizeEqGains((item as { gains?: unknown }).gains);
      if (!gains) continue;
      seen.add(lower);
      out.push({ name, gains });
    }
    return out;
  } catch {
    return [];
  }
}

/** Parse Stored Eq Gains is part of this module's public API. */
export function parseStoredEqGains(raw: string | undefined): number[] | null {
  if (!raw) return null;
  try {
    return normalizeEqGains(JSON.parse(raw));
  } catch {
    return null;
  }
}

// ─── Parametric EQ constants ──────────────────────────────────────────────────

/** PARAMETRIC FREQ MIN is part of this module's public API. */
export const PARAMETRIC_FREQ_MIN = 20;
/** PARAMETRIC FREQ MAX is part of this module's public API. */
export const PARAMETRIC_FREQ_MAX = 20000;
/** PARAMETRIC DB MIN is part of this module's public API. */
export const PARAMETRIC_DB_MIN = -12;
/** PARAMETRIC DB MAX is part of this module's public API. */
export const PARAMETRIC_DB_MAX = 12;
/** PARAMETRIC Q MIN is part of this module's public API. */
export const PARAMETRIC_Q_MIN = 0.2;
/** PARAMETRIC Q MAX is part of this module's public API. */
export const PARAMETRIC_Q_MAX = 10;

/** Clamp DB is part of this module's public API. */
export function clampDb(v: number): number {
  return Math.max(PARAMETRIC_DB_MIN, Math.min(PARAMETRIC_DB_MAX, v));
}

/** Clamp Freq is part of this module's public API. */
export function clampFreq(v: number): number {
  return Math.max(PARAMETRIC_FREQ_MIN, Math.min(PARAMETRIC_FREQ_MAX, v));
}

/** Clamp Q is part of this module's public API. */
export function clampQ(v: number): number {
  return Math.max(PARAMETRIC_Q_MIN, Math.min(PARAMETRIC_Q_MAX, v));
}

/** DEFAULT PARAMETRIC BANDS is part of this module's public API. */
export const DEFAULT_PARAMETRIC_BANDS: ParametricEqBand[] = [
  { id: 'b1', label: 'Sub',     enabled: true, type: 'low_shelf',  frequencyHz: 60,    gainDb: 0, q: 0.7 },
  { id: 'b2', label: 'Bass',    enabled: true, type: 'peaking',    frequencyHz: 120,   gainDb: 0, q: 1.0 },
  { id: 'b3', label: 'Mud',     enabled: true, type: 'peaking',    frequencyHz: 300,   gainDb: 0, q: 1.0 },
  { id: 'b4', label: 'Low Mid', enabled: true, type: 'peaking',    frequencyHz: 800,   gainDb: 0, q: 1.0 },
  { id: 'b5', label: 'Pres',    enabled: true, type: 'peaking',    frequencyHz: 2500,  gainDb: 0, q: 1.0 },
  { id: 'b6', label: 'Bite',    enabled: true, type: 'peaking',    frequencyHz: 6000,  gainDb: 0, q: 1.0 },
  { id: 'b7', label: 'Air',     enabled: true, type: 'high_shelf', frequencyHz: 12000, gainDb: 0, q: 0.7 },
];

const VALID_BAND_TYPES = new Set<ParametricEqBandType>(['peaking', 'low_shelf', 'high_shelf', 'highpass', 'lowpass']);

/** Normalize Parametric Band is part of this module's public API. */
export function normalizeParametricBand(raw: unknown, defaults?: ParametricEqBand): ParametricEqBand | null {
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  const id = String(obj.id ?? defaults?.id ?? '').trim();
  const label = String(obj.label ?? defaults?.label ?? '').trim();
  if (!id || !label) return null;
  const rawType = obj.type as string;
  const type: ParametricEqBandType = VALID_BAND_TYPES.has(rawType as ParametricEqBandType)
    ? (rawType as ParametricEqBandType)
    : (defaults?.type ?? 'peaking');
  const rawFreq = Number(obj.frequencyHz);
  const rawGain = Number(obj.gainDb);
  const rawQ = Number(obj.q);
  if (!Number.isFinite(rawFreq) || !Number.isFinite(rawGain) || !Number.isFinite(rawQ)) return null;
  return {
    id,
    label,
    enabled: obj.enabled !== false,
    type,
    frequencyHz: clampFreq(rawFreq),
    gainDb: clampDb(rawGain),
    q: clampQ(rawQ),
  };
}

/** Parse Stored Parametric Bands is part of this module's public API. */
export function parseStoredParametricBands(raw: string | undefined): ParametricEqBand[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed) || parsed.length !== DEFAULT_PARAMETRIC_BANDS.length) return null;
    const bands: ParametricEqBand[] = [];
    for (let i = 0; i < parsed.length; i++) {
      const band = normalizeParametricBand(parsed[i], DEFAULT_PARAMETRIC_BANDS[i]);
      if (!band) return null;
      bands.push(band);
    }
    return bands;
  } catch {
    return null;
  }
}

function normalizeParametricProfile(raw: unknown): ParametricEqProfile | null {
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  const name = String(obj.name ?? '').trim();
  if (!name) return null;
  if (!Array.isArray(obj.bands) || obj.bands.length !== DEFAULT_PARAMETRIC_BANDS.length) return null;
  const bands: ParametricEqBand[] = [];
  for (let i = 0; i < obj.bands.length; i++) {
    const band = normalizeParametricBand(obj.bands[i], DEFAULT_PARAMETRIC_BANDS[i]);
    if (!band) return null;
    bands.push(band);
  }
  return { name, bands };
}

/** Parse Stored Parametric Profiles is part of this module's public API. */
export function parseStoredParametricProfiles(raw: string | undefined): ParametricEqProfile[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    const seen = new Set<string>();
    const out: ParametricEqProfile[] = [];
    for (const item of parsed) {
      const profile = normalizeParametricProfile(item);
      if (!profile) continue;
      const lower = profile.name.toLowerCase();
      if (isBuiltinParametricPresetName(profile.name) || seen.has(lower)) continue;
      seen.add(lower);
      out.push(profile);
    }
    return out;
  } catch {
    return [];
  }
}

// ─── Built-in parametric presets ─────────────────────────────────────────────

function makePreset(gains: number[]): ParametricEqBand[] {
  return DEFAULT_PARAMETRIC_BANDS.map((b, i) => ({ ...b, gainDb: gains[i] ?? 0 }));
}

/** BUILTIN PARAMETRIC PRESETS is part of this module's public API. */
export const BUILTIN_PARAMETRIC_PRESETS: Record<string, ParametricEqBand[]> = {
  Manual:        makePreset([0,  0,  0,  0,  0,  0,  0]),
  Warm:          makePreset([3,  2,  1,  0, -1, -2, -2]),
  Bright:        makePreset([-2, -1,  0,  0,  1,  3,  4]),
  'Vocal Focus': makePreset([-3, -2, -1,  2,  4,  1, -1]),
  'Bass Tighten':makePreset([-2,  4, -2, -1,  0,  0,  0]),
  'Late Night':  makePreset([-4, -3, -1,  0,  0, -1, -3]),
  'Vinyl Soft':  makePreset([2,   3,  1,  0, -1, -3, -4]),
};

/** BUILTIN PARAMETRIC PRESET NAMES is part of this module's public API. */
export const BUILTIN_PARAMETRIC_PRESET_NAMES = Object.keys(BUILTIN_PARAMETRIC_PRESETS);

/** Is Builtin Parametric Preset Name is part of this module's public API. */
export function isBuiltinParametricPresetName(name: string): boolean {
  return Object.prototype.hasOwnProperty.call(BUILTIN_PARAMETRIC_PRESETS, name);
}

// ─── Biquad frequency response computation ───────────────────────────────────
// Uses Audio EQ Cookbook formulas. Sample rate 48 kHz; 20–20 kHz is safely
// below Nyquist so no special clamping is needed.

const CANVAS_SAMPLE_RATE = 48000;

interface BiquadCoefficients {
  b0: number; b1: number; b2: number;
  a1: number; a2: number; // a0-normalised (a0=1)
}

function computeCoefficients(band: ParametricEqBand): BiquadCoefficients | null {
  const { type, frequencyHz, gainDb, q } = band;
  if (frequencyHz <= 0 || q <= 0) return null;
  const w0 = (2 * Math.PI * frequencyHz) / CANVAS_SAMPLE_RATE;
  const cosw = Math.cos(w0);
  const sinw = Math.sin(w0);
  const alpha = sinw / (2 * q);
  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;

  if (type === 'peaking') {
    const A = Math.pow(10, gainDb / 40);
    b0 = 1 + alpha * A;
    b1 = -2 * cosw;
    b2 = 1 - alpha * A;
    a0 = 1 + alpha / A;
    a1 = -2 * cosw;
    a2 = 1 - alpha / A;
  } else if (type === 'low_shelf') {
    const A = Math.pow(10, gainDb / 40);
    const sqA = Math.sqrt(A);
    const aS = (sinw / 2) * Math.SQRT2; // S=1
    b0 = A  * ((A + 1) - (A - 1) * cosw + 2 * sqA * aS);
    b1 = 2 * A * ((A - 1) - (A + 1) * cosw);
    b2 = A  * ((A + 1) - (A - 1) * cosw - 2 * sqA * aS);
    a0 =      (A + 1) + (A - 1) * cosw + 2 * sqA * aS;
    a1 = -2 * ((A - 1) + (A + 1) * cosw);
    a2 =      (A + 1) + (A - 1) * cosw - 2 * sqA * aS;
  } else if (type === 'high_shelf') {
    const A = Math.pow(10, gainDb / 40);
    const sqA = Math.sqrt(A);
    const aS = (sinw / 2) * Math.SQRT2; // S=1
    b0 = A  * ((A + 1) + (A - 1) * cosw + 2 * sqA * aS);
    b1 = -2 * A * ((A - 1) + (A + 1) * cosw);
    b2 = A  * ((A + 1) + (A - 1) * cosw - 2 * sqA * aS);
    a0 =      (A + 1) - (A - 1) * cosw + 2 * sqA * aS;
    a1 = 2  * ((A - 1) - (A + 1) * cosw);
    a2 =      (A + 1) - (A - 1) * cosw - 2 * sqA * aS;
  } else if (type === 'highpass') {
    b0 =  (1 + cosw) / 2;
    b1 = -(1 + cosw);
    b2 =  (1 + cosw) / 2;
    a0 =  1 + alpha;
    a1 = -2 * cosw;
    a2 =  1 - alpha;
  } else { // lowpass
    b0 = (1 - cosw) / 2;
    b1 =  1 - cosw;
    b2 = (1 - cosw) / 2;
    a0 =  1 + alpha;
    a1 = -2 * cosw;
    a2 =  1 - alpha;
  }

  if (a0 === 0) return null;
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

function magnitudeDb(c: BiquadCoefficients, w: number): number {
  const cw = Math.cos(w);
  const sw = Math.sin(w);
  const c2w = Math.cos(2 * w);
  const s2w = Math.sin(2 * w);
  const nr = c.b0 + c.b1 * cw + c.b2 * c2w;
  const ni = -(c.b1 * sw + c.b2 * s2w);
  const dr = 1 + c.a1 * cw + c.a2 * c2w;
  const di = -(c.a1 * sw + c.a2 * s2w);
  const denom = dr * dr + di * di;
  if (denom <= 0) return -120;
  return 10 * Math.log10((nr * nr + ni * ni) / denom);
}

// Returns combined dB response at each frequency in `frequencies` (Hz).
/** Compute Combined Response DB is part of this module's public API. */
export function computeCombinedResponseDb(
  bands: ParametricEqBand[],
  frequencies: Float32Array,
): Float32Array {
  const result = new Float32Array(frequencies.length);
  const coefs = bands
    .filter((b) => b.enabled)
    .map(computeCoefficients)
    .filter((c): c is BiquadCoefficients => c !== null);

  for (let i = 0; i < frequencies.length; i++) {
    const w = (2 * Math.PI * frequencies[i]) / CANVAS_SAMPLE_RATE;
    let db = 0;
    for (const c of coefs) db += magnitudeDb(c, w);
    result[i] = db;
  }
  return result;
}

// ─── Auto EQ mapping ──────────────────────────────────────────────────────────

const GRAPHIC_TO_PARAMETRIC: Record<BuiltinEqProfileName, string> = {
  Manual:     'Manual',
  Rock:       'Warm',
  Metal:      'Bright',
  Pop:        'Vocal Focus',
  Punk:       'Bright',
  Electronic: 'Bass Tighten',
  Club:       'Bass Tighten',
  'Hip-Hop':  'Bass Tighten',
  Soul:       'Warm',
  Acoustic:   'Vocal Focus',
  Atmosphere: 'Late Night',
  Classical:  'Late Night',
  Vintage:    'Vinyl Soft',
};

/** Map Graphic Profile To Parametric Preset is part of this module's public API. */
export function mapGraphicProfileToParametricPreset(graphicProfile: BuiltinEqProfileName): string {
  return GRAPHIC_TO_PARAMETRIC[graphicProfile] ?? 'Manual';
}

const GRAPHIC_TO_PARAMETRIC_SAMPLE_PAIRS = [
  [0, 1],
  [1, 2],
  [3, 4],
  [4, 5],
  [6, 7],
  [7, 8],
  [8, 9],
] as const;

/** Migrate Graphic Gains To Parametric Bands is part of this module's public API. */
export function migrateGraphicGainsToParametricBands(gains: unknown): ParametricEqBand[] | null {
  const normalized = normalizeEqGains(gains);
  if (!normalized) return null;
  return DEFAULT_PARAMETRIC_BANDS.map((band, index) => {
    const [a, b] = GRAPHIC_TO_PARAMETRIC_SAMPLE_PAIRS[index];
    return {
      ...band,
      gainDb: clampDb((normalized[a] + normalized[b]) / 2),
    };
  });
}

/** Migrate Graphic Profile To Parametric Profile is part of this module's public API. */
export function migrateGraphicProfileToParametricProfile(profile: UserEqProfile): ParametricEqProfile | null {
  const name = profile.name.trim();
  if (!name || isBuiltinParametricPresetName(name)) return null;
  const bands = migrateGraphicGainsToParametricBands(profile.gains);
  if (!bands) return null;
  return { name, bands };
}

// ─── Web Audio helpers ────────────────────────────────────────────────────────

/** Apply Graphic Eq To Filters is part of this module's public API. */
export function applyGraphicEqToFilters(filters: BiquadFilterNode[], gains: number[]): void {
  for (let i = 0; i < filters.length; i++) {
    filters[i].gain.value = gains[i] ?? 0;
  }
}

const BAND_TYPE_TO_BIQUAD: Record<ParametricEqBandType, BiquadFilterType> = {
  peaking:    'peaking',
  low_shelf:  'lowshelf',
  high_shelf: 'highshelf',
  highpass:   'highpass',
  lowpass:    'lowpass',
};

/** Apply Parametric Band To Filter is part of this module's public API. */
export function applyParametricBandToFilter(filter: BiquadFilterNode, band: ParametricEqBand): void {
  filter.type = BAND_TYPE_TO_BIQUAD[band.type];
  filter.frequency.value = band.frequencyHz;
  filter.Q.value = band.q;
  filter.gain.value = (band.type === 'highpass' || band.type === 'lowpass') ? 0 : band.gainDb;
}

// Applies 7 parametric bands to a 7-element filter array. Disabled bands are
// set to 0 dB peaking so they remain in the chain but contribute nothing.
/** Apply Parametric Eq To Filters is part of this module's public API. */
export function applyParametricEqToFilters(
  filters: BiquadFilterNode[],
  bands: ParametricEqBand[],
): void {
  for (let i = 0; i < filters.length && i < bands.length; i++) {
    const band = bands[i];
    if (!band.enabled) {
      filters[i].type = 'peaking';
      filters[i].gain.value = 0;
      filters[i].frequency.value = band.frequencyHz;
      filters[i].Q.value = 1;
    } else {
      applyParametricBandToFilter(filters[i], band);
    }
  }
}
