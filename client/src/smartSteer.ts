/**
 * Smart playlist steering pills: the API contract, the approved wording (wip/steer-approved-copy.md), and the
 * rules for turning pills on and off. Shared by the desktop and mobile views.
 */

import type { PlayerState } from './components/Player';
import type { ClientEntityId, PlaylistSnapshot, QueueSource, Track } from './types';

export type SteerDirection = 'more' | 'less' | 'deeper';
export type SteerSignal = 'favourites' | 'popular' | 'new' | 'faster' | 'slower' | 'energetic' | 'calmer';

/** One pill as stored in a smart playlist's definition. */
export interface SmartSteer {
  kind: 'style' | 'mood' | 'signal';
  tag?: string;
  bucket?: string;
  direction?: SteerDirection;
  signal?: SteerSignal;
}

/** A pill to offer: send it back without `count` to turn it on. */
export interface SteerPill extends SmartSteer {
  count: number;
}

export type PopularSource = 'lastfm' | 'local';

/** `GET /api/playlists/{id}/steer`. */
export interface SteerView {
  active: SmartSteer[];
  pills: SteerPill[];
  pool: number;
  tagged: number;
  fewTags: boolean;
  popularSource: PopularSource;
  /** Tracks in the playlist only because of the pills. */
  addedTrackIds: ClientEntityId[];
  editRevision: number;
}

/** `PUT /api/playlists/{id}/steer`. */
export interface SteerUpdate {
  steer: SteerView;
  snapshot: PlaylistSnapshot;
}

export const MAX_ACTIVE_PILLS = 3;

export const STEER_COPY = {
  limit: 'Up to 3 at a time',
  fewTags: 'More pills appear once BoogieBox finds style tags for these tracks.',
  queueStale: 'Your queue still has the old tracks.',
  updateNext: "Update what's next",
  notNow: 'Not now',
  failed: "Couldn't update this playlist. The pill was turned back off.",
  tryAgain: 'Try again',
  reset: 'Reset',
  group: 'Steer this playlist',
  clear: 'Clear',
} as const;

function directionWord(direction: SteerDirection | undefined): string {
  if (direction === 'less') return 'Less';
  return 'More';
}

/** The label on a pill. */
export function pillLabel(steer: SmartSteer, popularSource: PopularSource = 'local'): string {
  if (steer.kind === 'style') {
    if (steer.direction === 'deeper') return `Go deeper into ${steer.tag}`;
    return `${directionWord(steer.direction)} ${steer.tag}`;
  }
  if (steer.kind === 'mood') return `${directionWord(steer.direction)} ${steer.bucket}`;
  switch (steer.signal) {
    case 'favourites': return 'My favourites';
    case 'popular': return popularSource === 'lastfm' ? 'Popular' : 'Most played here';
    case 'new': return 'Something new';
    case 'faster': return 'Faster';
    case 'slower': return 'Slower';
    case 'energetic': return 'More energetic';
    case 'calmer': return 'Calmer';
    default: return String(steer.signal ?? '');
  }
}

function tracksWord(count: number): string {
  return count === 1 ? '1 track' : `${count.toLocaleString()} tracks`;
}

/** The tooltip on a pill being offered. */
export function pillTip(pill: SteerPill, popularSource: PopularSource = 'local'): string | undefined {
  if (pill.kind === 'style' && pill.direction === 'deeper') {
    return `${tracksWord(pill.count)} in narrower kinds of ${pill.tag}`;
  }
  if (pill.kind !== 'signal') return tracksWord(pill.count);
  switch (pill.signal) {
    case 'popular':
      return popularSource === 'lastfm' ? tracksWord(pill.count) : `${tracksWord(pill.count)} played on this server`;
    case 'favourites': return tracksWord(pill.count);
    case 'new': return 'Tracks you have not played yet';
    case 'energetic':
    case 'calmer': return 'Uses deep analysis energy';
    default: return undefined;
  }
}

/** Icon path for a pill (24 × 24, stroked). One shared icon for every style: their number is unbounded. */
export function pillIcon(steer: SmartSteer): string {
  if (steer.kind === 'style') return 'M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8zM7.5 7.5h.01';
  if (steer.kind === 'mood') return 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM8 15s1.5-2 4-2 4 2 4 2M9 9h.01M15 9h.01';
  switch (steer.signal) {
    case 'favourites': return 'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8l1 1L12 21l7.8-7.6 1-1a5.5 5.5 0 0 0 0-7.8z';
    case 'popular': return 'M3 17l6-6 4 4 8-8M15 7h6v6';
    case 'new': return 'M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z';
    case 'faster':
    case 'slower': return 'M3 12h4l3-8 4 16 3-8h4';
    case 'calmer': return 'M2 12c2.5-4 5-4 7.5 0s5 4 7.5 0 3.5-3 5-2';
    default: return 'M13 2 4 14h7l-1 8 9-12h-7z';
  }
}

export const STEER_CHANGED_ICON = 'M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5';

/** "N tracks changed". */
export function changedLabel(count: number): string {
  return count === 1 ? '1 track changed' : `${count.toLocaleString()} tracks changed`;
}

/** A pill stripped to what the server stores. */
export function steerOnly(pill: SmartSteer): SmartSteer {
  const out: SmartSteer = { kind: pill.kind };
  if (pill.tag !== undefined) out.tag = pill.tag;
  if (pill.bucket !== undefined) out.bucket = pill.bucket;
  if (pill.direction !== undefined) out.direction = pill.direction;
  if (pill.signal !== undefined) out.signal = pill.signal;
  return out;
}

/** Two pills about the same thing. The server refuses both on at once. */
export function sameTarget(a: SmartSteer, b: SmartSteer): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'style') return a.tag === b.tag;
  if (a.kind === 'mood') return a.bucket === b.bucket;
  const group = (signal?: SteerSignal) => {
    if (signal === 'faster' || signal === 'slower') return 'bpm';
    if (signal === 'energetic' || signal === 'calmer') return 'energy';
    return signal;
  };
  return group(a.signal) === group(b.signal);
}

/** The pills on after tapping `pill`: off if it was on, replacing a pill about the same thing otherwise. */
export function togglePill(active: SmartSteer[], pill: SmartSteer): SmartSteer[] {
  const existing = active.find(other => sameTarget(other, pill));
  const others = active.filter(other => !sameTarget(other, pill));
  if (existing && existing.direction === pill.direction && existing.signal === pill.signal) return others;
  return others.length >= MAX_ACTIVE_PILLS ? active : [...others, steerOnly(pill)];
}

/** Whether the player's queue came from this playlist. */
export function isQueueFrom(source: QueueSource | null | undefined, playlistId: ClientEntityId): boolean {
  return source?.type === 'playlist' && String(source.id) === String(playlistId);
}

/**
 * The player state with what comes next replaced by `tracks`. The current track and what already played stay,
 * and nothing restarts. Leaves the state alone unless the queue came from this playlist.
 */
export function withUpcoming(state: PlayerState, playlistId: ClientEntityId, tracks: Track[]): PlayerState {
  if (!isQueueFrom(state.queueSource, playlistId)) return state;
  const kept = state.queue.slice(0, state.currentIndex + 1);
  const heard = new Set(kept.map(track => String(track.id)));
  return { ...state, queue: [...kept, ...tracks.filter(track => !heard.has(String(track.id)))] };
}
