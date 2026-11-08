/**
 * Tells the app that album, artist or track metadata was saved, so anything worked out from the old tags
 * (such as the player's Auto EQ choices) can be worked out again.
 */

export const METADATA_SAVED_EVENT = 'boogiebox:metadata-saved';

export function announceMetadataSaved(): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(METADATA_SAVED_EVENT));
}
