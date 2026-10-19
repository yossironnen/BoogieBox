/**
 * Favourites (hearts): the signed-in user's favourite tracks, albums and artists. They are the same data as
 * the stars Subsonic apps set. A small shared store loads the id lists once, so any surface (context menus,
 * Browse, Home, mobile) can show and toggle a heart without its own request.
 */

import { useSyncExternalStore } from 'react';
import { api } from './api';
import type { Album, Artist, ClientEntityId, Track } from './types';

export type FavouriteKind = 'track' | 'album' | 'artist';

/** `GET /api/favourites/ids`. */
export interface FavouriteIds {
  tracks: string[];
  albums: string[];
  artists: string[];
}

/** `GET /api/favourites`: full rows, newest first. */
export interface Favourites {
  artists: Artist[];
  albums: Album[];
  tracks: Track[];
}

type Listener = () => void;

const sets: Record<FavouriteKind, Set<string>> = {
  track: new Set(),
  album: new Set(),
  artist: new Set(),
};
const listeners = new Set<Listener>();
let version = 0;
let loading: Promise<void> | null = null;
let loaded = false;

function emit() {
  version += 1;
  listeners.forEach((l) => l());
}

function fill(ids: FavouriteIds) {
  sets.track = new Set(ids.tracks.map(String));
  sets.album = new Set(ids.albums.map(String));
  sets.artist = new Set(ids.artists.map(String));
}

export const favouritesStore = {
  subscribe(listener: Listener): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },

  getVersion(): number {
    return version;
  },

  /** Loads the id lists (once, unless `force`). Failures leave the store empty. */
  load(force = false): Promise<void> {
    if (loaded && !force) return Promise.resolve();
    if (loading && !force) return loading;
    loading = Promise.resolve()
      .then(() => api.favourites.ids())
      .then((ids) => {
        fill(ids);
        loaded = true;
        emit();
      })
      .catch(() => {})
      .finally(() => { loading = null; });
    return loading;
  },

  has(kind: FavouriteKind, id: ClientEntityId | null | undefined): boolean {
    return id != null && sets[kind].has(String(id));
  },

  count(kind: FavouriteKind): number {
    return sets[kind].size;
  },

  /** Adds or removes a favourite (optimistic; reverted when the server refuses). Returns the new state. */
  async set(kind: FavouriteKind, id: ClientEntityId, on: boolean): Promise<boolean> {
    const key = String(id);
    const before = sets[kind].has(key);
    if (before === on) return on;
    if (on) sets[kind].add(key); else sets[kind].delete(key);
    emit();
    try {
      if (on) await api.favourites.add(kind, key); else await api.favourites.remove(kind, key);
      return on;
    } catch (err) {
      if (before) sets[kind].add(key); else sets[kind].delete(key);
      emit();
      throw err;
    }
  },

  toggle(kind: FavouriteKind, id: ClientEntityId): Promise<boolean> {
    return favouritesStore.set(kind, id, !favouritesStore.has(kind, id));
  },

  /** Forgets everything (sign-out, user switch). */
  reset() {
    sets.track = new Set();
    sets.album = new Set();
    sets.artist = new Set();
    loaded = false;
    loading = null;
    emit();
  },
};

/** Re-renders when favourites change; returns the store's lookup and toggle helpers. */
export function useFavourites() {
  useSyncExternalStore(favouritesStore.subscribe, favouritesStore.getVersion, favouritesStore.getVersion);
  return {
    has: favouritesStore.has,
    count: favouritesStore.count,
    toggle: favouritesStore.toggle,
    set: favouritesStore.set,
  };
}
