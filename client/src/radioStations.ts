/**
 * Internet radio stations: shared by every user, edited by admins. Playing one queues a single live item
 * whose stream comes through the server's relay (`/api/radio-stations/{id}/stream`), so the player's audio
 * graph works without CORS. Live items have no length: no seeking, crossfade, preload, play counts or scrobbles.
 */

import { api } from './api';
import type { ClientEntityId, Track } from './types';

/** `GET /api/radio-stations` entry. */
export interface RadioStation {
  id: string;
  name: string;
  streamUrl: string;
  homepageUrl: string | null;
}

/** Fields an admin edits. */
export interface RadioStationInput {
  name: string;
  streamUrl: string;
  homepageUrl: string;
}

export const RADIO_TRACK_PREFIX = 'radio:';

/** True for the synthetic track of a radio station. */
export function isRadioTrackId(id: ClientEntityId | null | undefined): boolean {
  return typeof id === 'string' && id.startsWith(RADIO_TRACK_PREFIX);
}

/** False for synthetic playables (a BoogieMix render `boogiemix:…` or a station `radio:…`) that have no `tracks` row. */
export function isLibraryTrackId(id: ClientEntityId | number | null | undefined): boolean {
  if (id == null) return false;
  const text = String(id);
  return text.length > 0 && !text.startsWith('boogiemix:') && !text.startsWith(RADIO_TRACK_PREFIX);
}

/** True for a playable item with no fixed length (a radio station). */
export function isLiveTrack(track: Pick<Track, 'id' | 'live'> | null | undefined): boolean {
  return Boolean(track && (track.live || isRadioTrackId(track.id)));
}

/** The queue item for a station. */
export function radioStationTrack(station: RadioStation): Track {
  return {
    id: `${RADIO_TRACK_PREFIX}${station.id}`,
    file_name: station.name,
    file_size: null,
    format: null,
    duration: null,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: station.name,
    artist: 'Internet radio',
    album: null,
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    album_id: null,
    scanned_at: '',
    stream_url_override: api.radioStations.streamUrl(station.id),
    live: true,
  };
}

/** Client-side check mirroring the server: an absolute http(s) URL with a host. */
export function isHttpUrl(value: string): boolean {
  const v = value.trim();
  if (!/^https?:\/\//i.test(v) || /\s/.test(v)) return false;
  try {
    return new URL(v).hostname.length > 0;
  } catch {
    return false;
  }
}

/** First problem with station fields, or null when they can be saved. */
export function radioStationProblem(input: RadioStationInput): string | null {
  const name = input.name.trim();
  if (!name || name.length > 200) return 'Station name must be 1–200 characters';
  if (!isHttpUrl(input.streamUrl)) return 'Stream URL must be an http:// or https:// address';
  if (input.homepageUrl.trim() && !isHttpUrl(input.homepageUrl)) return 'Homepage must be an http:// or https:// address';
  return null;
}
