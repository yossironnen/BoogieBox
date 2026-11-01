/**
 * Loudness levelling maths for the web player (wip/loudness-levelling-plan.md §7): which gain a
 * track gets for the user's mode and pre-amp, clamped so it never clips.
 */

export type LoudnessMode = 'off' | 'track' | 'album' | 'auto';

export const LOUDNESS_MODES: readonly LoudnessMode[] = ['off', 'track', 'album', 'auto'];
export const LOUDNESS_PREAMP_MIN = -6;
export const LOUDNESS_PREAMP_MAX = 6;
/** Positive gain allowed when the peak is unknown. */
export const MAX_GAIN_WITHOUT_PEAK_DB = 6;

/** A user's levelling choice. */
export interface LoudnessPrefs {
  mode: LoudnessMode;
  preampDb: number;
}

export const DEFAULT_LOUDNESS_PREFS: LoudnessPrefs = { mode: 'off', preampDb: 0 };

/** Server values for one track (gains in dB relative to −18 LUFS, peaks linear). */
export interface TrackLoudness {
  trackGainDb: number;
  trackPeak?: number | null;
  albumGainDb?: number | null;
  albumPeak?: number | null;
}

export function parseLoudnessMode(value: string | null | undefined): LoudnessMode {
  return LOUDNESS_MODES.includes(value as LoudnessMode) ? (value as LoudnessMode) : 'off';
}

export function parseLoudnessPreamp(value: string | null | undefined): number {
  const n = Number(value);
  if (value == null || value === '' || !Number.isFinite(n)) return 0;
  return Math.max(LOUDNESS_PREAMP_MIN, Math.min(LOUDNESS_PREAMP_MAX, Math.round(n * 2) / 2));
}

/**
 * Gain in dB for one track. Off, or no data, plays at unity. Album gain is used in album mode, and in
 * auto mode when a neighbouring queue item is from the same album. The result never pushes the
 * peak above full scale; without a peak, positive gain is capped.
 */
export function pickGainDb(
  mode: LoudnessMode,
  preampDb: number,
  data: TrackLoudness | null | undefined,
  albumContext: boolean,
): number {
  if (mode === 'off' || !data || !Number.isFinite(data.trackGainDb)) return 0;
  const useAlbum = (mode === 'album' || (mode === 'auto' && albumContext)) && data.albumGainDb != null;
  const gain = useAlbum ? (data.albumGainDb as number) : data.trackGainDb;
  const peak = useAlbum ? (data.albumPeak ?? data.trackPeak) : data.trackPeak;
  let applied = gain + preampDb;
  applied = peak != null && peak > 0
    ? Math.min(applied, -20 * Math.log10(peak))
    : Math.min(applied, MAX_GAIN_WITHOUT_PEAK_DB);
  return Math.round(applied * 100) / 100;
}

export function dbToLinear(db: number): number {
  return 10 ** (db / 20);
}

interface AlbumKeyed {
  album_id?: string | number | null;
  album?: string | null;
  album_artist?: string | null;
}

/** True when two queue items come from the same album. */
export function sameAlbum(a: AlbumKeyed | null | undefined, b: AlbumKeyed | null | undefined): boolean {
  if (!a || !b) return false;
  if (a.album_id != null && b.album_id != null) return String(a.album_id) === String(b.album_id);
  return !!a.album && a.album === b.album && (a.album_artist ?? '') === (b.album_artist ?? '');
}
