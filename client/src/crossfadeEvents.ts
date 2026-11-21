/**
 * Tells the player that the global crossfade setting (Settings › Track Transitions) was saved, so the song playing
 * now follows it instead of waiting for the next album or playlist.
 */

export const CROSSFADE_SAVED_EVENT = 'boogiebox:crossfade-saved';

export function announceCrossfadeSaved(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(CROSSFADE_SAVED_EVENT));
}
