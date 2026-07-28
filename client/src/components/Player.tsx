/**
 * Defines the Player React component and related UI helpers.
 */

import React, { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import type { ClientEntityId, Track, TrackWaveform, CrossfadeConfig, CrossfadeMode, QueueSource, SonicFingerprint } from '../types';
import { DEFAULT_SETTINGS } from '../types';
import { api } from '../api';
import WaveformBar, { type WaveformBarStatus } from './WaveformBar';
import SonicFingerprintPanel from './SonicFingerprintPanel';
import VinylTurntable from './VinylTurntable';
import { playNeedleDrop, preloadVinylFx } from '../audio/VinylFxEngine';
import {
  isBuiltinEqProfileName, parseStoredEqProfiles, parseStoredEqGains,
  type BuiltinEqProfileName,
  type ParametricEqBand, type ParametricEqProfile,
  DEFAULT_PARAMETRIC_BANDS, BUILTIN_PARAMETRIC_PRESETS,
  isBuiltinParametricPresetName, parseStoredParametricBands, parseStoredParametricProfiles,
  applyParametricEqToFilters,
  mapGraphicProfileToParametricPreset, migrateGraphicGainsToParametricBands,
  migrateGraphicProfileToParametricProfile,
} from '../audio/eq';
import ParametricEqEditor from './ParametricEqEditor';
import { useAdaptiveAccentEnabled } from '../hooks/useAdaptiveAccent';
import {
  DESKTOP_PLAYER_DOCK_HEIGHT,
  DESKTOP_PLAYER_POPUP_GAP,
  DESKTOP_VINYL_PLAYER_DOCK_HEIGHT,
  hybridAudioPanelStyles,
  hybridControlStyles,
  hybridPlayerStyles,
} from '../hybridPreview';

/** Player State is part of this module's public API. */
export interface PlayerState {
  queue: Track[];
  currentIndex: number;
  isPlaying: boolean;
  /** Monotonically increasing counter. Incrementing forces the Player to
   *  reload and play the current track even when the track ID hasn't changed
   *  (e.g. clicking Play All on the same album, or retrying after an error). */
  playToken: number;
  queueSource?: QueueSource;
}
/** Playback Snapshot is part of this module's public API. */
export interface PlaybackSnapshot {
  currentTrack: Track | null;
  currentTime: number;
  duration: number;
  isPlaying: boolean;
  volume: number;
  muted: boolean;
  loading: boolean;
  audioError: string | null;
}
/** Player EQ Controls exposes the active audio graph's EQ state to alternate control surfaces. */
export interface PlayerEqControls {
  bands: ParametricEqBand[];
  profile: string;
  customProfiles: ParametricEqProfile[];
  autoEqEnabled: boolean;
  autoEqCurrentPreset: BuiltinEqProfileName;
  onAutoEqEnabledChange: (enabled: boolean) => void;
  onBandsChange: (bands: ParametricEqBand[]) => void;
  onProfileChange: (name: string, bands: ParametricEqBand[]) => void;
  onSaveProfile: (name: string, bands: ParametricEqBand[]) => Promise<string | null>;
  onDeleteProfile: (name: string) => Promise<void>;
}
interface PlayerProps {
  state: PlayerState;
  onStateChange: (s: PlayerState) => void;
  ffmpegAvailable: boolean | null;
  onOpenArtist?: (artistName: string) => void;
  onOpenAlbum?: (albumName: string, artistName?: string | null) => void;
  playbackMode?: 'standard' | 'vinyl';
  vinylHardcore?: boolean;
  vinylNeedleDrop?: boolean;
  vinylAnalogFxDisabled?: boolean;
  vinylNeedleDropIntensity?: number;
  headless?: boolean;
  onPlaybackSnapshotChange?: (snapshot: PlaybackSnapshot) => void;
  onEqControlsChange?: (controls: PlayerEqControls | null) => void;
  hybridPreview?: boolean;
  adaptiveAccentEnabled?: boolean;
}

export function resolveDesktopPlayerDockHeight(isVinylMode: boolean): number {
  return isVinylMode ? DESKTOP_VINYL_PLAYER_DOCK_HEIGHT : DESKTOP_PLAYER_DOCK_HEIGHT;
}

/** Fmt is part of this module's public API. */
export function fmt(s: number): string {
  if (!isFinite(s) || isNaN(s)) return '0:00';
  const m = Math.floor(s / 60); const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, '0')}`;
}

/** Get Track Meta Display is part of this module's public API. */
export function getTrackMetaDisplay(track: Track | null): { artist: string; album: string } {
  const artist = (track?.artist ?? '').trim();
  const album = (track?.album ?? '').trim();
  return { artist, album };
}

/** Truncate Track Title is part of this module's public API. */
export function truncateTrackTitle(title: string, maxChars: number): string {
  if (maxChars <= 0) return '';
  if (title.length <= maxChars) return title;
  if (maxChars <= 3) return '.'.repeat(maxChars);
  return `${title.slice(0, maxChars - 3).trimEnd()}...`;
}

const TRANSCODE_EXTS = new Set(['.flac','.m4a','.aac','.wma','.alac','.ape','.aiff','.aif']);
function getTrackExtension(track: Track | null): string {
  const source = track?.file_name || track?.file_path || '';
  const dot = source.lastIndexOf('.');
  return dot >= 0 ? source.slice(dot).toLowerCase() : '';
}

/** Get Transcode Warning is part of this module's public API. */
export function getTranscodeWarning(track: Track | null, ffmpegAvailable: boolean | null) {
  const ext = getTrackExtension(track);
  if (!ext) return null;
  if (!TRANSCODE_EXTS.has(ext)) return null;
  if (ffmpegAvailable === false) return `${ext.slice(1).toUpperCase()} requires ffmpeg (not installed)`;
  return null;
}

/** Get Transcode Fallback Url is part of this module's public API. */
export function getTranscodeFallbackUrl(streamUrl: string): string | null {
  if (!streamUrl) return null;
  try {
    const baseHref = typeof window !== 'undefined' ? window.location.href : 'http://localhost';
    const url = new URL(streamUrl, baseHref);
    if (url.searchParams.get('noTranscode') !== '1') return null;
    url.searchParams.delete('noTranscode');
    const isAbsolute = /^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(streamUrl) || streamUrl.startsWith('//');
    if (isAbsolute) return url.toString();
    return `${url.pathname}${url.search}${url.hash}`;
  } catch {
    return null;
  }
}

/** Build Playback Debug Info is part of this module's public API. */
export function buildPlaybackDebugInfo(track: Track | null, url: string): {
  trackId: ClientEntityId | null;
  title: string;
  ext: string;
  url: string;
} {
  const ext = getTrackExtension(track);
  return {
    trackId: track?.id ?? null,
    title: track?.title || track?.file_name || '',
    ext,
    url,
  };
}

/** Get Preferred Track Stream Url is part of this module's public API. */
export function getPreferredTrackStreamUrl(track: Track): string {
  const url = api.trackStreamUrl(track.id);
  const ext = getTrackExtension(track);
  const fallbackUrl = getTranscodeFallbackUrl(url);
  const preferredUrl = TRANSCODE_EXTS.has(ext) ? (fallbackUrl ?? url) : url;
  if (preferredUrl !== url) {
    console.info('[Player] forcing transcode stream for browser-risk audio', {
      trackId: track.id,
      ext,
      requestedUrl: url,
      preferredUrl,
    });
  }
  return preferredUrl;
}

// ─── Crossfade helpers (exported for testing) ────────────────────────────────

/** Compute how many seconds before end of track to trigger the transition. */
export function computeTransitionThreshold(mode: CrossfadeMode, crossfadeDuration: number): number {
  if (mode === 'crossfade') return crossfadeDuration;
  if (mode === 'zerogap') return 0.3;
  return 0; // 'off'
}

/** Clamp crossfade duration so it never exceeds half the track length. */
export function clampCrossfadeDuration(requestedDuration: number, trackDuration: number): number {
  if (trackDuration <= 0) return 0;
  const maxAllowed = Math.floor(trackDuration / 2);
  return Math.min(requestedDuration, Math.max(maxAllowed, 1));
}

/** Compute volume at a given progress (0→1) during crossfade ramp. */
export function crossfadeVolumeAt(progress: number, targetVolume: number, direction: 'in' | 'out'): number {
  const p = Math.max(0, Math.min(1, progress));
  if (direction === 'out') return targetVolume * (1 - p);
  return targetVolume * p;
}

function toComparablePlaybackUrl(url: string): string {
  if (!url) return '';
  try {
    const baseHref = typeof window !== 'undefined' ? window.location.href : 'http://localhost';
    const parsed = new URL(url, baseHref);
    return `${parsed.pathname}${parsed.search}${parsed.hash}`;
  } catch {
    return url;
  }
}

/** Should Preserve Transition Playback is part of this module's public API. */
export function shouldPreserveTransitionPlayback({
  handoffInProgress,
  expectedUrl,
  loadedUrl,
  currentAudioSrc,
}: {
  handoffInProgress: boolean;
  expectedUrl: string;
  loadedUrl: string;
  currentAudioSrc: string;
}): boolean {
  if (!handoffInProgress) return false;
  const expected = toComparablePlaybackUrl(expectedUrl);
  if (!expected) return false;
  const loaded = toComparablePlaybackUrl(loadedUrl);
  const current = toComparablePlaybackUrl(currentAudioSrc);
  return loaded === expected || current === expected;
}

const PlayIcon   = () => <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><polygon points="5,3 19,12 5,21"/></svg>;
const PauseIcon  = () => <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>;
const PrevIcon   = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><polygon points="19,20 9,12 19,4"/><rect x="5" y="4" width="3" height="16"/></svg>;
const NextIcon   = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><polygon points="5,4 15,12 5,20"/><rect x="16" y="4" width="3" height="16"/></svg>;
const VolumeIcon = ({ muted }: { muted: boolean }) => muted
  ? <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/></svg>
  : <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/></svg>;
const EqIcon = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="2" y1="14" x2="6" y2="14"/><line x1="10" y1="12" x2="14" y2="12"/><line x1="18" y1="16" x2="22" y2="16"/></svg>;
const QueueIcon  = () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>;
const CloseIcon  = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>;
const ShuffleIcon   = () => <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="16 3 21 3 21 8"/><line x1="4" y1="20" x2="21" y2="3"/><polyline points="21 16 21 21 16 21"/><line x1="15" y1="15" x2="21" y2="21"/><line x1="4" y1="4" x2="9" y2="9"/></svg>;
const RepeatOneIcon = () => <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/><text x="10.5" y="14.5" fontSize="6" fontWeight="bold" fill="currentColor" stroke="none">1</text></svg>;
const RepeatAllIcon = () => <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/></svg>;
const LyricsIcon = () => <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 5h16"/><path d="M4 10h16"/><path d="M4 15h10"/><path d="M4 20h8"/></svg>;

/** PLAYER THEME TOKENS is part of this module's public API. */
export const PLAYER_THEME_TOKENS = {
  bg: 'var(--bg)',
  surface: 'var(--surface)',
  border: 'var(--border)',
  accent: 'var(--accent)',
  text: 'var(--text)',
  textMuted: 'var(--text-muted)',
  font: 'var(--font), monospace',
} as const;

/** PLAYER LAYOUT is part of this module's public API. */
export const PLAYER_LAYOUT = {
  trackTitleMaxChars: 35,
  trackInfoMinWidth: 180,
  trackInfoMaxWidth: 360,
  progressWidth: '36vw',
  progressMaxWidth: 460,
  progressMinWidth: 180,
} as const;

const WAVEFORM_POLL_INTERVAL_MS = 1400;
const WAVEFORM_POLL_MAX_ATTEMPTS = 24;
const USER_EQ_SETTINGS_KEY = 'eqProfiles';
const USER_AUTO_EQ_ENABLED_SETTINGS_KEY = 'autoEqEnabled';
const USER_EQ_SELECTED_PROFILE_SETTINGS_KEY = 'eqSelectedProfile';
const USER_EQ_GAINS_SETTINGS_KEY = 'eqGains';
const USER_VOLUME_SETTINGS_KEY = 'volume';
const USER_MUTED_SETTINGS_KEY = 'muted';
const USER_EQ_MODE_SETTINGS_KEY = 'eqMode';
const USER_PARAMETRIC_EQ_BANDS_SETTINGS_KEY = 'parametricEqBands';
const USER_PARAMETRIC_EQ_PROFILES_SETTINGS_KEY = 'parametricEqProfiles';
const USER_PARAMETRIC_EQ_SELECTED_PROFILE_SETTINGS_KEY = 'parametricEqSelectedProfile';

function parseStoredVolume(raw: string | undefined): number | null {
  if (raw == null) return null;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed)) return null;
  return Math.max(0, Math.min(1, parsed));
}

interface NeedleMeterThemeVars {
  bg: string;
  surface: string;
  border: string;
  accent: string;
  text: string;
  textMuted: string;
}

/** Needle Meter Palette is part of this module's public API. */
export interface NeedleMeterPalette {
  plateTop: string;
  plateMid: string;
  plateBottom: string;
  bezel: string;
  arcGreenStart: string;
  arcGreenEnd: string;
  arcYellowStart: string;
  arcYellowEnd: string;
  arcRedStart: string;
  arcRedEnd: string;
  arcBorder: string;
  tickLow: string;
  tickWarn: string;
  tickHot: string;
  labelNormal: string;
  labelHot: string;
  vuLabel: string;
  channelLabel: string;
  needleShadow: string;
  needleStart: string;
  needleMid: string;
  needleEnd: string;
  pivotTop: string;
  pivotMid: string;
  pivotBottom: string;
  pivotStroke: string;
}

/** Hifi Meter Palette is part of this module's public API. */
export interface HifiMeterPalette {
  frameTop: string;
  frameBottom: string;
  frameStroke: string;
  glassTop: string;
  glassBottom: string;
  glassGlow: string;
  dialTop: string;
  dialCenter: string;
  dialBottom: string;
  dialEdge: string;
  scaleLine: string;
  tickMajor: string;
  tickMinor: string;
  scaleText: string;
  unitText: string;
  channelText: string;
  needleShadow: string;
  needleCore: string;
  needleHighlight: string;
  pivotOuter: string;
  pivotInner: string;
  pivotStroke: string;
}

const DEFAULT_NEEDLE_THEME_VARS: NeedleMeterThemeVars = {
  bg: DEFAULT_SETTINGS.colorBg,
  surface: DEFAULT_SETTINGS.colorSurface,
  border: DEFAULT_SETTINGS.colorBorder,
  accent: DEFAULT_SETTINGS.colorAccent,
  text: DEFAULT_SETTINGS.colorText,
  textMuted: DEFAULT_SETTINGS.colorTextMuted,
};

/** DARK DEFAULT NEEDLE PALETTE is part of this module's public API. */
export const DARK_DEFAULT_NEEDLE_PALETTE: NeedleMeterPalette = {
  plateTop: '#1c1a14',
  plateMid: '#16140e',
  plateBottom: '#0e0c08',
  bezel: '#3a3020',
  arcGreenStart: 'rgba(22,163,74,0.25)',
  arcGreenEnd: 'rgba(34,197,94,0.55)',
  arcYellowStart: 'rgba(180,130,0,0.35)',
  arcYellowEnd: 'rgba(234,179,8,0.70)',
  arcRedStart: 'rgba(180,30,30,0.45)',
  arcRedEnd: 'rgba(239,68,68,0.85)',
  arcBorder: 'rgba(255,255,255,0.07)',
  tickLow: 'rgba(200,190,160,0.55)',
  tickWarn: 'rgba(234,190,8,0.75)',
  tickHot: 'rgba(239,100,100,0.85)',
  labelNormal: 'rgba(210,200,170,0.70)',
  labelHot: 'rgba(255,120,120,0.90)',
  vuLabel: 'rgba(255,220,100,0.55)',
  channelLabel: 'rgba(255,255,255,0.28)',
  needleShadow: 'rgba(0,0,0,0.6)',
  needleStart: '#c0b060',
  needleMid: '#e8d080',
  needleEnd: '#ff4444',
  pivotTop: '#e0c060',
  pivotMid: '#a08020',
  pivotBottom: '#302010',
  pivotStroke: '#604010',
};

/** DARK DEFAULT HIFI PALETTE is part of this module's public API. */
export const DARK_DEFAULT_HIFI_PALETTE: HifiMeterPalette = {
  frameTop: '#090a0c',
  frameBottom: '#020203',
  frameStroke: '#2b2f33',
  glassTop: 'rgba(255,255,255,0.06)',
  glassBottom: 'rgba(0,0,0,0.15)',
  glassGlow: 'rgba(11,180,255,0.16)',
  dialTop: '#0baeff',
  dialCenter: '#03a3f3',
  dialBottom: '#077fca',
  dialEdge: '#023a68',
  scaleLine: 'rgba(6,33,62,0.88)',
  tickMajor: 'rgba(4,25,50,0.92)',
  tickMinor: 'rgba(6,36,66,0.74)',
  scaleText: 'rgba(3,24,46,0.92)',
  unitText: 'rgba(7,41,72,0.80)',
  channelText: 'rgba(14,64,106,0.78)',
  needleShadow: 'rgba(0,0,0,0.48)',
  needleCore: '#0a1118',
  needleHighlight: '#9ecff0',
  pivotOuter: '#0e1116',
  pivotInner: '#9bb6cc',
  pivotStroke: '#d7ecff',
};

interface Rgb {
  r: number;
  g: number;
  b: number;
}

const WARM_YELLOW: Rgb = { r: 234, g: 179, b: 8 };
const WARNING_RED: Rgb = { r: 239, g: 68, b: 68 };
const BLACK: Rgb = { r: 0, g: 0, b: 0 };
const ELECTRIC_BLUE: Rgb = { r: 3, g: 163, b: 243 };

function normalizeThemeValue(v: string): string {
  return v.trim().toLowerCase();
}

function isDarkDefaultTheme(vars: NeedleMeterThemeVars): boolean {
  return normalizeThemeValue(vars.bg) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.bg)
    && normalizeThemeValue(vars.surface) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.surface)
    && normalizeThemeValue(vars.border) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.border)
    && normalizeThemeValue(vars.accent) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.accent)
    && normalizeThemeValue(vars.text) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.text)
    && normalizeThemeValue(vars.textMuted) === normalizeThemeValue(DEFAULT_NEEDLE_THEME_VARS.textMuted);
}

function clampByte(v: number): number {
  return Math.max(0, Math.min(255, Math.round(v)));
}

function parseColorToRgb(raw: string): Rgb | null {
  const value = raw.trim().toLowerCase();
  if (!value) return null;
  if (value.startsWith('#')) {
    const hex = value.slice(1);
    if (/^[0-9a-f]{3}$/.test(hex)) {
      return {
        r: parseInt(hex[0] + hex[0], 16),
        g: parseInt(hex[1] + hex[1], 16),
        b: parseInt(hex[2] + hex[2], 16),
      };
    }
    if (/^[0-9a-f]{6}$/.test(hex)) {
      return {
        r: parseInt(hex.slice(0, 2), 16),
        g: parseInt(hex.slice(2, 4), 16),
        b: parseInt(hex.slice(4, 6), 16),
      };
    }
    return null;
  }
  const rgb = value.match(/^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})(?:\s*,\s*[0-9.]+\s*)?\)$/);
  if (!rgb) return null;
  return {
    r: clampByte(Number(rgb[1])),
    g: clampByte(Number(rgb[2])),
    b: clampByte(Number(rgb[3])),
  };
}

function mixRgb(from: Rgb, to: Rgb, amount: number): Rgb {
  const t = Math.max(0, Math.min(1, amount));
  return {
    r: clampByte(from.r + (to.r - from.r) * t),
    g: clampByte(from.g + (to.g - from.g) * t),
    b: clampByte(from.b + (to.b - from.b) * t),
  };
}

function rgbString(rgb: Rgb): string {
  return `rgb(${rgb.r},${rgb.g},${rgb.b})`;
}

function rgbaString(rgb: Rgb, alpha: number): string {
  const a = Math.max(0, Math.min(1, alpha));
  return `rgba(${rgb.r},${rgb.g},${rgb.b},${a.toFixed(2)})`;
}

function getThemeRgb(value: string, fallbackHex: string): Rgb {
  return parseColorToRgb(value) ?? parseColorToRgb(fallbackHex)!;
}

/** Viz Mode is part of this module's public API. */
export type VizMode = 'bars' | 'needle' | 'hifi' | 'wave';

/** Wave style sub-mode for the wave visualizer. */
export type WaveStyle = 'freq' | 'mirror' | 'scope' | 'fill';

const WAVE_STYLE_KEY = 'bb_wave_style';

function normalizeWaveStyle(v: string | null | undefined): WaveStyle {
  if (v === 'freq' || v === 'mirror' || v === 'scope' || v === 'fill') return v;
  return 'freq';
}

function getNextWaveStyle(s: WaveStyle): WaveStyle {
  if (s === 'freq')   return 'mirror';
  if (s === 'mirror') return 'scope';
  if (s === 'scope')  return 'fill';
  return 'freq';
}

function waveStyleLabel(s: WaveStyle): string {
  if (s === 'freq')   return 'FREQ';
  if (s === 'mirror') return 'MIR';
  if (s === 'scope')  return 'SCP';
  return 'FILL';
}

/** Normalize Viz Mode is part of this module's public API. */
export function normalizeVizMode(value: string | null | undefined): VizMode {
  if (value === 'needle' || value === 'hifi' || value === 'wave' || value === 'bars') return value;
  return 'bars';
}

/** Get Next Viz Mode is part of this module's public API. */
export function getNextVizMode(mode: VizMode): VizMode {
  if (mode === 'bars') return 'needle';
  if (mode === 'needle') return 'hifi';
  if (mode === 'hifi') return 'wave';
  return 'bars';
}

/** Get Viz Mode Toggle Title is part of this module's public API. */
export function getVizModeToggleTitle(mode: VizMode): string {
  if (mode === 'bars') return 'Switch to needle meter';
  if (mode === 'needle') return 'Switch to HiFi meter';
  if (mode === 'hifi') return 'Switch to visualizer';
  return 'Switch to bar meter';
}

/** Resolve Needle Meter Palette is part of this module's public API. */
export function resolveNeedleMeterPalette(vars: NeedleMeterThemeVars): NeedleMeterPalette {
  if (isDarkDefaultTheme(vars)) {
    return { ...DARK_DEFAULT_NEEDLE_PALETTE };
  }

  const bg = getThemeRgb(vars.bg, DEFAULT_NEEDLE_THEME_VARS.bg);
  const surface = getThemeRgb(vars.surface, DEFAULT_NEEDLE_THEME_VARS.surface);
  const border = getThemeRgb(vars.border, DEFAULT_NEEDLE_THEME_VARS.border);
  const accent = getThemeRgb(vars.accent, DEFAULT_NEEDLE_THEME_VARS.accent);
  const text = getThemeRgb(vars.text, DEFAULT_NEEDLE_THEME_VARS.text);
  const textMuted = getThemeRgb(vars.textMuted, DEFAULT_NEEDLE_THEME_VARS.textMuted);

  return {
    plateTop: rgbString(mixRgb(surface, text, 0.06)),
    plateMid: rgbString(mixRgb(surface, bg, 0.42)),
    plateBottom: rgbString(mixRgb(bg, BLACK, 0.12)),
    bezel: rgbString(mixRgb(border, textMuted, 0.18)),
    arcGreenStart: DARK_DEFAULT_NEEDLE_PALETTE.arcGreenStart,
    arcGreenEnd: DARK_DEFAULT_NEEDLE_PALETTE.arcGreenEnd,
    arcYellowStart: DARK_DEFAULT_NEEDLE_PALETTE.arcYellowStart,
    arcYellowEnd: DARK_DEFAULT_NEEDLE_PALETTE.arcYellowEnd,
    arcRedStart: DARK_DEFAULT_NEEDLE_PALETTE.arcRedStart,
    arcRedEnd: DARK_DEFAULT_NEEDLE_PALETTE.arcRedEnd,
    arcBorder: rgbaString(mixRgb(border, text, 0.30), 0.22),
    tickLow: rgbaString(mixRgb(textMuted, text, 0.15), 0.72),
    tickWarn: rgbaString(mixRgb(WARM_YELLOW, text, 0.22), 0.82),
    tickHot: rgbaString(mixRgb(WARNING_RED, text, 0.12), 0.90),
    labelNormal: rgbaString(mixRgb(textMuted, text, 0.30), 0.90),
    labelHot: rgbaString(mixRgb(WARNING_RED, text, 0.15), 0.94),
    vuLabel: rgbaString(mixRgb(accent, text, 0.35), 0.78),
    channelLabel: rgbaString(mixRgb(textMuted, text, 0.20), 0.72),
    needleShadow: 'rgba(0,0,0,0.45)',
    needleStart: rgbString(mixRgb(accent, border, 0.36)),
    needleMid: rgbString(mixRgb(accent, text, 0.24)),
    needleEnd: rgbString(mixRgb(WARNING_RED, text, 0.08)),
    pivotTop: rgbString(mixRgb(accent, text, 0.34)),
    pivotMid: rgbString(mixRgb(accent, border, 0.50)),
    pivotBottom: rgbString(mixRgb(border, bg, 0.56)),
    pivotStroke: rgbString(mixRgb(border, textMuted, 0.28)),
  };
}

/** Resolve Hifi Meter Palette is part of this module's public API. */
export function resolveHifiMeterPalette(vars: NeedleMeterThemeVars): HifiMeterPalette {
  if (isDarkDefaultTheme(vars)) {
    return { ...DARK_DEFAULT_HIFI_PALETTE };
  }

  const bg = getThemeRgb(vars.bg, DEFAULT_NEEDLE_THEME_VARS.bg);
  const surface = getThemeRgb(vars.surface, DEFAULT_NEEDLE_THEME_VARS.surface);
  const border = getThemeRgb(vars.border, DEFAULT_NEEDLE_THEME_VARS.border);
  const accent = getThemeRgb(vars.accent, DEFAULT_NEEDLE_THEME_VARS.accent);
  const text = getThemeRgb(vars.text, DEFAULT_NEEDLE_THEME_VARS.text);
  const textMuted = getThemeRgb(vars.textMuted, DEFAULT_NEEDLE_THEME_VARS.textMuted);
  const blueBase = mixRgb(ELECTRIC_BLUE, accent, 0.28);

  return {
    frameTop: rgbString(mixRgb(surface, bg, 0.56)),
    frameBottom: rgbString(mixRgb(bg, BLACK, 0.22)),
    frameStroke: rgbString(mixRgb(border, textMuted, 0.22)),
    glassTop: rgbaString(text, 0.08),
    glassBottom: rgbaString(bg, 0.24),
    glassGlow: rgbaString(blueBase, 0.16),
    dialTop: rgbString(mixRgb(blueBase, text, 0.12)),
    dialCenter: rgbString(blueBase),
    dialBottom: rgbString(mixRgb(blueBase, bg, 0.24)),
    dialEdge: rgbString(mixRgb(blueBase, border, 0.54)),
    scaleLine: rgbaString(mixRgb(bg, blueBase, 0.35), 0.88),
    tickMajor: rgbaString(mixRgb(bg, blueBase, 0.32), 0.92),
    tickMinor: rgbaString(mixRgb(bg, blueBase, 0.26), 0.74),
    scaleText: rgbaString(mixRgb(bg, text, 0.30), 0.92),
    unitText: rgbaString(mixRgb(bg, textMuted, 0.26), 0.82),
    channelText: rgbaString(mixRgb(bg, textMuted, 0.24), 0.76),
    needleShadow: 'rgba(0,0,0,0.48)',
    needleCore: rgbString(mixRgb(bg, text, 0.08)),
    needleHighlight: rgbString(mixRgb(text, blueBase, 0.30)),
    pivotOuter: rgbString(mixRgb(bg, border, 0.22)),
    pivotInner: rgbString(mixRgb(text, blueBase, 0.20)),
    pivotStroke: rgbString(mixRgb(text, blueBase, 0.10)),
  };
}

function readNeedleThemeVarsFromCss(): NeedleMeterThemeVars {
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return { ...DEFAULT_NEEDLE_THEME_VARS };
  }
  const styles = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) => {
    const value = styles.getPropertyValue(name).trim();
    return value || fallback;
  };
  return {
    bg: read('--bg', DEFAULT_NEEDLE_THEME_VARS.bg),
    surface: read('--surface', DEFAULT_NEEDLE_THEME_VARS.surface),
    border: read('--border', DEFAULT_NEEDLE_THEME_VARS.border),
    accent: read('--accent', DEFAULT_NEEDLE_THEME_VARS.accent),
    text: read('--text', DEFAULT_NEEDLE_THEME_VARS.text),
    textMuted: read('--text-muted', DEFAULT_NEEDLE_THEME_VARS.textMuted),
  };
}

// ─────────────────────────────────────────────────────────────────────────────
// MODE 1 — Horizontal bar-graph VU meter
// ─────────────────────────────────────────────────────────────────────────────

function drawBarMeter(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  level: number, peak: number, label: string,
) {
  ctx.clearRect(0, 0, w, h);
  const padL = 5, padR = 5, padT = 5, padB = 16;
  const mw = w - padL - padR, mh = h - padT - padB;

  const bg = ctx.createLinearGradient(padL, padT, padL, padT + mh);
  bg.addColorStop(0, '#1e1e1e'); bg.addColorStop(1, '#0f0f0f');
  ctx.fillStyle = bg;
  beginRoundedRect(ctx, padL, padT, mw, mh, 3); ctx.fill();
  ctx.strokeStyle = '#2a2a2a'; ctx.lineWidth = 1;
  beginRoundedRect(ctx, padL, padT, mw, mh, 3); ctx.stroke();

  const DB_MIN = -24, DB_MAX = 3;
  const dbToX = (db: number) =>
    padL + 2 + Math.max(0, Math.min(1, (db - DB_MIN) / (DB_MAX - DB_MIN))) * (mw - 4);

  // Zone fills
  const gG = ctx.createLinearGradient(dbToX(-24), 0, dbToX(-6), 0);
  gG.addColorStop(0, 'rgba(34,197,94,0.08)'); gG.addColorStop(1, 'rgba(34,197,94,0.16)');
  ctx.fillStyle = gG; ctx.fillRect(dbToX(-24), padT+2, dbToX(-6)-dbToX(-24), mh-4);

  const yG = ctx.createLinearGradient(dbToX(-6), 0, dbToX(0), 0);
  yG.addColorStop(0, 'rgba(234,179,8,0.10)'); yG.addColorStop(1, 'rgba(234,179,8,0.22)');
  ctx.fillStyle = yG; ctx.fillRect(dbToX(-6), padT+2, dbToX(0)-dbToX(-6), mh-4);

  const rG = ctx.createLinearGradient(dbToX(0), 0, dbToX(3), 0);
  rG.addColorStop(0, 'rgba(239,68,68,0.16)'); rG.addColorStop(1, 'rgba(239,68,68,0.32)');
  ctx.fillStyle = rG; ctx.fillRect(dbToX(0), padT+2, dbToX(3)-dbToX(0), mh-4);

  ctx.strokeStyle = 'rgba(255,255,255,0.05)'; ctx.lineWidth = 1;
  [dbToX(-6), dbToX(0)].forEach(x => {
    ctx.beginPath(); ctx.moveTo(x, padT+2); ctx.lineTo(x, padT+mh-2); ctx.stroke();
  });

  // Ticks
  const marks = [{db:-24,m:true},{db:-18,m:true},{db:-12,m:true},{db:-9,m:false},{db:-6,m:true},{db:-3,m:false},{db:0,m:true},{db:3,m:true}];
  const tZH = mh * 0.42;
  marks.forEach(({db, m}) => {
    const x = dbToX(db);
    ctx.strokeStyle = db>=0 ? 'rgba(239,80,80,0.55)' : db>=-6 ? 'rgba(234,200,8,0.45)' : 'rgba(255,255,255,0.18)';
    ctx.lineWidth = m ? 1.5 : 0.8;
    ctx.beginPath(); ctx.moveTo(x, padT+2); ctx.lineTo(x, padT+2+(m?tZH:tZH*0.5)); ctx.stroke();
    if (m) {
      ctx.fillStyle = db>=0 ? 'rgba(255,120,120,0.8)' : 'rgba(255,255,255,0.28)';
      ctx.font = `${db>=0?'bold ':''}6.5px monospace`; ctx.textAlign = 'center';
      ctx.fillText(db>=0?`+${db}`:`${db}`, x, padT+mh-3);
    }
  });

  // Active bar
  const lDb = level > 0 ? Math.max(DB_MIN, 20*Math.log10(level)) : DB_MIN;
  const barRight = Math.min(dbToX(DB_MAX), Math.max(dbToX(DB_MIN), dbToX(lDb)));
  const barY = padT + mh*0.16, barH = mh*0.52;

  if (barRight > dbToX(DB_MIN)+1) {
    const bG = ctx.createLinearGradient(dbToX(DB_MIN), 0, barRight, 0);
    if (lDb < -6) { bG.addColorStop(0,'#16a34a'); bG.addColorStop(0.7,'#22c55e'); bG.addColorStop(1,'#4ade80'); }
    else if (lDb < 0) { bG.addColorStop(0,'#16a34a'); bG.addColorStop(0.45,'#22c55e'); bG.addColorStop(0.75,'#eab308'); bG.addColorStop(1,'#facc15'); }
    else { bG.addColorStop(0,'#16a34a'); bG.addColorStop(0.4,'#22c55e'); bG.addColorStop(0.65,'#eab308'); bG.addColorStop(0.82,'#facc15'); bG.addColorStop(1,'#ef4444'); }
    ctx.fillStyle = bG; ctx.fillRect(dbToX(DB_MIN)+1, barY, barRight-dbToX(DB_MIN)-1, barH);
    const sh = ctx.createLinearGradient(0, barY, 0, barY+barH);
    sh.addColorStop(0,'rgba(255,255,255,0.20)'); sh.addColorStop(0.35,'rgba(255,255,255,0.06)'); sh.addColorStop(1,'rgba(0,0,0,0.08)');
    ctx.fillStyle = sh; ctx.fillRect(dbToX(DB_MIN)+1, barY, barRight-dbToX(DB_MIN)-1, barH);
  }

  // Peak
  const pDb = peak > 0 ? Math.max(DB_MIN, 20*Math.log10(peak)) : DB_MIN;
  const pX = Math.min(dbToX(DB_MAX), Math.max(dbToX(DB_MIN), dbToX(pDb)));
  if (pX > dbToX(DB_MIN)+2) {
    const pc = pDb>=0 ? '#ef4444' : pDb>=-6 ? '#facc15' : '#86efac';
    ctx.shadowColor = pc; ctx.shadowBlur = 5; ctx.fillStyle = pc;
    ctx.fillRect(pX-1.5, barY-1, 3, barH+2); ctx.shadowBlur = 0;
  }

  ctx.fillStyle = 'rgba(255,255,255,0.30)'; ctx.font = 'bold 7px monospace';
  ctx.textAlign = 'center'; ctx.fillText(label, w/2, h-4);
}

/** Begin Rounded Rect is part of this module's public API. */
export function beginRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  const rr = Math.max(0, Math.min(radius, Math.min(width, height) / 2));
  const anyCtx = ctx as CanvasRenderingContext2D & {
    roundRect?: (x: number, y: number, width: number, height: number, radius: number) => void;
  };

  ctx.beginPath();
  if (typeof anyCtx.roundRect === 'function') {
    anyCtx.roundRect(x, y, width, height, rr);
    return;
  }

  // Safari versions without CanvasRenderingContext2D.roundRect
  // still support arcTo, so build a rounded rectangle manually.
  ctx.moveTo(x + rr, y);
  ctx.lineTo(x + width - rr, y);
  ctx.arcTo(x + width, y, x + width, y + rr, rr);
  ctx.lineTo(x + width, y + height - rr);
  ctx.arcTo(x + width, y + height, x + width - rr, y + height, rr);
  ctx.lineTo(x + rr, y + height);
  ctx.arcTo(x, y + height, x, y + height - rr, rr);
  ctx.lineTo(x, y + rr);
  ctx.arcTo(x, y, x + rr, y, rr);
  ctx.closePath();
}

/** Get Synthetic Vu Level is part of this module's public API. */
export function getSyntheticVuLevel(
  isPlaying: boolean,
  analyserPresent: boolean,
  timeSeconds: number,
  channel: 'left' | 'right',
): number | null {
  if (!isPlaying || analyserPresent) return null;
  const phase = channel === 'left' ? 0 : Math.PI * 0.35;
  const wobble = (Math.sin(timeSeconds * 2.2 + phase) + 1) * 0.5;
  return 0.04 + wobble * 0.12;
}

/** Should Resume Audio Context is part of this module's public API. */
export function shouldResumeAudioContext(state: AudioContextState | string): boolean {
  return state !== 'running';
}

// ─────────────────────────────────────────────────────────────────────────────
// MODE 2 — Analog needle VU meter (arc dial)
// ─────────────────────────────────────────────────────────────────────────────

function drawNeedleMeter(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  level: number,  // 0–1 smoothed
  label: string,
) {
  ctx.clearRect(0, 0, w, h);
  const palette = resolveNeedleMeterPalette(readNeedleThemeVarsFromCss());

  // Arc sweeps from -145° to -35° (left to right), centred on straight-up
  // In canvas coords: angles measured from +x axis
  const DEG_MIN = 215;   // degrees — far left  (−VU)
  const DEG_MAX = 325;   // degrees — far right (+VU)
  const degToRad = (d: number) => (d * Math.PI) / 180;

  // Pivot sits in the lower portion of the canvas (visible hinge).
  // R is constrained so the arc band stays within the canvas width.
  const cx = w / 2;
  const cy = h * 0.80;
  const R  = Math.min(
    cx / Math.abs(Math.cos(degToRad(DEG_MIN))) * 0.88,   // horizontal fit
    cy * 0.82,                                             // vertical fit
  );

  // Map 0–1 level → angle
  // We use a slightly non-linear mapping to mimic real VU ballistics
  const levelAngle = (lv: number) => {
    const t = Math.pow(Math.max(0, Math.min(1, lv)), 0.85);
    return degToRad(DEG_MIN + t * (DEG_MAX - DEG_MIN));
  };

  // ── Background plate ──
  const bgG = ctx.createLinearGradient(0, 0, 0, h);
  bgG.addColorStop(0, palette.plateTop);
  bgG.addColorStop(0.5, palette.plateMid);
  bgG.addColorStop(1, palette.plateBottom);
  ctx.fillStyle = bgG;
  beginRoundedRect(ctx, 0, 0, w, h, 4); ctx.fill();

  // Outer bezel ring
  ctx.strokeStyle = palette.bezel; ctx.lineWidth = 1.5;
  beginRoundedRect(ctx, 0.75, 0.75, w-1.5, h-1.5, 3.5); ctx.stroke();

  // ── Arc color zones ──
  // We split the arc: green zone, yellow zone, red zone
  const arcThick = Math.min(w, h) * 0.13;         // band thickness
  const arcR = R - arcThick * 0.3;

  // Helper: draw arc sector
  const drawArc = (aStart: number, aEnd: number, color1: string, color2: string, thick: number) => {
    const grad = ctx.createRadialGradient(cx, cy, arcR - thick * 0.5, cx, cy, arcR + thick * 0.4);
    grad.addColorStop(0, color1); grad.addColorStop(1, color2);
    ctx.beginPath();
    ctx.arc(cx, cy, arcR, aStart, aEnd);
    ctx.arc(cx, cy, arcR - thick, aEnd, aStart, true);
    ctx.closePath();
    ctx.fillStyle = grad; ctx.fill();
  };

  // Green: min → 80% of range
  const greenEnd = degToRad(DEG_MIN + 0.76 * (DEG_MAX - DEG_MIN));
  drawArc(degToRad(DEG_MIN), greenEnd, palette.arcGreenStart, palette.arcGreenEnd, arcThick);

  // Yellow: 80–92%
  const yellowEnd = degToRad(DEG_MIN + 0.92 * (DEG_MAX - DEG_MIN));
  drawArc(greenEnd, yellowEnd, palette.arcYellowStart, palette.arcYellowEnd, arcThick);

  // Red: 92–100%
  drawArc(yellowEnd, degToRad(DEG_MAX), palette.arcRedStart, palette.arcRedEnd, arcThick);

  // Arc border lines (inner & outer)
  ctx.strokeStyle = palette.arcBorder; ctx.lineWidth = 0.8;
  ctx.beginPath(); ctx.arc(cx, cy, arcR,              degToRad(DEG_MIN), degToRad(DEG_MAX)); ctx.stroke();
  ctx.beginPath(); ctx.arc(cx, cy, arcR - arcThick,   degToRad(DEG_MIN), degToRad(DEG_MAX)); ctx.stroke();

  // ── Tick marks & scale labels ──
  // VU scale: -20, -10, -7, -5, -3, -2, -1, 0, +1, +2, +3 (non-linear!)
  // Map VU positions to our 0–1 range
  const vuScale = [
    { vu: -20, t: 0.00, major: true  },
    { vu: -10, t: 0.20, major: true  },
    { vu:  -7, t: 0.35, major: false },
    { vu:  -5, t: 0.46, major: true  },
    { vu:  -3, t: 0.57, major: false },
    { vu:  -2, t: 0.63, major: false },
    { vu:  -1, t: 0.70, major: false },
    { vu:   0, t: 0.76, major: true  },
    { vu:   1, t: 0.83, major: false },
    { vu:   2, t: 0.89, major: false },
    { vu:   3, t: 0.96, major: true  },
  ];

  const tickRO = arcR + arcThick * 0.18;   // outer tick radius
  const tickRI_maj = arcR - arcThick * 1.5;
  const tickRI_min = arcR - arcThick * 0.8;
  const labelR = arcR - arcThick * 2.4;

  vuScale.forEach(({ vu, t, major }) => {
    const angle = degToRad(DEG_MIN + t * (DEG_MAX - DEG_MIN));
    const cos = Math.cos(angle), sin = Math.sin(angle);
    const ri = major ? tickRI_maj : tickRI_min;
    const isRed = vu >= 0;
    const isYellow = vu >= -3 && vu < 0;

    ctx.strokeStyle = isRed
      ? palette.tickHot
      : isYellow
      ? palette.tickWarn
      : palette.tickLow;
    ctx.lineWidth = major ? 1.8 : 0.9;
    ctx.beginPath();
    ctx.moveTo(cx + tickRO * cos, cy + tickRO * sin);
    ctx.lineTo(cx + ri * cos,     cy + ri * sin);
    ctx.stroke();

    if (major) {
      const lx = cx + labelR * cos;
      const ly = cy + labelR * sin;
      ctx.fillStyle = isRed ? palette.labelHot : palette.labelNormal;
      ctx.font = `${isRed ? 'bold ' : ''}${major && Math.abs(vu) <= 3 ? 7.5 : 6.5}px monospace`;
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(vu > 0 ? `+${vu}` : `${vu}`, lx, ly);
    }
  });

  ctx.textBaseline = 'alphabetic';

  // ── "VU" label ──
  ctx.fillStyle = palette.vuLabel;
  ctx.font = 'bold 9px monospace';
  ctx.textAlign = 'center';
  ctx.fillText('VU', cx, h * 0.72);

  // ── Channel label (L / R) ──
  ctx.fillStyle = palette.channelLabel;
  ctx.font = 'bold 7px monospace';
  ctx.textAlign = 'center';
  ctx.fillText(label, cx, h - 4);

  // ── Needle ──
  const angle = levelAngle(level);
  const nLen  = arcR - arcThick * 0.15;
  const nTip  = { x: cx + nLen * Math.cos(angle),  y: cy + nLen * Math.sin(angle)  };
  const nBase = { x: cx + (arcThick*0.7) * Math.cos(angle + Math.PI), y: cy + (arcThick*0.7) * Math.sin(angle + Math.PI) };

  // Needle shadow
  ctx.shadowColor = palette.needleShadow; ctx.shadowBlur = 4; ctx.shadowOffsetX = 1; ctx.shadowOffsetY = 1;

  // Needle body
  ctx.beginPath();
  const perpAngle = angle + Math.PI / 2;
  const hw = 1.2; // half-width at base
  ctx.moveTo(nBase.x + hw * Math.cos(perpAngle), nBase.y + hw * Math.sin(perpAngle));
  ctx.lineTo(nTip.x, nTip.y);
  ctx.lineTo(nBase.x - hw * Math.cos(perpAngle), nBase.y - hw * Math.sin(perpAngle));
  ctx.closePath();
  const nGrad = ctx.createLinearGradient(nBase.x, nBase.y, nTip.x, nTip.y);
  nGrad.addColorStop(0, palette.needleStart);
  nGrad.addColorStop(0.3, palette.needleMid);
  nGrad.addColorStop(1, palette.needleEnd);
  ctx.fillStyle = nGrad; ctx.fill();

  ctx.shadowColor = 'transparent'; ctx.shadowBlur = 0; ctx.shadowOffsetX = 0; ctx.shadowOffsetY = 0;

  // Pivot jewel
  const pivotR = 4;
  const pivGrad = ctx.createRadialGradient(cx - pivotR*0.3, cy - pivotR*0.3, 0.5, cx, cy, pivotR);
  pivGrad.addColorStop(0, palette.pivotTop);
  pivGrad.addColorStop(0.5, palette.pivotMid);
  pivGrad.addColorStop(1, palette.pivotBottom);
  ctx.beginPath(); ctx.arc(cx, cy, pivotR, 0, Math.PI*2);
  ctx.fillStyle = pivGrad; ctx.fill();
  ctx.strokeStyle = palette.pivotStroke; ctx.lineWidth = 0.5;
  ctx.stroke();
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared audio meter canvas — reads from analyser, calls draw fn
// ─────────────────────────────────────────────────────────────────────────────

function drawHifiMeter(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  level: number,
  label: string,
) {
  ctx.clearRect(0, 0, w, h);
  const palette = resolveHifiMeterPalette(readNeedleThemeVarsFromCss());

  const frameG = ctx.createLinearGradient(0, 0, 0, h);
  frameG.addColorStop(0, palette.frameTop);
  frameG.addColorStop(1, palette.frameBottom);
  ctx.fillStyle = frameG;
  beginRoundedRect(ctx, 0.5, 0.5, w - 1, h - 1, 4);
  ctx.fill();
  ctx.strokeStyle = palette.frameStroke;
  ctx.lineWidth = 1.1;
  beginRoundedRect(ctx, 0.5, 0.5, w - 1, h - 1, 4);
  ctx.stroke();

  const glass = ctx.createLinearGradient(0, 0, 0, h * 0.44);
  glass.addColorStop(0, palette.glassTop);
  glass.addColorStop(1, palette.glassBottom);
  ctx.fillStyle = glass;
  beginRoundedRect(ctx, 1.5, 1.5, w - 3, h * 0.5, 3.5);
  ctx.fill();

  const dialX = 7;
  const dialY = 8;
  const dialW = w - 14;
  const dialH = h - 24;
  const dialGrad = ctx.createLinearGradient(0, dialY, 0, dialY + dialH);
  dialGrad.addColorStop(0, palette.dialTop);
  dialGrad.addColorStop(0.52, palette.dialCenter);
  dialGrad.addColorStop(1, palette.dialBottom);
  ctx.fillStyle = dialGrad;
  beginRoundedRect(ctx, dialX, dialY, dialW, dialH, 3);
  ctx.fill();

  const glow = ctx.createRadialGradient(w / 2, dialY + dialH * 0.45, 2, w / 2, dialY + dialH * 0.5, dialW * 0.52);
  glow.addColorStop(0, palette.glassGlow);
  glow.addColorStop(1, 'rgba(0,0,0,0)');
  ctx.fillStyle = glow;
  beginRoundedRect(ctx, dialX, dialY, dialW, dialH, 3);
  ctx.fill();
  ctx.strokeStyle = palette.dialEdge;
  ctx.lineWidth = 1;
  beginRoundedRect(ctx, dialX + 0.5, dialY + 0.5, dialW - 1, dialH - 1, 2.5);
  ctx.stroke();

  const curveY = (t: number) => dialY + dialH * 0.63 - Math.sin(t * Math.PI) * dialH * 0.20;
  ctx.strokeStyle = palette.scaleLine;
  ctx.lineWidth = 1.25;
  ctx.beginPath();
  for (let i = 0; i <= 40; i++) {
    const t = i / 40;
    const x = dialX + t * dialW;
    const y = curveY(t);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();

  const ticks = [0.08, 0.18, 0.28, 0.38, 0.50, 0.62, 0.72, 0.82, 0.92];
  ticks.forEach((t, idx) => {
    const x = dialX + t * dialW;
    const y = curveY(t);
    const yL = curveY(Math.max(0, t - 0.01));
    const yR = curveY(Math.min(1, t + 0.01));
    const tangent = Math.atan2(yR - yL, dialW * 0.02);
    const normal = tangent + Math.PI / 2;
    const major = idx % 2 === 0;
    const len = major ? 7.8 : 4.6;
    ctx.strokeStyle = major ? palette.tickMajor : palette.tickMinor;
    ctx.lineWidth = major ? 1 : 0.8;
    ctx.beginPath();
    ctx.moveTo(x, y);
    ctx.lineTo(x + Math.cos(normal) * len, y + Math.sin(normal) * len);
    ctx.stroke();
  });

  const wattLabels = [
    { t: 0.11, text: '.012' },
    { t: 0.28, text: '.12' },
    { t: 0.44, text: '1.2' },
    { t: 0.58, text: '12' },
    { t: 0.73, text: '120' },
    { t: 0.88, text: '1.2kW' },
  ];
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillStyle = palette.scaleText;
  ctx.font = 'bold 5.8px monospace';
  wattLabels.forEach(({ t, text }) => {
    ctx.fillText(text, dialX + t * dialW, curveY(t) - 8.7);
  });

  const dbLabels = [
    { t: 0.12, text: '-50' },
    { t: 0.28, text: '-40' },
    { t: 0.43, text: '-30' },
    { t: 0.58, text: '-20' },
    { t: 0.73, text: '-10' },
    { t: 0.86, text: '0' },
    { t: 0.94, text: '+3' },
  ];
  ctx.font = '5px monospace';
  dbLabels.forEach(({ t, text }) => {
    ctx.fillText(text, dialX + t * dialW, curveY(t) + 6.6);
  });

  ctx.fillStyle = palette.unitText;
  ctx.font = 'bold 4.8px monospace';
  ctx.fillText('WATTS', w / 2, dialY + dialH * 0.14);
  ctx.font = 'bold 4.9px monospace';
  ctx.fillText('DECIBELS', w / 2, dialY + dialH * 0.77);
  ctx.fillText('POWER OUTPUT', w / 2, dialY + dialH * 0.94);

  const DEG_MIN = 205;
  const DEG_MAX = 335;
  const degToRad = (d: number) => (d * Math.PI) / 180;
  const clamped = Math.max(0, Math.min(1, level));
  const ang = degToRad(DEG_MIN + Math.pow(clamped, 0.78) * (DEG_MAX - DEG_MIN));
  const cx = w / 2;
  const cy = dialY + dialH * 0.84;
  const needleLen = Math.min(dialW * 0.44, dialH * 0.76);
  const tipX = cx + Math.cos(ang) * needleLen;
  const tipY = cy + Math.sin(ang) * needleLen;

  ctx.shadowColor = palette.needleShadow;
  ctx.shadowBlur = 3.5;
  ctx.lineWidth = 1.25;
  ctx.strokeStyle = palette.needleCore;
  ctx.beginPath();
  ctx.moveTo(cx - Math.cos(ang) * 4.2, cy - Math.sin(ang) * 4.2);
  ctx.lineTo(tipX, tipY);
  ctx.stroke();
  ctx.shadowBlur = 0;

  ctx.lineWidth = 0.55;
  ctx.strokeStyle = palette.needleHighlight;
  ctx.beginPath();
  ctx.moveTo(cx, cy);
  ctx.lineTo(cx + Math.cos(ang) * (needleLen * 0.96), cy + Math.sin(ang) * (needleLen * 0.96));
  ctx.stroke();

  ctx.beginPath();
  ctx.arc(cx, cy, 3.4, 0, Math.PI * 2);
  ctx.fillStyle = palette.pivotOuter;
  ctx.fill();
  ctx.beginPath();
  ctx.arc(cx, cy, 1.6, 0, Math.PI * 2);
  ctx.fillStyle = palette.pivotInner;
  ctx.fill();
  ctx.strokeStyle = palette.pivotStroke;
  ctx.lineWidth = 0.5;
  ctx.beginPath();
  ctx.arc(cx, cy, 3.4, 0, Math.PI * 2);
  ctx.stroke();

  ctx.fillStyle = palette.channelText;
  ctx.font = 'bold 6px monospace';
  ctx.fillText(label, w / 2, h - 4.2);
}

function MeterCanvas({
  analyser, channel, label, isPlaying, mode, width, height,
}: {
  analyser: AnalyserNode | null;
  channel: 'left' | 'right';
  label: string;
  isPlaying: boolean;
  mode: VizMode;
  width: number;
  height: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef    = useRef<number>(0);
  const peakRef   = useRef(0);
  const peakTtl   = useRef(0);
  const levelRef  = useRef(0);
  const dataArr   = useRef<Uint8Array<ArrayBuffer> | null>(null);

  useEffect(() => {
    if (analyser) dataArr.current = new Uint8Array(analyser.frequencyBinCount);
  }, [analyser]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Needle-style modes share slower attack/decay than bar mode.
    const ATTACK      = mode === 'bars' ? 0.30 : 0.18;
    const DECAY       = mode === 'bars' ? 0.07 : 0.05;
    const PEAK_HOLD   = 55;
    const PEAK_DECAY  = 0.965;

    const draw = () => {
      rafRef.current = requestAnimationFrame(draw);
      let raw = 0;

      if (analyser && dataArr.current && isPlaying) {
        analyser.getByteTimeDomainData(dataArr.current);
        let sum = 0;
        for (let i = 0; i < dataArr.current.length; i++) {
          const v = (dataArr.current[i] - 128) / 128;
          sum += v * v;
        }
        raw = Math.sqrt(sum / dataArr.current.length);
      }

      // Fallback only when analyser is unavailable; otherwise always trust
      // measured audio so motion reflects real program material.
      const synthetic = getSyntheticVuLevel(
        isPlaying,
        Boolean(analyser),
        performance.now() / 1000,
        channel,
      );
      if (synthetic !== null) {
        raw = synthetic;
      }

      const target = isPlaying ? raw : 0;
      const s = target > levelRef.current ? ATTACK : DECAY;
      levelRef.current = levelRef.current * (1 - s) + target * s;

      if (levelRef.current >= peakRef.current) {
        peakRef.current = levelRef.current;
        peakTtl.current = PEAK_HOLD;
      } else {
        if (peakTtl.current > 0) {
          peakTtl.current -= 1;
        } else {
          peakRef.current *= PEAK_DECAY;
        }
      }

      try {
        if (mode === 'bars') {
          drawBarMeter(ctx, canvas.width, canvas.height, levelRef.current, peakRef.current, label);
        } else if (mode === 'needle') {
          drawNeedleMeter(ctx, canvas.width, canvas.height, levelRef.current, label);
        } else {
          drawHifiMeter(ctx, canvas.width, canvas.height, levelRef.current, label);
        }
      } catch (e) {
        console.error('[VU meter draw error]', e);
      }
    };

    draw();
    return () => cancelAnimationFrame(rafRef.current);
  }, [analyser, channel, label, isPlaying, mode]);

  return (
    <canvas
      ref={canvasRef}
      width={width}
      height={height}
      style={{ display: 'block', imageRendering: 'pixelated' }}
    />
  );
}

function WaveVisualizer({
  analyser, isPlaying, style, width, height,
}: {
  analyser: AnalyserNode | null;
  isPlaying: boolean;
  style: WaveStyle;
  width: number;
  height: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef    = useRef<number>(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !analyser) return;
    const ctx2d = canvas.getContext('2d')!;

    // Resolve CSS variables via documentElement — same approach as readNeedleThemeVarsFromCss().
    const cs     = getComputedStyle(document.documentElement);
    const accent = cs.getPropertyValue('--accent').trim() || '#4a9eff';
    const border = cs.getPropertyValue('--border').trim() || 'rgba(255,255,255,0.12)';
    // Derive a semi-transparent fill variant from the resolved accent.
    const accentFill = (() => {
      const tmp = document.createElement('canvas');
      tmp.width = tmp.height = 1;
      const t = tmp.getContext('2d')!;
      t.fillStyle = accent;
      t.fillRect(0, 0, 1, 1);
      const [r, g, b] = t.getImageData(0, 0, 1, 1).data;
      return `rgba(${r},${g},${b},0.55)`;
    })();

    const isFreq = style === 'freq' || style === 'mirror';
    const fftSize = isFreq ? 256 : 1024;
    if (analyser.fftSize !== fftSize) analyser.fftSize = fftSize;
    const bufLen = isFreq ? analyser.frequencyBinCount : analyser.fftSize;
    const buf    = new Uint8Array(bufLen);

    const drawIdle = () => {
      ctx2d.clearRect(0, 0, width, height);
      ctx2d.fillStyle = border;
      ctx2d.fillRect(0, height / 2 - 1, width, 2);
    };

    const drawFreqBars = () => {
      analyser.getByteFrequencyData(buf);
      ctx2d.clearRect(0, 0, width, height);
      const barW = (width / bufLen) * 2.2;
      for (let i = 0, x = 0; i < bufLen && x < width; i++, x += barW) {
        const v    = buf[i] / 255;
        const barH = v * height;
        if (barH < 1) continue;
        const grad = ctx2d.createLinearGradient(0, height - barH, 0, height);
        grad.addColorStop(0, accent);
        grad.addColorStop(1, 'rgba(0,0,0,0.15)');
        ctx2d.fillStyle = grad;
        ctx2d.fillRect(x, height - barH, Math.max(1, barW - 1), barH);
      }
    };

    const drawMirrorBars = () => {
      analyser.getByteFrequencyData(buf);
      ctx2d.clearRect(0, 0, width, height);
      const mid  = height / 2;
      const barW = (width / bufLen) * 2.2;
      for (let i = 0, x = 0; i < bufLen && x < width; i++, x += barW) {
        const v    = buf[i] / 255;
        const half = v * mid;
        if (half < 1) continue;
        const grad = ctx2d.createLinearGradient(0, mid - half, 0, mid + half);
        grad.addColorStop(0,   'rgba(0,0,0,0.1)');
        grad.addColorStop(0.5, accent);
        grad.addColorStop(1,   'rgba(0,0,0,0.1)');
        ctx2d.fillStyle = grad;
        ctx2d.fillRect(x, mid - half, Math.max(1, barW - 1), half * 2);
      }
    };

    const drawScope = () => {
      analyser.getByteTimeDomainData(buf);
      ctx2d.clearRect(0, 0, width, height);
      ctx2d.strokeStyle = accent;
      ctx2d.lineWidth   = 1.5;
      ctx2d.beginPath();
      const sliceW = width / bufLen;
      for (let i = 0; i < bufLen; i++) {
        const y = (buf[i] / 128.0) * (height / 2);
        if (i === 0) ctx2d.moveTo(0, y);
        else         ctx2d.lineTo(i * sliceW, y);
      }
      ctx2d.stroke();
    };

    const drawFill = () => {
      analyser.getByteTimeDomainData(buf);
      ctx2d.clearRect(0, 0, width, height);
      const sliceW = width / bufLen;

      // filled area
      const areaGrad = ctx2d.createLinearGradient(0, 0, 0, height);
      areaGrad.addColorStop(0, accentFill);
      areaGrad.addColorStop(1, 'rgba(0,0,0,0)');
      ctx2d.fillStyle = areaGrad;
      ctx2d.beginPath();
      ctx2d.moveTo(0, height);
      for (let i = 0; i < bufLen; i++) {
        ctx2d.lineTo(i * sliceW, (buf[i] / 128.0) * (height / 2));
      }
      ctx2d.lineTo(width, height);
      ctx2d.closePath();
      ctx2d.fill();

      // outline on top
      ctx2d.strokeStyle = accent;
      ctx2d.lineWidth   = 1.5;
      ctx2d.beginPath();
      for (let i = 0; i < bufLen; i++) {
        const y = (buf[i] / 128.0) * (height / 2);
        if (i === 0) ctx2d.moveTo(0, y);
        else         ctx2d.lineTo(i * sliceW, y);
      }
      ctx2d.stroke();
    };

    const drawMap: Record<WaveStyle, () => void> = {
      freq: drawFreqBars, mirror: drawMirrorBars, scope: drawScope, fill: drawFill,
    };

    const loop = () => {
      rafRef.current = requestAnimationFrame(loop);
      drawMap[style]();
    };

    if (isPlaying) loop(); else drawIdle();
    return () => cancelAnimationFrame(rafRef.current);
  }, [analyser, isPlaying, style, width, height]);

  return <canvas ref={canvasRef} width={width} height={height} style={{ display: 'block' }} />;
}

// ─────────────────────────────────────────────────────────────────────────────
// StereoVU — manages Web Audio graph, renders both channels + mode toggle
// ─────────────────────────────────────────────────────────────────────────────

function StereoVU({
  analyser, isPlaying, mode, onToggleMode,
}: {
  analyser: AnalyserNode | null;
  isPlaying: boolean;
  mode: VizMode;
  onToggleMode: () => void;
}) {
  // The analyser is created and owned by Player (tapped off the same
  // MediaElementAudioSourceNode the parametric EQ chain uses for the active
  // slot) and swapped in here as it changes. A <audio> element may only ever
  // be associated with ONE MediaElementAudioSourceNode for its lifetime, so
  // this component must not create its own — doing so throws on the second
  // caller and previously left the meter stuck on its synthetic fallback
  // animation whenever the EQ chain's setup won that race.
  const analyserL = analyser;
  const analyserR = analyser;

  // TEMP DIAGNOSTIC — remove once the iPad/Safari VU meter fix is confirmed.
  // Enable with ?vudebug=1 in the URL; no effect otherwise on any platform.
  const vuDebugOn = typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('vudebug') === '1';

  // Merged mono analyser for the wave visualizer
  const mergedAnalyserRef = useRef<AnalyserNode | null>(null);
  useEffect(() => {
    if (!analyserL || !analyserR) return;
    const audioCtx = analyserL.context;
    const merger   = audioCtx.createChannelMerger(2);
    analyserL.connect(merger, 0, 0);
    analyserR.connect(merger, 0, 1);
    const merged = audioCtx.createAnalyser();
    merged.fftSize = 256;
    merged.smoothingTimeConstant = 0.78;
    merger.connect(merged);
    mergedAnalyserRef.current = merged;
    return () => {
      try { merger.disconnect(); merged.disconnect(); } catch {}
      mergedAnalyserRef.current = null;
    };
  }, [analyserL, analyserR]);

  // Wave sub-style state
  const [waveStyle, setWaveStyle] = useState<WaveStyle>(() =>
    normalizeWaveStyle(localStorage.getItem(WAVE_STYLE_KEY))
  );
  const cycleWaveStyle = useCallback(() => {
    setWaveStyle(s => {
      const next = getNextWaveStyle(s);
      localStorage.setItem(WAVE_STYLE_KEY, next);
      return next;
    });
  }, []);

  // Canvas sizes per mode
  const isNeedleLike = mode !== 'bars';
  const mW = 112;
  const mH = 90;
  const nextMode = getNextVizMode(mode);

  const miniButtonStyle: React.CSSProperties = {
    background: 'color-mix(in srgb, var(--text) 6%, transparent)',
    border: `1px solid ${PLAYER_THEME_TOKENS.border}`,
    borderRadius: 3, padding: '1px 5px',
    color: PLAYER_THEME_TOKENS.textMuted, fontSize: 8, fontFamily: PLAYER_THEME_TOKENS.font,
    cursor: 'pointer', letterSpacing: 0.5, lineHeight: '12px',
  };

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: isNeedleLike ? 2 : 3,
      padding: '0 8px',
      borderLeft: `1px solid ${PLAYER_THEME_TOKENS.border}`,
      borderRight: `1px solid ${PLAYER_THEME_TOKENS.border}`,
      height: '100%', position: 'relative',
    }}>
      {/* Mode toggle button — top-right of the meter block */}
      <button
        onClick={onToggleMode}
        title={getVizModeToggleTitle(mode)}
        style={{ position: 'absolute', top: 6, right: 10, zIndex: 1, ...miniButtonStyle }}
      >
        {nextMode === 'needle' ? 'NDL' : nextMode === 'hifi' ? 'HIFI' : nextMode === 'wave' ? 'WAVE' : 'BAR'}
      </button>

      {mode === 'bars' && (
        <div style={{
          writingMode: 'vertical-rl', fontSize: 7, fontWeight: 700,
          letterSpacing: 3, color: PLAYER_THEME_TOKENS.textMuted, userSelect: 'none',
          fontFamily: PLAYER_THEME_TOKENS.font, marginRight: 1, textTransform: 'uppercase',
        }}>VU</div>
      )}

      {mode === 'wave' ? (
        <div style={{ position: 'relative' }}>
          <WaveVisualizer
            analyser={mergedAnalyserRef.current}
            isPlaying={isPlaying}
            style={waveStyle}
            width={mW * 2 + 8}
            height={mH}
          />
          {/* Sub-style cycle button — bottom-left of the visualizer canvas */}
          <button
            onClick={cycleWaveStyle}
            title={`Wave style: ${waveStyle} — click to change`}
            style={{ position: 'absolute', bottom: 6, left: 6, ...miniButtonStyle }}
          >
            {waveStyleLabel(waveStyle)}
          </button>
        </div>
      ) : (
        <>
          <MeterCanvas analyser={analyserL} channel="left"  label="L" isPlaying={isPlaying} mode={mode} width={mW} height={mH} />
          <MeterCanvas analyser={analyserR} channel="right" label="R" isPlaying={isPlaying} mode={mode} width={mW} height={mH} />
        </>
      )}
      {vuDebugOn && (
        <div style={{
          position: 'fixed', left: 4, bottom: 4, zIndex: 9999,
          background: 'rgba(0,0,0,0.85)', color: '#0f0', fontSize: 11,
          fontFamily: 'monospace', padding: '4px 6px', borderRadius: 4,
          maxWidth: '90vw', wordBreak: 'break-all', pointerEvents: 'none',
        }}>
          VU: analyser={analyser ? 'set' : 'null'} | ctx={analyser?.context.state ?? 'none'}
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Slider
// ─────────────────────────────────────────────────────────────────────────────

function Slider({ value, max, onChange, onSeekStart, onSeekEnd, color = PLAYER_THEME_TOKENS.accent, thin = false, vertical = false, verticalHeight = 64 }: {
  value: number; max: number; onChange: (v: number) => void;
  onSeekStart?: () => void; onSeekEnd?: () => void;
  color?: string; thin?: boolean; vertical?: boolean; verticalHeight?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const calc = useCallback((e: MouseEvent | React.MouseEvent) => {
    const rect = ref.current!.getBoundingClientRect();
    if (vertical) {
      return Math.max(0, Math.min(1, 1 - (e.clientY - rect.top) / rect.height)) * max;
    }
    return Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)) * max;
  }, [max, vertical]);
  const onMouseDown = (e: React.MouseEvent) => {
    dragging.current = true; onSeekStart?.(); onChange(calc(e));
    const move = (ev: MouseEvent) => { if (dragging.current) onChange(calc(ev)); };
    const up   = (ev: MouseEvent) => {
      if (dragging.current) { dragging.current = false; onChange(calc(ev)); onSeekEnd?.(); }
      window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
  };
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  const h = thin ? 3 : 4;
  if (vertical) {
    return (
      <div ref={ref} onMouseDown={onMouseDown} style={{ width: h+8, height: verticalHeight, display: 'flex', justifyContent: 'center', cursor: 'pointer', userSelect: 'none' }}>
        <div style={{ width: h, flex: 1, backgroundColor: PLAYER_THEME_TOKENS.border, borderRadius: h, position: 'relative' }}>
          <div style={{ position: 'absolute', left: 0, bottom: 0, width: '100%', height: `${pct}%`, backgroundColor: color, borderRadius: h }} />
          <div style={{ position: 'absolute', left: '50%', bottom: `${pct}%`, transform: 'translate(-50%, 50%)', width: thin?10:12, height: thin?10:12, backgroundColor: PLAYER_THEME_TOKENS.text, borderRadius: '50%', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' }} />
        </div>
      </div>
    );
  }
  return (
    <div ref={ref} onMouseDown={onMouseDown} style={{ flex: 1, height: h+8, display: 'flex', alignItems: 'center', cursor: 'pointer', userSelect: 'none' }}>
      <div style={{ flex: 1, height: h, backgroundColor: PLAYER_THEME_TOKENS.border, borderRadius: h, position: 'relative' }}>
        <div style={{ position: 'absolute', left: 0, top: 0, height: '100%', width: `${pct}%`, backgroundColor: color, borderRadius: h }} />
        <div style={{ position: 'absolute', top: '50%', left: `${pct}%`, transform: 'translate(-50%,-50%)', width: thin?10:12, height: thin?10:12, backgroundColor: PLAYER_THEME_TOKENS.text, borderRadius: '50%', boxShadow: '0 1px 4px rgba(0,0,0,0.35)' }} />
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Queue Panel
// ─────────────────────────────────────────────────────────────────────────────

function QueuePanel({ queue, currentIndex, onSelect, onRemove, onClear, onClose, lockSelect, lockEdit, bottom = DESKTOP_PLAYER_DOCK_HEIGHT }: {
  queue: Track[]; currentIndex: number;
  onSelect: (i: number) => void; onRemove: (i: number) => void;
  onClear: () => void; onClose: () => void;
  lockSelect?: boolean;
  lockEdit?: boolean;
  bottom?: number;
}) {
  const popupBottom = bottom + DESKTOP_PLAYER_POPUP_GAP;
  return (
    <div
      role="dialog"
      aria-label="Playback queue"
      data-ui-region="playback-queue"
      style={{
        ...hybridAudioPanelStyles.popup,
        position: 'fixed',
        right: 12,
        bottom: popupBottom,
        width: 352,
        display: 'flex',
        flexDirection: 'column',
        maxHeight: `calc(100vh - ${popupBottom + 12}px)`,
        zIndex: 200,
        fontFamily: PLAYER_THEME_TOKENS.font,
      }}
    >
      <div style={hybridAudioPanelStyles.header}>
        <span style={hybridAudioPanelStyles.title}>Up next <span style={{ color: 'var(--text-muted)', fontWeight: 600 }}>· {queue.length}</span></span>
        <div style={hybridAudioPanelStyles.headerActions}>
          <button
            type="button"
            disabled={lockEdit}
            onClick={onClear}
            style={{
              ...hybridControlStyles.secondaryButton,
              minHeight: 32,
              padding: '6px 10px',
              ...(lockEdit ? hybridControlStyles.disabled : {}),
            }}
          >
            Clear
          </button>
          <button type="button" aria-label="Close queue" onClick={onClose} style={{ ...hybridControlStyles.iconButton, width: 32, minWidth: 32, height: 32 }}>
            <CloseIcon />
          </button>
        </div>
      </div>
      <div role="listbox" aria-label="Queued tracks" style={hybridAudioPanelStyles.list}>
        {queue.length === 0 && <div style={hybridAudioPanelStyles.empty}>Your queue is empty.</div>}
        {queue.map((track, i) => (
          <div
            key={`${track.id}-${i}`}
            role="option"
            aria-selected={i === currentIndex}
            tabIndex={lockSelect ? -1 : 0}
            style={{
              ...hybridAudioPanelStyles.listRow,
              ...(i === currentIndex ? hybridAudioPanelStyles.listRowActive : {}),
              cursor: lockSelect ? 'default' : 'pointer',
            }}
            onClick={() => { if (!lockSelect) onSelect(i); }}
            onKeyDown={(event) => {
              if (lockSelect || (event.key !== 'Enter' && event.key !== ' ')) return;
              event.preventDefault();
              onSelect(i);
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 12, fontWeight: i === currentIndex ? 700 : 550, color: PLAYER_THEME_TOKENS.text, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{track.title || track.file_name}</div>
              <div style={{ fontSize: 11, color: PLAYER_THEME_TOKENS.textMuted, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{track.artist || 'Unknown artist'}</div>
            </div>
            <button
              type="button"
              aria-label={`Remove ${track.title || track.file_name} from queue`}
              disabled={lockEdit}
              onClick={e => { e.stopPropagation(); if (!lockEdit) onRemove(i); }}
              style={{
                ...hybridControlStyles.iconButton,
                width: 30,
                minWidth: 30,
                height: 30,
                background: 'transparent',
                ...(lockEdit ? hybridControlStyles.disabled : {}),
              }}
            >
              <CloseIcon />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function PlayerAdaptiveAccent({ imageUrl, enabled }: { imageUrl: string | null; enabled: boolean }) {
  useAdaptiveAccentEnabled(imageUrl, enabled);
  return null;
}

// ─────────────────────────────────────────────────────────────────────────────
// Player
// ─────────────────────────────────────────────────────────────────────────────

/** Player is part of this module's public API. */
export default function Player({
  state,
  onStateChange,
  ffmpegAvailable,
  onOpenArtist,
  onOpenAlbum,
  playbackMode = 'standard',
  vinylHardcore = false,
  vinylNeedleDrop = false,
  vinylAnalogFxDisabled = false,
  vinylNeedleDropIntensity = 0.65,
  headless = false,
  onPlaybackSnapshotChange,
  onEqControlsChange,
  hybridPreview = false,
  adaptiveAccentEnabled = true,
}: PlayerProps) {
  const audioRef    = useRef<HTMLAudioElement | null>(null);
  const audioBRef   = useRef<HTMLAudioElement | null>(null);
  const activeSlotRef = useRef<'A' | 'B'>('A');
  const [audioElForVu, setAudioElForVu] = useState<HTMLAudioElement | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration,    setDuration]    = useState(0);
  const [volume,      setVolume]      = useState(0.5);
  const [muted,       setMuted]       = useState(false);
  const [eqOpen,      setEqOpen]      = useState(false);
  const [autoEqEnabled, setAutoEqEnabled] = useState(false);
  const [autoEqCurrentPreset, setAutoEqCurrentPreset] = useState<BuiltinEqProfileName>('Rock');
  const [parametricBands, setParametricBands] = useState<ParametricEqBand[]>(() => DEFAULT_PARAMETRIC_BANDS.map((b) => ({ ...b })));
  const [parametricProfile, setParametricProfile] = useState<string>('Manual');
  const [customParametricProfiles, setCustomParametricProfiles] = useState<ParametricEqProfile[]>([]);
  const [newParametricProfileName, setNewParametricProfileName] = useState('');
  const [showQueue,   setShowQueue]   = useState(false);
  const [repeatMode,  setRepeatMode]  = useState<'off' | 'one' | 'all'>('off');
  const [shuffled,    setShuffled]    = useState(false);
  const [lyricsOpen,  setLyricsOpen]  = useState(false);
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [lyricsText, setLyricsText] = useState('');
  const [lyricsError, setLyricsError] = useState<string | null>(null);
  const [lyricsSource, setLyricsSource] = useState<'cache' | 'lrclib' | 'lyrics.ovh' | null>(null);
  const [lyricsTrackId, setLyricsTrackId] = useState<ClientEntityId | null>(null);
  const [lyricsSynced, setLyricsSynced] = useState<Array<{ time: number; text: string }>>([]);
  const [karaokeMode, setKaraokeMode] = useState(false);
  const lyricsBodyRef = useRef<HTMLDivElement | null>(null);
  const syncedLineRefs = useRef<Array<HTMLDivElement | null>>([]);
  const repeatModeRef = useRef<'off' | 'one' | 'all'>('off');
  repeatModeRef.current = repeatMode;
  const [seeking,     setSeeking]     = useState(false);
  const [loading,     setLoading]     = useState(false);
  const [audioError,  setAudioError]  = useState<string | null>(null);
  const [waveform,    setWaveform]    = useState<TrackWaveform | null>(null);
  const [waveformStatus, setWaveformStatus] = useState<WaveformBarStatus>('loading');
  const [sonicFingerprint, setSonicFingerprint] = useState<SonicFingerprint | null>(null);
  const [sonicFingerprintChecked, setSonicFingerprintChecked] = useState(false);
  const [showSonicFingerprint, setShowSonicFingerprint] = useState(false);
  const [audioReady,  setAudioReady]  = useState(false);
  const [vizMode,     setVizMode]     = useState<VizMode>(() => {
    try { return normalizeVizMode(localStorage.getItem('vizMode')); } catch { return 'bars'; }
  });
  const seekValueRef = useRef(0);
  const waveformPollRef = useRef<number | null>(null);
  const waveformTrackIdRef = useRef<ClientEntityId | null>(null);
  // Tracks the stream URL that was last loaded into the audio element.
  // Used to detect when the noTranscode cookie changed so we can reload the
  // src before playing, even when currentTrack?.id hasn't changed.
  const loadedUrlRef = useRef('');
  // Set during zero-gap/crossfade handoff so index changes do not force-reload
  // a standby track that is already in-progress.
  const handoffInProgressRef = useRef(false);
  // If direct streaming fails for the current track, retry once with server
  // transcoding and keep that URL for pause/resume on the same track.
  const forceTranscodeForTrackRef = useRef(false);
  const lastNeedleDropTokenRef = useRef(-1);
  const wasPlayingRef = useRef(false);
  const eqPopupRef = useRef<HTMLDivElement | null>(null);
  const eqParametricFiltersARef = useRef<BiquadFilterNode[] | null>(null);
  const eqParametricFiltersBRef = useRef<BiquadFilterNode[] | null>(null);
  const eqCtxARef = useRef<AudioContext | null>(null);
  const eqCtxBRef = useRef<AudioContext | null>(null);
  const eqSourceARef = useRef<MediaElementAudioSourceNode | null>(null);
  const eqSourceBRef = useRef<MediaElementAudioSourceNode | null>(null);
  // VU meter analysers tap the same MediaElementAudioSourceNode the EQ chain
  // creates for each slot. A media element can only ever be associated with
  // ONE MediaElementAudioSourceNode for its lifetime, so the VU meter must
  // NOT create its own — doing so throws (and previously left the VU meter
  // permanently stuck on its synthetic fallback animation).
  const eqAnalyserARef = useRef<AnalyserNode | null>(null);
  const eqAnalyserBRef = useRef<AnalyserNode | null>(null);
  const [analyserForVu, setAnalyserForVu] = useState<AnalyserNode | null>(null);
  const autoEqLoadedRef = useRef(false);

  // Keeps the VU meter's analyser in lockstep with whichever audio element is
  // currently active (mirrors setAudioElForVu). Returns null if the EQ chain
  // (and therefore the analyser tap) hasn't been created for that element yet
  // — it will follow once the first user-gesture EQ setup runs.
  const selectVuElement = useCallback((el: HTMLAudioElement | null) => {
    setAudioElForVu(el);
    if (el && el === audioRef.current) setAnalyserForVu(eqAnalyserARef.current);
    else if (el && el === audioBRef.current) setAnalyserForVu(eqAnalyserBRef.current);
    else setAnalyserForVu(null);
  }, []);

  // ─── Progress-tracking refs ─────────────────────────────────────────────
  const currentTimeRef = useRef(0);       // stale-closure-safe current time
  const pendingSeekRef = useRef<number | null>(null); // seek to apply on canplay
  const currentIndexRef = useRef(0);
  const progressMapRef = useRef(new Map<ClientEntityId, number>()); // trackId→saved seconds (session cache)
  const lastProgressSaveTimeRef = useRef(-Infinity);        // throttle periodic saves

  // ─── Crossfade state ───────────────────────────────────────────────────
  const [crossfadeConfig, setCrossfadeConfig] = useState<CrossfadeConfig>({ mode: 'off', duration: 2, source: 'global' });
  const crossfadeActiveRef = useRef(false);   // true while a crossfade ramp is in progress
  const crossfadeRafRef = useRef<number | null>(null);
  const crossfadeTriggeredRef = useRef(false); // guard to prevent re-triggering

  const getActiveAudio = useCallback(() =>
    activeSlotRef.current === 'A' ? audioRef.current : audioBRef.current,
  []);
  const getStandbyAudio = useCallback(() =>
    activeSlotRef.current === 'A' ? audioBRef.current : audioRef.current,
  []);

  const { queue, currentIndex, isPlaying, playToken, queueSource } = state;
  const isVinylMode = playbackMode === 'vinyl';
  const playerDockHeight = resolveDesktopPlayerDockHeight(isVinylMode);
  currentIndexRef.current = currentIndex;

  // Always-fresh saveProgress — avoids stale closures in useCallback handlers
  const saveProgressRef = useRef<(idx: number) => void>(() => {});
  saveProgressRef.current = (idx: number) => {
    if (queueSource?.type !== 'playlist' || !queueSource.rememberProgress) return;
    const track = queue[idx];
    if (!track) return;
    // Read directly from the audio element — always accurate even after a seek
    // that hasn't yet fired a timeupdate event.
    const activeAudio = activeSlotRef.current === 'A' ? audioRef.current : audioBRef.current;
    const seconds = Math.floor(activeAudio?.currentTime ?? currentTimeRef.current);
    const trackDuration = track.duration ?? 0;
    const nearEnd = trackDuration > 0 && (trackDuration - seconds) < 10;
    const finalSeconds = (seconds <= 3 || nearEnd) ? 0 : seconds;
    progressMapRef.current.set(track.id, finalSeconds);
    api.playlists.saveTrackProgress(String(queueSource.id), track.id, finalSeconds).catch(() => {});
  };

  const currentTrack = queue[currentIndex] ?? null;
  const currentTrackMeta = getTrackMetaDisplay(currentTrack);
  const currentTrackAlbumArt = currentTrack?.album_id ? api.albumArtUrl(currentTrack.album_id, 300) : null;
  const currentTrackTitle = truncateTrackTitle(
    currentTrack?.title || currentTrack?.file_name || '-',
    PLAYER_LAYOUT.trackTitleMaxChars,
  );
  const waveformPoints = waveform?.points?.length ? waveform.points : null;
  const showWaveformProgress = waveformStatus === 'ready' || waveformStatus === 'loading' || waveformStatus === 'generating';

  useEffect(() => {
    if (!currentTrack?.id) return;
    if (!autoEqEnabled) return;
    if (typeof (api as any).trackEqProfile !== 'function') return;
    let cancelled = false;
    const applyAutoProfile = (profile: BuiltinEqProfileName) => {
      setAutoEqCurrentPreset(profile);
      const mapped = mapGraphicProfileToParametricPreset(profile);
      setParametricProfile(mapped);
      setParametricBands((BUILTIN_PARAMETRIC_PRESETS[mapped] ?? BUILTIN_PARAMETRIC_PRESETS.Manual).map((b) => ({ ...b })));
    };
    (api as any).trackEqProfile(currentTrack.id)
      .then((result: any) => {
        if (cancelled) return;
        const profile = result?.eq_profile as string;
        if (!isBuiltinEqProfileName(profile)) return;
        applyAutoProfile(profile);
      })
      .catch(() => {
        if (cancelled) return;
        applyAutoProfile('Rock');
      });
    return () => { cancelled = true; };
  }, [autoEqEnabled, currentTrack?.id, playToken]);

  const seekToTime = useCallback((timeSeconds: number, commit: boolean) => {
    if (isVinylMode && vinylHardcore) {
      if (commit) setSeeking(false);
      return;
    }
    const target = Math.max(0, Math.min(Math.max(duration || 0, 0), timeSeconds));
    seekValueRef.current = target;
    currentTimeRef.current = target;
    setCurrentTime(target);
    const active = getActiveAudio();
    if (active && Number.isFinite(target)) {
      active.currentTime = target;
    }
    if (commit) {
      setSeeking(false);
    }
  }, [duration, getActiveAudio, isVinylMode, vinylHardcore]);

  const shouldRunVinylFx = isVinylMode && vinylNeedleDrop && !vinylAnalogFxDisabled;
  const userSettingsApi = (api as { userSettings?: { get?: () => Promise<Record<string, string>>; update?: (updates: Record<string, string>) => Promise<{ ok: boolean }> } }).userSettings;
  const canPersistUserEqProfiles = typeof userSettingsApi?.get === 'function' && typeof userSettingsApi?.update === 'function';

  const persistCustomParametricProfiles = useCallback(async (profiles: ParametricEqProfile[]) => {
    if (!canPersistUserEqProfiles) return;
    await userSettingsApi!.update!({ [USER_PARAMETRIC_EQ_PROFILES_SETTINGS_KEY]: JSON.stringify(profiles) });
  }, [canPersistUserEqProfiles, userSettingsApi]);

  const changeParametricBands = useCallback((bands: ParametricEqBand[]) => {
    setParametricBands(bands);
    setParametricProfile('Manual');
  }, []);

  const changeParametricProfile = useCallback((name: string, bands: ParametricEqBand[]) => {
    setParametricProfile(name);
    setParametricBands(bands.map((band) => ({ ...band })));
  }, []);

  const saveParametricProfile = useCallback(async (name: string, bands: ParametricEqBand[]) => {
    if (!name) return 'Enter a profile name.';
    if (isBuiltinParametricPresetName(name)) return 'Built-in preset names are reserved.';
    if (customParametricProfiles.some((profile) => profile.name.toLowerCase() === name.toLowerCase())) {
      return 'A custom profile with this name already exists.';
    }
    const next = [...customParametricProfiles, { name, bands: bands.map((band) => ({ ...band })) }];
    await persistCustomParametricProfiles(next);
    setCustomParametricProfiles(next);
    setParametricProfile(name);
    setNewParametricProfileName('');
    return null;
  }, [customParametricProfiles, persistCustomParametricProfiles]);

  const deleteParametricProfile = useCallback(async (name: string) => {
    const next = customParametricProfiles.filter((profile) => profile.name !== name);
    await persistCustomParametricProfiles(next);
    setCustomParametricProfiles(next);
    setParametricProfile('Manual');
    setParametricBands(BUILTIN_PARAMETRIC_PRESETS.Manual.map((band) => ({ ...band })));
  }, [customParametricProfiles, persistCustomParametricProfiles]);

  const eqControls = useMemo<PlayerEqControls>(() => ({
    bands: parametricBands,
    profile: parametricProfile,
    customProfiles: customParametricProfiles,
    autoEqEnabled,
    autoEqCurrentPreset,
    onAutoEqEnabledChange: setAutoEqEnabled,
    onBandsChange: changeParametricBands,
    onProfileChange: changeParametricProfile,
    onSaveProfile: saveParametricProfile,
    onDeleteProfile: deleteParametricProfile,
  }), [
    autoEqCurrentPreset,
    autoEqEnabled,
    changeParametricBands,
    changeParametricProfile,
    customParametricProfiles,
    deleteParametricProfile,
    parametricBands,
    parametricProfile,
    saveParametricProfile,
  ]);

  useEffect(() => {
    onEqControlsChange?.(eqControls);
  }, [eqControls, onEqControlsChange]);

  useEffect(() => () => {
    onEqControlsChange?.(null);
  }, [onEqControlsChange]);

  useEffect(() => {
    if (!canPersistUserEqProfiles) return;
    let cancelled = false;
    userSettingsApi!.get!().then((settings) => {
      if (cancelled) return;

      // ── Graphic EQ ────────────────────────────────────────────────────────
      const storedCustomProfiles = parseStoredEqProfiles(settings[USER_EQ_SETTINGS_KEY]);
      const storedProfileRaw = String(settings[USER_EQ_SELECTED_PROFILE_SETTINGS_KEY] ?? '').trim();
      const storedGains = parseStoredEqGains(settings[USER_EQ_GAINS_SETTINGS_KEY]);

      // ── Parametric EQ ─────────────────────────────────────────────────────
      const storedParamBands = parseStoredParametricBands(settings[USER_PARAMETRIC_EQ_BANDS_SETTINGS_KEY]);
      const storedParamProfiles = parseStoredParametricProfiles(settings[USER_PARAMETRIC_EQ_PROFILES_SETTINGS_KEY]);
      const migratedProfiles = storedCustomProfiles
        .map(migrateGraphicProfileToParametricProfile)
        .filter((profile): profile is ParametricEqProfile => profile !== null)
        .filter((profile) => !storedParamProfiles.some((existing) => existing.name.toLowerCase() === profile.name.toLowerCase()));
      const nextParamProfiles = [...storedParamProfiles, ...migratedProfiles];
      setCustomParametricProfiles(nextParamProfiles);

      const storedParamProfile = String(settings[USER_PARAMETRIC_EQ_SELECTED_PROFILE_SETTINGS_KEY] ?? '').trim();
      if (storedParamBands) {
        setParametricBands(storedParamBands);
        if (storedParamProfile) setParametricProfile(storedParamProfile);
      } else {
        let migratedBands: ParametricEqBand[] | null = null;
        let migratedProfile = 'Manual';
        if (isBuiltinEqProfileName(storedProfileRaw)) {
          migratedProfile = mapGraphicProfileToParametricPreset(storedProfileRaw);
          migratedBands = (BUILTIN_PARAMETRIC_PRESETS[migratedProfile] ?? BUILTIN_PARAMETRIC_PRESETS.Manual).map((band) => ({ ...band }));
        } else {
          const legacyCustom = storedCustomProfiles.find((profile) => profile.name === storedProfileRaw);
          migratedBands = migrateGraphicGainsToParametricBands(storedGains ?? legacyCustom?.gains ?? null);
          if (legacyCustom && migratedBands) migratedProfile = legacyCustom.name;
        }
        if (migratedBands) {
          setParametricBands(migratedBands);
          setParametricProfile(migratedProfile);
          userSettingsApi!.update!({
            [USER_PARAMETRIC_EQ_BANDS_SETTINGS_KEY]: JSON.stringify(migratedBands),
            [USER_PARAMETRIC_EQ_SELECTED_PROFILE_SETTINGS_KEY]: migratedProfile,
            [USER_PARAMETRIC_EQ_PROFILES_SETTINGS_KEY]: JSON.stringify(nextParamProfiles),
            [USER_EQ_MODE_SETTINGS_KEY]: 'parametric',
          }).catch(() => {});
        } else if (storedParamProfile) {
          setParametricProfile(storedParamProfile);
        }
      }

      // ── Common ────────────────────────────────────────────────────────────
      if (settings[USER_AUTO_EQ_ENABLED_SETTINGS_KEY] !== undefined) {
        setAutoEqEnabled(settings[USER_AUTO_EQ_ENABLED_SETTINGS_KEY] === 'true');
      } else {
        setAutoEqEnabled(false);
      }
      const storedVolume = parseStoredVolume(settings[USER_VOLUME_SETTINGS_KEY]);
      if (storedVolume != null) {
        setVolume(storedVolume);
      } else {
        setVolume(0.5);
      }
      if (settings[USER_MUTED_SETTINGS_KEY] !== undefined) {
        setMuted(settings[USER_MUTED_SETTINGS_KEY] === 'true');
      } else {
        setMuted(false);
      }
      autoEqLoadedRef.current = true;
    }).catch(() => {
      if (cancelled) return;
      setCustomParametricProfiles([]);
      setAutoEqEnabled(false);
      setVolume(0.5);
      setMuted(false);
      autoEqLoadedRef.current = true;
    });
    return () => {
      cancelled = true;
      autoEqLoadedRef.current = false;
    };
  }, [canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    if (!canPersistUserEqProfiles || !autoEqLoadedRef.current) return;
    userSettingsApi!.update!({ [USER_AUTO_EQ_ENABLED_SETTINGS_KEY]: String(autoEqEnabled) }).catch(() => {});
  }, [autoEqEnabled, canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    if (!canPersistUserEqProfiles || !autoEqLoadedRef.current) return;
    userSettingsApi!.update!({ [USER_VOLUME_SETTINGS_KEY]: String(volume) }).catch(() => {});
  }, [volume, canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    if (!canPersistUserEqProfiles || !autoEqLoadedRef.current) return;
    userSettingsApi!.update!({ [USER_MUTED_SETTINGS_KEY]: String(muted) }).catch(() => {});
  }, [muted, canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    if (!canPersistUserEqProfiles || !autoEqLoadedRef.current) return;
    userSettingsApi!.update!({ [USER_PARAMETRIC_EQ_BANDS_SETTINGS_KEY]: JSON.stringify(parametricBands) }).catch(() => {});
  }, [parametricBands, canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    if (!canPersistUserEqProfiles || !autoEqLoadedRef.current) return;
    userSettingsApi!.update!({ [USER_PARAMETRIC_EQ_SELECTED_PROFILE_SETTINGS_KEY]: parametricProfile }).catch(() => {});
  }, [parametricProfile, canPersistUserEqProfiles, userSettingsApi]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!eqOpen) return;
      const target = event.target as Node | null;
      if (target && eqPopupRef.current?.contains(target)) return;
      setEqOpen(false);
    };
    window.addEventListener('pointerdown', onPointerDown);
    return () => window.removeEventListener('pointerdown', onPointerDown);
  }, [eqOpen]);

  const ensureEqForElement = useCallback((audio: HTMLAudioElement | null, slot: 'A' | 'B') => {
    if (!audio) return;
    const hasFilters = slot === 'A' ? eqParametricFiltersARef.current : eqParametricFiltersBRef.current;
    if (hasFilters) return;

    const AudioContextCtor = window.AudioContext || (window as any).webkitAudioContext;
    if (typeof AudioContextCtor !== 'function') return;
    const ctx = new AudioContextCtor();
    const source = ctx.createMediaElementSource(audio);

    const parametricFilters = DEFAULT_PARAMETRIC_BANDS.map(() => {
      const filter = ctx.createBiquadFilter();
      filter.type = 'peaking';
      filter.frequency.value = 1000;
      filter.Q.value = 1;
      filter.gain.value = 0;
      return filter;
    });

    // source -> parametric[0..6] -> destination
    source.connect(parametricFilters[0]);
    for (let i = 0; i < parametricFilters.length - 1; i += 1) parametricFilters[i].connect(parametricFilters[i + 1]);
    parametricFilters[parametricFilters.length - 1].connect(ctx.destination);

    // VU meter tap — parallel branch off the raw source, silent (not
    // connected onward), so it doesn't affect the audible EQ chain above.
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 2048;
    analyser.smoothingTimeConstant = 0.4;
    source.connect(analyser);

    if (slot === 'A') {
      eqCtxARef.current = ctx;
      eqSourceARef.current = source;
      eqParametricFiltersARef.current = parametricFilters;
      eqAnalyserARef.current = analyser;
    } else {
      eqCtxBRef.current = ctx;
      eqSourceBRef.current = source;
      eqParametricFiltersBRef.current = parametricFilters;
      eqAnalyserBRef.current = analyser;
    }

    if (activeSlotRef.current === slot) setAnalyserForVu(analyser);

    applyParametricEqToFilters(parametricFilters, parametricBands);
  }, [parametricBands]);

  useEffect(() => {
    const applyParam = (filters: BiquadFilterNode[] | null) => {
      if (!filters) return;
      applyParametricEqToFilters(filters, parametricBands);
    };
    applyParam(eqParametricFiltersARef.current);
    applyParam(eqParametricFiltersBRef.current);
  }, [parametricBands]);

  const ensureEqContextsRunning = useCallback(() => {
    void eqCtxARef.current?.resume().catch(() => {});
    void eqCtxBRef.current?.resume().catch(() => {});
  }, []);

  useEffect(() => {
    const onUserGesture = () => {
      ensureEqForElement(audioRef.current, 'A');
      ensureEqForElement(audioBRef.current, 'B');
      ensureEqContextsRunning();
    };
    const opts: AddEventListenerOptions = { passive: true };
    window.addEventListener('touchstart', onUserGesture, opts);
    window.addEventListener('pointerdown', onUserGesture, opts);
    window.addEventListener('click', onUserGesture, opts);
    return () => {
      window.removeEventListener('touchstart', onUserGesture);
      window.removeEventListener('pointerdown', onUserGesture);
      window.removeEventListener('click', onUserGesture);
    };
  }, [ensureEqContextsRunning, ensureEqForElement]);

  useEffect(() => {
    return () => {
      try {
        eqSourceARef.current?.disconnect();
        eqSourceBRef.current?.disconnect();
        eqParametricFiltersARef.current?.forEach((node) => node.disconnect());
        eqParametricFiltersBRef.current?.forEach((node) => node.disconnect());
      } catch {}
      void eqCtxARef.current?.close().catch(() => {});
      void eqCtxBRef.current?.close().catch(() => {});
    };
  }, []);

  const runVinylRamp = useCallback((audio: HTMLAudioElement | null) => {
    if (!audio || !isVinylMode || crossfadeActiveRef.current) return;
    const target = muted ? 0 : volume;
    if (target <= 0) return;
    const floor = Math.min(0.12, target);
    const rampMs = 400;
    const started = performance.now();
    audio.volume = floor;
    const tick = () => {
      const p = Math.min(1, (performance.now() - started) / rampMs);
      audio.volume = floor + (target - floor) * p;
      if (p < 1 && isVinylMode) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }, [isVinylMode, muted, volume]);

  const triggerVinylNeedleDrop = useCallback((audio: HTMLAudioElement | null, intensityOverride?: number) => {
    if (!audio || !isVinylMode) return;
    if (shouldRunVinylFx) {
      const fxIntensity = typeof intensityOverride === 'number' ? intensityOverride : vinylNeedleDropIntensity;
      void playNeedleDrop(fxIntensity);
    }
    if (!vinylAnalogFxDisabled) {
      runVinylRamp(audio);
    }
  }, [isVinylMode, runVinylRamp, shouldRunVinylFx, vinylAnalogFxDisabled, vinylNeedleDropIntensity]);

  // Stable ref so load-track effect doesn't rerun when volume changes triggerVinylNeedleDrop identity
  const triggerVinylNeedleDropRef = useRef(triggerVinylNeedleDrop);
  triggerVinylNeedleDropRef.current = triggerVinylNeedleDrop;

  const toggleMode = useCallback(() => {
    setVizMode(m => {
      const next = getNextVizMode(m);
      try { localStorage.setItem('vizMode', next); } catch {}
      return next;
    });
  }, []);

  // ─── Fetch crossfade config when queue source changes ────────────────────
  useEffect(() => {
    if (queueSource && (queueSource.type === 'album' || queueSource.type === 'playlist' || queueSource.type === 'autodj')) {
      api.crossfade.config(queueSource.type, queueSource.id).then(setCrossfadeConfig).catch(() => {});
    } else {
      api.crossfade.config().then(setCrossfadeConfig).catch(() => {});
    }
  }, [queueSource]);

  const stopWaveformPoll = useCallback(() => {
    if (waveformPollRef.current != null) {
      window.clearTimeout(waveformPollRef.current);
      waveformPollRef.current = null;
    }
  }, []);

  useEffect(() => {
    stopWaveformPoll();
    setWaveform(null);
    if (!currentTrack) {
      setWaveformStatus('missing');
      return;
    }

    let cancelled = false;
    const trackId = currentTrack.id;
    waveformTrackIdRef.current = trackId;
    setWaveformStatus('loading');

    const loadWaveform = async (attempt = 0) => {
      if (cancelled || waveformTrackIdRef.current !== trackId) return;
      try {
        const response = await api.trackWaveform(trackId);
        if (cancelled || waveformTrackIdRef.current !== trackId) return;

        if (response.status === 'ready') {
          setWaveform(response.waveform);
          setWaveformStatus('ready');
          stopWaveformPoll();
          return;
        }

        if (response.status === 'generating') {
          setWaveform(null);
          setWaveformStatus('generating');
          if (attempt >= WAVEFORM_POLL_MAX_ATTEMPTS) {
            setWaveformStatus('missing');
            return;
          }
          waveformPollRef.current = window.setTimeout(() => {
            void loadWaveform(attempt + 1);
          }, WAVEFORM_POLL_INTERVAL_MS);
          return;
        }

        if (response.status === 'missing') {
          setWaveform(null);
          setWaveformStatus('missing');
          return;
        }

        setWaveform(null);
        setWaveformStatus('error');
      } catch {
        if (!cancelled) {
          setWaveform(null);
          setWaveformStatus('error');
        }
      }
    };

    void loadWaveform();

    return () => {
      cancelled = true;
      stopWaveformPoll();
    };
  }, [currentTrack, stopWaveformPoll]);

  // ─── Load sonic fingerprint when track changes ─────────────────────────
  useEffect(() => {
    setSonicFingerprint(null);
    setSonicFingerprintChecked(false);
    setShowSonicFingerprint(false);
    if (!currentTrack) return;
    let cancelled = false;
    api.trackSonicFingerprint(currentTrack.id).then(fp => {
      if (!cancelled) { setSonicFingerprint(fp); setSonicFingerprintChecked(true); }
    }).catch(() => {
      if (!cancelled) { setSonicFingerprint(null); setSonicFingerprintChecked(true); }
    });
    return () => { cancelled = true; };
  }, [currentTrack]);

  // ─── Cancel crossfade on track change or skip ──────────────────────────
  const cancelCrossfade = useCallback(() => {
    if (crossfadeRafRef.current != null) {
      cancelAnimationFrame(crossfadeRafRef.current);
      crossfadeRafRef.current = null;
    }
    crossfadeActiveRef.current = false;
    crossfadeTriggeredRef.current = false;
    handoffInProgressRef.current = false;
    const standby = getStandbyAudio();
    if (standby) {
      standby.pause();
      standby.removeAttribute('src');
      standby.load();
    }
  }, [getStandbyAudio]);

  // ─── Crossfade ramp function ────────────────────────────────────────────
  const beginCrossfade = useCallback((nextIndex: number) => {
    const active = getActiveAudio();
    const standby = getStandbyAudio();
    if (!active || !standby || nextIndex >= queue.length) return;

    const nextTrack = queue[nextIndex];
    if (!nextTrack) return;

    const cfDuration = clampCrossfadeDuration(crossfadeConfig.duration, active.duration);
    if (cfDuration <= 0) return;

    crossfadeActiveRef.current = true;

    // Load next track into standby
    const url = getPreferredTrackStreamUrl(nextTrack);
    standby.src = url;
    standby.load();
    standby.volume = 0;

    const startPlaying = () => {
      standby.play().catch(() => {});
      const startTime = performance.now();
      const durationMs = cfDuration * 1000;
      const targetVol = muted ? 0 : volume;

      const step = () => {
        const elapsed = performance.now() - startTime;
        const progress = Math.min(elapsed / durationMs, 1);

        active.volume = crossfadeVolumeAt(progress, targetVol, 'out');
        standby.volume = crossfadeVolumeAt(progress, targetVol, 'in');

        if (progress < 1) {
          crossfadeRafRef.current = requestAnimationFrame(step);
        } else {
          // Ramp complete: swap
          active.pause();
          active.removeAttribute('src');
          active.load();
          activeSlotRef.current = activeSlotRef.current === 'A' ? 'B' : 'A';
          crossfadeActiveRef.current = false;
          crossfadeTriggeredRef.current = false;
          crossfadeRafRef.current = null;

          // Update VU meter to new active element
          selectVuElement(standby);

          // Advance the queue index without incrementing playToken (track is already playing)
          loadedUrlRef.current = url;
          handoffInProgressRef.current = true;
          onStateChange({ ...state, currentIndex: nextIndex, isPlaying: true });
        }
      };
      crossfadeRafRef.current = requestAnimationFrame(step);
    };

    standby.addEventListener('canplay', startPlaying, { once: true });
  }, [getActiveAudio, getStandbyAudio, queue, crossfadeConfig.duration, volume, muted, state, onStateChange, selectVuElement]);

  // ─── Zero-gap preload + immediate swap ──────────────────────────────────
  const beginZeroGap = useCallback((nextIndex: number) => {
    const standby = getStandbyAudio();
    if (!standby || nextIndex >= queue.length) return;

    const nextTrack = queue[nextIndex];
    if (!nextTrack) return;

    crossfadeActiveRef.current = true;

    const url = getPreferredTrackStreamUrl(nextTrack);
    standby.src = url;
    standby.load();
    standby.volume = muted ? 0 : volume;

    // We'll start the standby when the active track ends (see onEnded handler)
  }, [getStandbyAudio, queue, volume, muted]);

  const finalizeZeroGap = useCallback((nextIndex: number) => {
    const active = getActiveAudio();
    const standby = getStandbyAudio();
    const nextTrack = queue[nextIndex];
    if (!standby || !nextTrack) return;
    const url = getPreferredTrackStreamUrl(nextTrack);

    active?.pause();
    if (active) { active.removeAttribute('src'); active.load(); }

    standby.play().catch(() => {});
    activeSlotRef.current = activeSlotRef.current === 'A' ? 'B' : 'A';
    crossfadeActiveRef.current = false;
    crossfadeTriggeredRef.current = false;

    selectVuElement(standby);
    loadedUrlRef.current = url;
    handoffInProgressRef.current = true;
    onStateChange({ ...state, currentIndex: nextIndex, isPlaying: true });
  }, [getActiveAudio, getStandbyAudio, queue, state, onStateChange, selectVuElement]);

  // Cancel crossfade when playToken changes (user skipped)
  useEffect(() => {
    cancelCrossfade();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playToken]);

  // Load track and start playback. Depends on playToken which increments on
  // every explicit play action (Play, Play All, queue click), so even clicking
  // play on the same track forces a fresh load + play.
  useEffect(() => {
    const audio = activeSlotRef.current === 'A' ? audioRef.current : audioBRef.current;
    if (!audio || !currentTrack) return;
    const url = getPreferredTrackStreamUrl(currentTrack);
    forceTranscodeForTrackRef.current = false;
    if (shouldPreserveTransitionPlayback({
      handoffInProgress: handoffInProgressRef.current,
      expectedUrl: url,
      loadedUrl: loadedUrlRef.current,
      currentAudioSrc: audio.currentSrc || audio.src,
    })) {
      loadedUrlRef.current = url;
      handoffInProgressRef.current = false;
      setLoading(false);
      setAudioError(null);
      return;
    }
    handoffInProgressRef.current = false;
    console.info('[Player] load-track', {
      playToken,
      isPlaying,
      ...buildPlaybackDebugInfo(currentTrack, url),
    });
    loadedUrlRef.current = url;
    audio.src = url;
    // Set pending seek for remember-progress playlists
    if (queueSource?.type === 'playlist' && queueSource.rememberProgress) {
      const localSaved = progressMapRef.current.get(currentTrack.id);
      const dbSaved = (currentTrack as any).progress_seconds ?? 0;
      const saved = localSaved !== undefined ? localSaved : dbSaved;
      pendingSeekRef.current = saved > 3 ? saved : null;
    } else {
      pendingSeekRef.current = null;
    }
    lastProgressSaveTimeRef.current = -Infinity;
    audio.load(); setCurrentTime(0); setDuration(currentTrack.duration ?? 0); setLoading(true); setAudioError(null);
    if (isPlaying) {
      ensureEqForElement(audioRef.current, 'A');
      ensureEqForElement(audioBRef.current, 'B');
      ensureEqContextsRunning();
      if (isVinylMode && playToken !== lastNeedleDropTokenRef.current) {
        lastNeedleDropTokenRef.current = playToken;
        triggerVinylNeedleDropRef.current(audio);
      }
      audio.play().catch((err) => {
        console.warn('[Player] play() rejected after load', {
          ...buildPlaybackDebugInfo(currentTrack, url),
          error: err instanceof Error ? err.message : String(err),
        });
      });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playToken, currentTrack?.id, isVinylMode]);

  // Handle pause/resume toggling (user clicks pause then play without changing track).
  useEffect(() => {
    const audio = getActiveAudio();
    if (!audio || !currentTrack) return;
    if (isPlaying) {
      ensureEqForElement(audioRef.current, 'A');
      ensureEqForElement(audioBRef.current, 'B');
      ensureEqContextsRunning();
      // If crossfade is paused, resume both
      if (crossfadeActiveRef.current) {
        const standby = getStandbyAudio();
        if (standby && standby.src) standby.play().catch(() => {});
      }
      const preferredUrl = getPreferredTrackStreamUrl(currentTrack);
      const expectedUrl = forceTranscodeForTrackRef.current
        ? (getTranscodeFallbackUrl(preferredUrl) ?? preferredUrl)
        : preferredUrl;
      console.info('[Player] resume-check', {
        ...buildPlaybackDebugInfo(currentTrack, expectedUrl),
        loadedUrl: loadedUrlRef.current,
        hasAudioError: Boolean(audio.error),
        forceTranscodeForTrack: forceTranscodeForTrackRef.current,
      });
      if (loadedUrlRef.current !== expectedUrl || audio.error) {
        console.info('[Player] reload-before-play', {
          ...buildPlaybackDebugInfo(currentTrack, expectedUrl),
          reason: loadedUrlRef.current !== expectedUrl ? 'url_changed' : 'audio_error',
        });
        loadedUrlRef.current = expectedUrl;
        audio.src = expectedUrl;
        audio.load();
        setCurrentTime(0); setDuration(currentTrack.duration ?? 0); setLoading(true); setAudioError(null);
      }
      audio.play().catch((err) => {
        console.warn('[Player] play() rejected on resume', {
          ...buildPlaybackDebugInfo(currentTrack, expectedUrl),
          error: err instanceof Error ? err.message : String(err),
        });
      });
    } else {
      console.info('[Player] pause', {
        ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current),
      });
      audio.pause();
      saveProgressRef.current(currentIndexRef.current);
      // Pause standby too during crossfade
      if (crossfadeActiveRef.current) {
        const standby = getStandbyAudio();
        if (standby) standby.pause();
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isPlaying, ensureEqForElement, ensureEqContextsRunning]);

  useEffect(() => {
    const active = getActiveAudio();
    if (active && !crossfadeActiveRef.current) { active.volume = volume; active.muted = muted; }
  }, [volume, muted, getActiveAudio]);

  useEffect(() => {
    if (!isVinylMode || !shouldRunVinylFx) return;
    void preloadVinylFx();
  }, [isVinylMode, shouldRunVinylFx]);

  useEffect(() => { setAudioReady(true); }, []);

  useEffect(() => {
    const transitionedToPlaying = !wasPlayingRef.current && isPlaying;
    wasPlayingRef.current = isPlaying;
    if (!transitionedToPlaying || !isVinylMode) return;
    const audio = getActiveAudio();
    triggerVinylNeedleDrop(audio, 0.8 * vinylNeedleDropIntensity);
  }, [getActiveAudio, isPlaying, isVinylMode, triggerVinylNeedleDrop, vinylNeedleDropIntensity]);

  const playIndex = (i: number) => { saveProgressRef.current(currentIndex); onStateChange({ ...state, currentIndex: i, isPlaying: true, playToken: playToken + 1 }); };
  const playNext  = useCallback(() => {
    saveProgressRef.current(currentIndexRef.current);
    if (repeatModeRef.current === 'one') {
      onStateChange({ ...state, isPlaying: true, playToken: playToken + 1 });
    } else if (currentIndex < queue.length - 1) {
      onStateChange({ ...state, currentIndex: currentIndex + 1, isPlaying: true, playToken: playToken + 1 });
    } else if (repeatModeRef.current === 'all') {
      onStateChange({ ...state, currentIndex: 0, isPlaying: true, playToken: playToken + 1 });
    } else {
      onStateChange({ ...state, isPlaying: false });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentIndex, queue.length, state]);

  const toggleShuffle = () => {
    if (isVinylMode) return;
    const next = !shuffled;
    setShuffled(next);
    if (next && queue.length > 1) {
      const current = queue[currentIndex];
      const rest = queue.filter((_, i) => i !== currentIndex);
      for (let i = rest.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [rest[i], rest[j]] = [rest[j], rest[i]];
      }
      const newQueue = [...rest.slice(0, currentIndex), current, ...rest.slice(currentIndex)];
      onStateChange({ ...state, queue: newQueue });
    }
  };
  const playPrev = () => {
    const active = getActiveAudio();
    if ((active?.currentTime ?? 0) > 3) { if (active) active.currentTime = 0; }
    else if (currentIndex > 0) playIndex(currentIndex - 1);
  };
  const removeFromQueue = (i: number) => {
    const nq = queue.filter((_, idx) => idx !== i);
    let ni = currentIndex;
    if (i < currentIndex) ni--; else if (i === currentIndex) ni = Math.min(currentIndex, nq.length - 1);
    onStateChange({ queue: nq, currentIndex: Math.max(0, ni), isPlaying: i === currentIndex ? false : isPlaying, playToken });
  };
  const clearQueue = () => {
    cancelCrossfade();
    getActiveAudio()?.pause();
    onStateChange({ queue: [], currentIndex: 0, isPlaying: false, playToken });
  };

  useEffect(() => {
    if (isVinylMode && shuffled) {
      setShuffled(false);
    }
  }, [isVinylMode, shuffled]);

  const cycleRepeatMode = useCallback(() => {
    setRepeatMode((mode) => (mode === 'off' ? 'one' : mode === 'one' ? 'all' : 'off'));
  }, []);

  useEffect(() => {
    if (!lyricsOpen || !currentTrack?.id) return;
    if (lyricsTrackId === currentTrack.id && (lyricsText || lyricsError)) return;
    let cancelled = false;
    setLyricsLoading(true);
    setLyricsError(null);
    setLyricsText('');
    setLyricsSource(null);
    setLyricsSynced([]);
    api.trackLyrics(currentTrack.id)
      .then((result) => {
        if (cancelled) return;
        setLyricsTrackId(currentTrack.id);
        setLyricsText(result.lyrics);
        setLyricsSource(result.source);
        setLyricsSynced(Array.isArray(result.synced) ? result.synced : []);
      })
      .catch((e: any) => {
        if (cancelled) return;
        setLyricsTrackId(currentTrack.id);
        setLyricsError(e.message || 'Lyrics not found');
      })
      .finally(() => {
        if (!cancelled) setLyricsLoading(false);
      });
    return () => { cancelled = true; };
  }, [lyricsOpen, currentTrack?.id, lyricsTrackId, lyricsText, lyricsError]);

  const activeSyncedLyricIndex = (() => {
    if (!lyricsSynced.length) return -1;
    let idx = -1;
    for (let i = 0; i < lyricsSynced.length; i++) {
      if (currentTime >= lyricsSynced[i].time) idx = i;
      else break;
    }
    return idx;
  })();

  useEffect(() => {
    if (!lyricsOpen || !karaokeMode || activeSyncedLyricIndex < 0) return;
    const activeLine = syncedLineRefs.current[activeSyncedLyricIndex];
    if (!lyricsBodyRef.current || !activeLine) return;
    activeLine.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [lyricsOpen, karaokeMode, activeSyncedLyricIndex]);

  useEffect(() => {
    onPlaybackSnapshotChange?.({
      currentTrack,
      currentTime,
      duration,
      isPlaying,
      volume,
      muted,
      loading,
      audioError,
    });
  }, [audioError, currentTime, currentTrack, duration, isPlaying, loading, muted, onPlaybackSnapshotChange, volume]);

  if (queue.length === 0) return null;

  return (
    <>
      {hybridPreview && (
        <PlayerAdaptiveAccent
          imageUrl={currentTrackAlbumArt}
          enabled={adaptiveAccentEnabled}
        />
      )}
      {showQueue && (
        <QueuePanel queue={queue} currentIndex={currentIndex}
          onSelect={playIndex} onRemove={removeFromQueue}
          onClear={clearQueue} onClose={() => setShowQueue(false)}
          lockSelect={isVinylMode && vinylHardcore}
          lockEdit={isVinylMode}
          bottom={playerDockHeight}
        />
      )}

      {/* Audio A */}
      <audio
        ref={(el) => {
          audioRef.current = el;
          if (activeSlotRef.current === 'A') selectVuElement(el);
        }}
        onTimeUpdate={e => {
          if (activeSlotRef.current !== 'A') return;
          const t = e.currentTarget.currentTime;
          if (!seeking) { currentTimeRef.current = t; setCurrentTime(t); }
          if (!seeking && t > 0 && t - lastProgressSaveTimeRef.current >= 5) {
            lastProgressSaveTimeRef.current = t;
            saveProgressRef.current(currentIndexRef.current);
          }
          // Crossfade/zerogap trigger
          const audio = e.currentTarget;
          if (repeatModeRef.current !== 'one' && crossfadeConfig.mode !== 'off' && audio.duration > 0 && !crossfadeTriggeredRef.current && !crossfadeActiveRef.current) {
            const nextIdx = currentIndex + 1;
            if (nextIdx < queue.length || repeatModeRef.current === 'all') {
              const safeNextIdx = nextIdx < queue.length ? nextIdx : 0;
              const threshold = computeTransitionThreshold(crossfadeConfig.mode, clampCrossfadeDuration(crossfadeConfig.duration, audio.duration));
              const remaining = audio.duration - audio.currentTime;
              if (remaining <= threshold && remaining > 0) {
                crossfadeTriggeredRef.current = true;
                if (crossfadeConfig.mode === 'crossfade') {
                  beginCrossfade(safeNextIdx);
                } else {
                  beginZeroGap(safeNextIdx);
                }
              }
            }
          }
        }}
        onDurationChange={e => { if (activeSlotRef.current === 'A' && !currentTrack?.duration) setDuration(e.currentTarget.duration); }}
        onEnded={() => {
          if (activeSlotRef.current !== 'A') return;
          setCurrentTime((prev) => Math.max(prev, duration));
          console.info('[Player] ended', {
            ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current),
          });
          if (repeatModeRef.current === 'one') {
            const audio = audioRef.current;
            if (audio) { audio.currentTime = 0; audio.play().catch(() => {}); }
          } else if (crossfadeActiveRef.current && crossfadeConfig.mode === 'zerogap') {
            finalizeZeroGap(currentIndex + 1);
          } else if (!crossfadeActiveRef.current) {
            playNext();
          }
        }}
        onCanPlay={(e) => {
          if (activeSlotRef.current !== 'A') return;
          console.info('[Player] can-play', {
            ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current),
          });
          const pending = pendingSeekRef.current;
          if (pending !== null) {
            pendingSeekRef.current = null;
            const audio = e.currentTarget;
            const target = audio.duration > 0 ? Math.min(pending, audio.duration - 1) : pending;
            audio.currentTime = target;
            currentTimeRef.current = target;
            setCurrentTime(target);
          }
          setLoading(false); setAudioError(null);
        }}
        onWaiting={() => {
          if (activeSlotRef.current !== 'A') return;
          console.info('[Player] waiting', {
            ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current),
          });
          setLoading(true);
        }}
        onError={e => {
          if (activeSlotRef.current !== 'A') return;
          const audio = e.currentTarget;
          const code = audio.error?.code ?? 0;
          const fallbackUrl = !forceTranscodeForTrackRef.current
            ? getTranscodeFallbackUrl(loadedUrlRef.current || audio.currentSrc || audio.src)
            : null;
          if (fallbackUrl) {
            console.warn('[Player] stream error, retrying with transcoding', {
              ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current || audio.currentSrc || audio.src),
              code,
              fallbackUrl,
            });
            forceTranscodeForTrackRef.current = true;
            loadedUrlRef.current = fallbackUrl;
            audio.src = fallbackUrl;
            audio.load();
            setLoading(true);
            setAudioError('Direct stream failed, retrying with transcoding...');
            if (isPlaying) {
              audio.play().catch((err) => {
                console.warn('[Player] play() rejected during fallback retry', {
                  ...buildPlaybackDebugInfo(currentTrack, fallbackUrl),
                  error: err instanceof Error ? err.message : String(err),
                });
              });
            }
            return;
          }
          const ext = getTrackExtension(currentTrack).replace(/^\./, '').toUpperCase();
          const msgs: Record<number, string> = { 1:'Playback aborted', 2:'Network error — check server', 3:`Cannot decode this ${ext} file`, 4:`${ext} is not supported by your browser` };
          console.error('[Player] fatal audio error', {
            ...buildPlaybackDebugInfo(currentTrack, loadedUrlRef.current || audio.currentSrc || audio.src),
            code,
            message: msgs[code] ?? 'Playback error',
            forceTranscodeForTrack: forceTranscodeForTrackRef.current,
          });
          setAudioError(msgs[audio.error?.code ?? 0] ?? 'Playback error'); setLoading(false);
          // Stop playback on error so isPlaying transitions false→true on the
          // next Play All / play action, ensuring the isPlaying effect fires and
          // can pick up any stream URL changes (e.g. noTranscode toggle).
          onStateChange({ ...state, isPlaying: false });
        }}
        preload="auto"
      />

      {/* Audio B (standby for crossfade/zerogap) */}
      <audio
        ref={audioBRef}
        onTimeUpdate={e => {
          if (activeSlotRef.current !== 'B') return;
          const t = e.currentTarget.currentTime;
          if (!seeking) { currentTimeRef.current = t; setCurrentTime(t); }
          if (!seeking && t > 0 && t - lastProgressSaveTimeRef.current >= 5) {
            lastProgressSaveTimeRef.current = t;
            saveProgressRef.current(currentIndexRef.current);
          }
          // Crossfade/zerogap trigger (same logic as Audio A)
          const audio = e.currentTarget;
          if (repeatModeRef.current !== 'one' && crossfadeConfig.mode !== 'off' && audio.duration > 0 && !crossfadeTriggeredRef.current && !crossfadeActiveRef.current) {
            const nextIdx = currentIndex + 1;
            if (nextIdx < queue.length || repeatModeRef.current === 'all') {
              const safeNextIdx = nextIdx < queue.length ? nextIdx : 0;
              const threshold = computeTransitionThreshold(crossfadeConfig.mode, clampCrossfadeDuration(crossfadeConfig.duration, audio.duration));
              const remaining = audio.duration - audio.currentTime;
              if (remaining <= threshold && remaining > 0) {
                crossfadeTriggeredRef.current = true;
                if (crossfadeConfig.mode === 'crossfade') {
                  beginCrossfade(safeNextIdx);
                } else {
                  beginZeroGap(safeNextIdx);
                }
              }
            }
          }
        }}
        onDurationChange={e => { if (activeSlotRef.current === 'B' && !currentTrack?.duration) setDuration(e.currentTarget.duration); }}
        onEnded={() => {
          if (activeSlotRef.current !== 'B') return;
          setCurrentTime((prev) => Math.max(prev, duration));
          if (repeatModeRef.current === 'one') {
            const audio = audioBRef.current;
            if (audio) { audio.currentTime = 0; audio.play().catch(() => {}); }
          } else if (crossfadeActiveRef.current && crossfadeConfig.mode === 'zerogap') {
            finalizeZeroGap(currentIndex + 1);
          } else if (!crossfadeActiveRef.current) {
            playNext();
          }
        }}
        onCanPlay={() => { if (activeSlotRef.current === 'B') { setLoading(false); setAudioError(null); } }}
        onWaiting={() => { if (activeSlotRef.current === 'B') setLoading(true); }}
        onError={() => { /* Standby errors are non-fatal */ }}
        preload="auto"
      />

      {headless ? null : (

      <div
        data-hybrid-preview-surface={hybridPreview ? 'player' : undefined}
        style={{
          ...P.bar,
          ...(hybridPreview ? hybridPlayerStyles.bar : {}),
          ...(isVinylMode ? P.barVinyl : {}),
        }}
      >
        {/* Album art */}
        <div style={{
          ...P.albumArtWrap,
          ...(hybridPreview ? hybridPlayerStyles.albumArtWrap : {}),
        }}>
          {currentTrackAlbumArt ? (
            <img
              src={currentTrackAlbumArt}
              alt={currentTrackMeta.album ? `${currentTrackMeta.album} cover` : 'Album cover'}
              style={P.albumArt}
            />
          ) : (
            <div style={P.albumArtPlaceholder} aria-hidden="true" />
          )}
        </div>

        {/* Track info */}
        <div style={P.trackInfo}>
          <div style={P.trackTitle}>{currentTrackTitle}</div>
          {getTranscodeWarning(currentTrack, ffmpegAvailable)
            ? <div style={{ fontSize: 10, color: '#f59e0b', marginTop: 2 }}>⚠ {getTranscodeWarning(currentTrack, ffmpegAvailable)}</div>
            : (
              <div style={P.trackSub}>
                {currentTrackMeta.artist ? (
                  onOpenArtist ? (
                    <button
                      type="button"
                      style={P.trackArtistLink}
                      onClick={() => onOpenArtist(currentTrackMeta.artist)}
                      title={`Open ${currentTrackMeta.artist}`}
                      aria-label={`Open artist ${currentTrackMeta.artist}`}
                    >
                      {currentTrackMeta.artist}
                    </button>
                  ) : (
                    <span style={P.trackSubText}>{currentTrackMeta.artist}</span>
                  )
                ) : (
                  <span style={P.trackSubText}>Unknown artist</span>
                )}
                {currentTrackMeta.album && (
                  <>
                    <span style={P.trackSubText}> · </span>
                    {onOpenAlbum ? (
                      <button
                        type="button"
                        style={P.trackAlbumLink}
                        onClick={() => onOpenAlbum(currentTrackMeta.album, currentTrackMeta.artist || null)}
                        title={`Open album ${currentTrackMeta.album}`}
                        aria-label={`Open album ${currentTrackMeta.album}`}
                      >
                        {currentTrackMeta.album}
                      </button>
                    ) : (
                      <span style={P.trackSubText}>{currentTrackMeta.album}</span>
                    )}
                  </>
                )}
              </div>
            )
          }
        </div>

        {/* Transport */}
        <div style={P.controls}>
          <button style={P.ctrlBtn} onClick={playPrev}><PrevIcon /></button>
          <button style={{ ...P.ctrlBtn, ...P.playBtn }} onClick={() => onStateChange({ ...state, isPlaying: !isPlaying })}>
            {loading ? <span style={{ fontSize: 12, lineHeight: 1 }}>•••</span> : isPlaying ? <PauseIcon /> : <PlayIcon />}
          </button>
          <button style={P.ctrlBtn} onClick={playNext}><NextIcon /></button>
        </div>

                {/* Progress */}
        {isVinylMode ? (
          <div style={P.vinylArea}>
            <VinylTurntable
              albumArtUrl={currentTrackAlbumArt}
              title={currentTrackTitle}
              isPlaying={isPlaying}
              currentTime={currentTime}
              duration={duration || 0}
              seekDisabled={vinylHardcore}
              onSeek={(seconds, commit) => seekToTime(seconds, commit)}
              onSeekStart={() => setSeeking(true)}
              onSeekEnd={(seconds) => {
                seekToTime(seconds, true);
                if (isPlaying) {
                  const active = getActiveAudio();
                  if (active) {
                    if (!vinylHardcore) {
                      triggerVinylNeedleDrop(active, 0.9 * vinylNeedleDropIntensity);
                    }
                    active.play().catch(() => {});
                  }
                }
              }}
            />
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 120 }}>
              <span style={P.timeLabel}>{fmt(currentTime)} / {fmt(duration)}</span>
              <span style={{ ...P.timeLabel, color: 'var(--accent)' }}>
                Vinyl Mode{vinylNeedleDrop ? ' + Needle Drop' : ''}
              </span>
              {vinylHardcore && <span style={P.timeLabel}>Hardcore: seeking/jumping disabled</span>}
            </div>
          </div>
        ) : (
          /* Wrapper takes the progress-area flex slot; stacks badge row, panel, waveform vertically */
          <div
            data-testid="player-progress-area"
            style={{
              flex: '1 1 0',
              width: PLAYER_LAYOUT.progressWidth,
              maxWidth: PLAYER_LAYOUT.progressMaxWidth,
              minWidth: PLAYER_LAYOUT.progressMinWidth,
              display: 'flex',
              flexDirection: 'column',
              gap: 2,
            }}
          >
            {/* Sonic Fingerprint badge row */}
            {sonicFingerprintChecked && (
              <div
                data-testid="sonic-fingerprint-badge-row"
                style={{ display: 'flex', alignItems: 'center', gap: 5, flexWrap: 'wrap' }}
              >
                {sonicFingerprint ? (
                  <>
                    {sonicFingerprint.bpmDetected != null && (
                      <span
                        style={P.fpBadge}
                        data-testid="fp-badge-bpm"
                        title="Beats per minute — detected by AI stem analysis"
                      >
                        ♩ {Math.round(sonicFingerprint.bpmDetected)} BPM
                      </span>
                    )}
                    <span
                      style={P.fpBadge}
                      data-testid="fp-badge-energy"
                      title="Energy score — overall intensity derived from stem activity (0–100%)"
                    >
                      ⚡ {Math.round(sonicFingerprint.energyScoreRefined * 100)}%
                    </span>
                    <span
                      style={P.fpBadge}
                      data-testid="fp-badge-confidence"
                      title="Analysis confidence — how reliable the AI stem analysis is (values below 30% indicate synthetic fallback data; real Demucs analysis required for full detail)"
                    >
                      ◎ {Math.round(sonicFingerprint.confidence * 100)}%
                    </span>
                    <button
                      style={{
                        ...P.fpBadge,
                        cursor: 'pointer',
                        ...(showSonicFingerprint ? hybridAudioPanelStyles.badgeAccent : {}),
                      }}
                      data-testid="fp-toggle-button"
                      onClick={() => setShowSonicFingerprint(v => !v)}
                      title="Show or hide the full Sonic Fingerprint panel — stem heatmap, energy curve, and section map"
                      aria-label={showSonicFingerprint ? 'Hide Sonic Fingerprint' : 'Show Sonic Fingerprint'}
                    >
                      {showSonicFingerprint ? 'Hide Fingerprint' : 'Sonic Fingerprint ✦'}
                    </button>
                  </>
                ) : (
                  <span
                    style={{ ...P.fpBadge, opacity: 0.45 }}
                    data-testid="fp-badge-unavailable"
                    title="This track has not been deep-analyzed yet. Run BoogieMix High Quality on a playlist containing this track to generate its Sonic Fingerprint."
                  >
                    ✦ No Sonic Fingerprint
                  </span>
                )}
              </div>
            )}

            {/* Expandable Sonic Fingerprint panel — rendered as a fixed popup above the player bar */}
            {sonicFingerprint && showSonicFingerprint && (
              <div style={{
                position: 'fixed',
                bottom: playerDockHeight + DESKTOP_PLAYER_POPUP_GAP,
                left: 12,
                right: 12,
                zIndex: 99,
              }}>
                <SonicFingerprintPanel
                  fingerprint={sonicFingerprint}
                  waveformPoints={waveformPoints}
                  waveformStatus={waveformStatus}
                  duration={duration || 0}
                  currentTime={currentTime}
                  onSeek={(time) => seekToTime(time, false)}
                  onSeekStart={() => setSeeking(true)}
                  onSeekEnd={(time) => seekToTime(time, true)}
                  onClose={() => setShowSonicFingerprint(false)}
                />
              </div>
            )}

            {/* Standard progress row: time + waveform/slider + time */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              {audioError
                ? <span style={{ color: '#ef4444', fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>? {audioError}</span>
                : <>
                    <span style={P.timeLabel}>{fmt(currentTime)}</span>
                    {showWaveformProgress ? (
                      <WaveformBar
                        points={waveformPoints}
                        duration={duration || 0}
                        currentTime={currentTime}
                        status={waveformStatus}
                        onSeek={(time) => seekToTime(time, false)}
                        onSeekStart={() => setSeeking(true)}
                        onSeekEnd={(time) => seekToTime(time, true)}
                        sections={sonicFingerprint?.sectionJson}
                        transitionWindows={sonicFingerprint?.transitionWindowsJson}
                      />
                    ) : (
                      <Slider value={currentTime} max={duration || 1}
                        onChange={v => { seekValueRef.current = v; setCurrentTime(v); }}
                        onSeekStart={() => setSeeking(true)}
                        onSeekEnd={() => { setSeeking(false); const a = getActiveAudio(); if (a) a.currentTime = seekValueRef.current; }} />
                    )}
                    <span style={P.timeLabel}>{fmt(duration)}</span>
                  </>
              }
            </div>
          </div>
        )}

        <div style={P.rightCluster} data-testid="player-right-cluster">
          {/* Visualizer */}
          {audioReady && (
            <StereoVU analyser={analyserForVu} isPlaying={isPlaying} mode={vizMode} onToggleMode={toggleMode} />
          )}

          {/* Playback modes + Volume + Queue */}
          <div style={P.rightControls} data-testid="player-right-controls">
            <div style={P.modeControls}>
              {/* Shuffle */}
              <button
                style={{ ...P.ctrlBtn, color: shuffled ? PLAYER_THEME_TOKENS.accent : PLAYER_THEME_TOKENS.textMuted, ...(shuffled ? P.ctrlBtnActive : {}) }}
                title="Shuffle queue"
                disabled={isVinylMode}
                onClick={toggleShuffle}
              ><ShuffleIcon /></button>
              <button
                style={{ ...P.ctrlBtn, color: repeatMode !== 'off' ? PLAYER_THEME_TOKENS.accent : PLAYER_THEME_TOKENS.textMuted, ...(repeatMode !== 'off' ? P.ctrlBtnActive : {}) }}
                title={repeatMode === 'off' ? 'Repeat off' : repeatMode === 'one' ? 'Repeat track' : 'Repeat queue'}
                aria-label={repeatMode === 'off' ? 'Repeat off' : repeatMode === 'one' ? 'Repeat track' : 'Repeat queue'}
                onClick={cycleRepeatMode}
              >
                {repeatMode === 'one' ? <RepeatOneIcon /> : <RepeatAllIcon />}
              </button>
              <button
                style={{ ...P.ctrlBtn, color: lyricsOpen ? PLAYER_THEME_TOKENS.accent : PLAYER_THEME_TOKENS.textMuted, ...(lyricsOpen ? P.ctrlBtnActive : {}) }}
                title="Show lyrics"
                aria-label="Show lyrics"
                onClick={() => {
                  setLyricsOpen((open) => !open);
                  setLyricsError(null);
                }}
              ><LyricsIcon /></button>
            </div>
            {/* Divider */}
            <div style={{ width: 1, height: 20, backgroundColor: PLAYER_THEME_TOKENS.border, flexShrink: 0 }} />
            {/* Volume icon */}
            <div style={P.volumeStack}>
              <button style={P.ctrlBtn} onClick={() => setMuted(m => !m)}><VolumeIcon muted={muted} /></button>
              <button
                style={{ ...P.ctrlBtn, color: eqOpen ? PLAYER_THEME_TOKENS.accent : PLAYER_THEME_TOKENS.textMuted, ...(eqOpen ? P.ctrlBtnActive : {}) }}
                title="Equalizer"
                aria-label="Equalizer"
                onClick={() => setEqOpen((open) => !open)}
              >
                <EqIcon />
              </button>
            </div>
            {/* Vertical volume slider */}
            <Slider
              value={muted ? 0 : volume} max={1}
              onChange={v => { setVolume(v); setMuted(false); }}
              color={PLAYER_THEME_TOKENS.textMuted} thin
              vertical verticalHeight={60}
            />
            {eqOpen && (
              <div style={{ ...P.eqPopup, bottom: playerDockHeight + DESKTOP_PLAYER_POPUP_GAP }} ref={eqPopupRef} role="dialog" aria-label="Equalizer">
                <div style={P.eqHeader}>
                  <strong style={{ fontSize: 12 }}>Equalizer</strong>
                  <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11 }}>
                    <span style={{ color: PLAYER_THEME_TOKENS.textMuted }}>Auto EQ</span>
                    <input type="checkbox" checked={autoEqEnabled} onChange={(e) => setAutoEqEnabled(e.currentTarget.checked)} />
                  </label>
                  <button style={P.eqCloseBtn} onClick={() => setEqOpen(false)} aria-label="Close equalizer"><CloseIcon /></button>
                </div>
                {autoEqEnabled && (
                  <div style={{ fontSize: 10, color: PLAYER_THEME_TOKENS.textMuted }}>
                    Auto EQ active - preset: <strong style={{ color: PLAYER_THEME_TOKENS.text }}>{autoEqCurrentPreset}</strong>
                    <span> {'->'} <strong style={{ color: PLAYER_THEME_TOKENS.text }}>{mapGraphicProfileToParametricPreset(autoEqCurrentPreset)}</strong></span>
                  </div>
                )}
                <ParametricEqEditor
                  bands={parametricBands}
                  profile={parametricProfile}
                  customProfiles={customParametricProfiles}
                  autoEqEnabled={autoEqEnabled}
                  newProfileName={newParametricProfileName}
                  onBandsChange={changeParametricBands}
                  onProfileChange={changeParametricProfile}
                  onNewProfileNameChange={setNewParametricProfileName}
                  onSaveProfile={saveParametricProfile}
                  onDeleteProfile={deleteParametricProfile}
                  accentColor={PLAYER_THEME_TOKENS.accent}
                />
              </div>
            )}
            {/* Queue */}
            <button
              style={{ ...P.ctrlBtn, color: showQueue ? PLAYER_THEME_TOKENS.accent : PLAYER_THEME_TOKENS.textMuted, ...(showQueue ? P.ctrlBtnActive : {}) }}
              onClick={() => setShowQueue(q => !q)}
              title="Playback queue"
              aria-label="Playback queue"
            >
              <QueueIcon />
            </button>
          </div>
          {lyricsOpen && (
            <div
              style={{ ...P.lyricsPopup, bottom: playerDockHeight + DESKTOP_PLAYER_POPUP_GAP }}
              role="dialog"
              aria-label="Lyrics popup"
            >
              <div style={P.lyricsHeader}>
                <strong style={hybridAudioPanelStyles.title}>Lyrics</strong>
                <div style={hybridAudioPanelStyles.headerActions}>
                  {lyricsSource && <span style={P.lyricsSource}>Source: {lyricsSource}</span>}
                  <label style={P.karaokeToggle}>
                    <input
                      type="checkbox"
                      checked={karaokeMode}
                      onChange={(e) => setKaraokeMode(e.target.checked)}
                      disabled={!lyricsSynced.length}
                    />
                    <span>Karaoke</span>
                  </label>
                  <button
                    type="button"
                    aria-label="Close lyrics"
                    onClick={() => setLyricsOpen(false)}
                    style={{ ...hybridControlStyles.iconButton, width: 30, minWidth: 30, height: 30 }}
                  >
                    <CloseIcon />
                  </button>
                </div>
              </div>
              <div style={P.lyricsBody} ref={lyricsBodyRef}>
                {lyricsLoading && <div style={P.lyricsMuted}>Loading lyrics…</div>}
                {!lyricsLoading && lyricsError && <div style={P.lyricsError}>{lyricsError}</div>}
                {!lyricsLoading && !lyricsError && karaokeMode && lyricsSynced.length > 0 && (
                  <div style={P.syncedLyricsWrap}>
                    {lyricsSynced.map((line, idx) => (
                      <div
                        key={`${line.time}-${idx}`}
                        ref={(el) => { syncedLineRefs.current[idx] = el; }}
                        style={{
                          ...P.syncedLine,
                          ...(idx === activeSyncedLyricIndex ? P.syncedLineActive : {}),
                        }}
                      >
                        {line.text}
                      </div>
                    ))}
                  </div>
                )}
                {!lyricsLoading && !lyricsError && (!karaokeMode || lyricsSynced.length === 0) && (
                  <pre style={P.lyricsText}>{lyricsText || 'Lyrics not available.'}</pre>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
      )}
    </>
  );
}

const P: Record<string, React.CSSProperties> = {
  bar: {
    height: DESKTOP_PLAYER_DOCK_HEIGHT,
    minHeight: DESKTOP_PLAYER_DOCK_HEIGHT,
    flexShrink: 0,
    background: [
      `radial-gradient(circle at top left, color-mix(in srgb, ${PLAYER_THEME_TOKENS.accent} 18%, transparent) 0%, transparent 32%)`,
      `linear-gradient(180deg, color-mix(in srgb, ${PLAYER_THEME_TOKENS.surface} 96%, ${PLAYER_THEME_TOKENS.bg}) 0%, color-mix(in srgb, ${PLAYER_THEME_TOKENS.surface} 88%, ${PLAYER_THEME_TOKENS.bg}) 100%)`,
    ].join(','),
    borderTop: `1px solid color-mix(in srgb, ${PLAYER_THEME_TOKENS.border} 78%, transparent)`,
    display: 'flex', alignItems: 'center', gap: 12, padding: '0 20px',
    zIndex: 100, fontFamily: PLAYER_THEME_TOKENS.font,
    boxShadow: '0 -16px 30px rgba(0,0,0,0.16)',
  },
  barVinyl: {
    height: DESKTOP_VINYL_PLAYER_DOCK_HEIGHT,
    minHeight: DESKTOP_VINYL_PLAYER_DOCK_HEIGHT,
  },
  albumArtWrap: {
    width: 62,
    height: 62,
    borderRadius: 16,
    overflow: 'hidden',
    border: `1px solid color-mix(in srgb, ${PLAYER_THEME_TOKENS.border} 72%, transparent)`,
    flexShrink: 0,
    backgroundColor: PLAYER_THEME_TOKENS.surface,
    boxShadow: '0 14px 24px rgba(0,0,0,0.14)',
  },
  albumArt: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  albumArtPlaceholder: {
    width: '100%',
    height: '100%',
    background: `linear-gradient(135deg, ${PLAYER_THEME_TOKENS.border}, color-mix(in srgb, ${PLAYER_THEME_TOKENS.border} 65%, transparent))`,
  },
  trackInfo: {
    flex: '0 1 auto',
    width: 'min(42ch, 30vw)',
    maxWidth: PLAYER_LAYOUT.trackInfoMaxWidth,
    minWidth: PLAYER_LAYOUT.trackInfoMinWidth,
    overflow: 'hidden',
  },
  trackTitle:   { fontSize: 14, fontWeight: 700, color: PLAYER_THEME_TOKENS.text, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackSub:     { display: 'flex', alignItems: 'center', fontSize: 11, marginTop: 4, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackSubText: { color: PLAYER_THEME_TOKENS.textMuted, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackArtistLink: {
    background: 'transparent',
    border: 'none',
    padding: 0,
    margin: 0,
    fontSize: 11,
    fontFamily: 'inherit',
    color: PLAYER_THEME_TOKENS.accent,
    textDecoration: 'underline',
    cursor: 'pointer',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    maxWidth: '100%',
  },
  trackAlbumLink: {
    background: 'transparent',
    border: 'none',
    padding: 0,
    margin: 0,
    fontSize: 11,
    fontFamily: 'inherit',
    color: PLAYER_THEME_TOKENS.accent,
    textDecoration: 'underline',
    cursor: 'pointer',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    maxWidth: '100%',
  },
  controls:     { display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 },
  ctrlBtn:      { background: 'rgba(255,255,255,0.03)', border: `1px solid color-mix(in srgb, ${PLAYER_THEME_TOKENS.border} 62%, transparent)`, color: PLAYER_THEME_TOKENS.textMuted, cursor: 'pointer', padding: 8, borderRadius: 999, display: 'flex', alignItems: 'center' },
  ctrlBtnActive: { background: `color-mix(in srgb, ${PLAYER_THEME_TOKENS.accent} 18%, transparent)`, boxShadow: `inset 0 1px 3px color-mix(in srgb, ${PLAYER_THEME_TOKENS.accent} 30%, transparent)` },
  playBtn:      { backgroundColor: PLAYER_THEME_TOKENS.accent, color: '#fff', borderRadius: '50%', width: 44, height: 44, justifyContent: 'center', padding: 0, transition: 'background-color 300ms ease, box-shadow 300ms ease', boxShadow: '0 0 18px color-mix(in srgb, var(--accent-primary, var(--accent)) 35%, transparent)' },
  progressArea: {
    flex: '1 1 0',
    width: PLAYER_LAYOUT.progressWidth,
    maxWidth: PLAYER_LAYOUT.progressMaxWidth,
    minWidth: PLAYER_LAYOUT.progressMinWidth,
    display: 'flex',
    alignItems: 'center',
    gap: 10,
  },
  fpBadge: {
    ...hybridAudioPanelStyles.badge,
    whiteSpace: 'nowrap' as const,
    fontVariantNumeric: 'tabular-nums',
    cursor: 'default',
    fontFamily: 'inherit',
  },
  vinylArea: {
    flex: '1 1 0',
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    minWidth: 220,
  },
  timeLabel:    { fontSize: 11, color: PLAYER_THEME_TOKENS.textMuted, flexShrink: 0, fontVariantNumeric: 'tabular-nums', minWidth: 36 },
  rightCluster: { display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0, marginLeft: 'auto' },
  rightControls:{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0, position: 'relative' },
  volumeStack: { display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2 },
  modeControls: { display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4, flexShrink: 0 },
  eqPopup: {
    ...hybridAudioPanelStyles.popup,
    position: 'fixed',
    right: 20,
    width: 560,
    zIndex: 220,
    display: 'flex',
    flexDirection: 'column',
    padding: 12,
    gap: 12,
  },
  eqHeader: {
    ...hybridAudioPanelStyles.header,
    minHeight: 38,
    margin: '-12px -12px 0',
    padding: '8px 8px 8px 14px',
  },
  eqCloseBtn: {
    ...hybridControlStyles.iconButton,
    width: 30,
    minWidth: 30,
    height: 30,
  },
  eqProfileLabel: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  },
  eqCustomRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  },
  eqInput: {
    flex: 1,
    minWidth: 140,
    backgroundColor: 'var(--bg)',
    border: `1px solid ${PLAYER_THEME_TOKENS.border}`,
    color: PLAYER_THEME_TOKENS.text,
    borderRadius: 6,
    padding: '5px 8px',
    fontSize: 12,
    fontFamily: 'inherit',
    outline: 'none',
  },
  eqActionBtn: {
    backgroundColor: 'var(--bg)',
    border: `1px solid ${PLAYER_THEME_TOKENS.border}`,
    color: PLAYER_THEME_TOKENS.text,
    borderRadius: 6,
    padding: '5px 10px',
    fontSize: 12,
    fontFamily: 'inherit',
    cursor: 'pointer',
  },
  eqStatus: {
    fontSize: 10,
    color: PLAYER_THEME_TOKENS.textMuted,
  },
  eqSelect: {
    backgroundColor: 'var(--bg)',
    border: `1px solid ${PLAYER_THEME_TOKENS.border}`,
    color: PLAYER_THEME_TOKENS.text,
    borderRadius: 6,
    padding: '5px 8px',
    fontSize: 12,
    fontFamily: 'inherit',
  },
  eqBands: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 8,
  },
  eqBandCol: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 4,
    minWidth: 44,
  },
  eqBandLabel: {
    fontSize: 10,
    color: PLAYER_THEME_TOKENS.textMuted,
  },
  eqDbLabel: {
    fontSize: 10,
    color: PLAYER_THEME_TOKENS.textMuted,
    minWidth: 34,
    textAlign: 'center',
  },
  lyricsPopup: {
    ...hybridAudioPanelStyles.popup,
    position: 'fixed',
    right: 20,
    bottom: DESKTOP_PLAYER_DOCK_HEIGHT + DESKTOP_PLAYER_POPUP_GAP,
    width: 360,
    maxHeight: 320,
    zIndex: 200,
    display: 'flex',
    flexDirection: 'column',
  },
  lyricsHeader: {
    ...hybridAudioPanelStyles.header,
    color: PLAYER_THEME_TOKENS.text,
  },
  lyricsSource: {
    ...hybridAudioPanelStyles.badge,
  },
  lyricsBody: {
    ...hybridAudioPanelStyles.body,
  },
  lyricsText: {
    margin: 0,
    whiteSpace: 'pre-wrap',
    fontFamily: PLAYER_THEME_TOKENS.font,
    fontSize: 11,
    color: PLAYER_THEME_TOKENS.text,
    lineHeight: 1.45,
  },
  lyricsMuted: {
    fontSize: 11,
    color: PLAYER_THEME_TOKENS.textMuted,
  },
  lyricsError: {
    fontSize: 11,
    color: 'var(--danger)',
  },
  karaokeToggle: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    fontSize: 10,
    color: PLAYER_THEME_TOKENS.textMuted,
  },
  syncedLyricsWrap: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  syncedLine: {
    padding: '7px 9px',
    borderRadius: 8,
    fontSize: 12,
    color: PLAYER_THEME_TOKENS.textMuted,
    lineHeight: 1.4,
    transition: 'color 120ms ease, background 120ms ease',
  },
  syncedLineActive: {
    color: PLAYER_THEME_TOKENS.accent,
    fontWeight: 700,
    background: 'var(--accent-soft)',
  },
};
