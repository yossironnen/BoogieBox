/**
 * Defines mobile Mobile Shell behavior for the BoogieBox React client.
 */

import type { AppSettings, Artist, Album, AuthUser, ClientEntityId, Library, Playlist, PlaylistTrack, QueueSource, Track } from '../types';
import type { PlaybackSnapshot, PlayerState } from '../components/Player';
import type { EntityId } from '../entityId';

/** Mobile Tab Id is part of this module's public API. */
export type MobileTabId = 'home' | 'browse' | 'search' | 'playlists' | 'now-playing';

/** Mobile Active Video is part of this module's public API. */
export interface MobileActiveVideo {
  title: string;
  mediaFileId: ClientEntityId;
  resumeSeconds?: number | null;
}

/** Mobile Shared Props is part of this module's public API. */
export interface MobileSharedProps {
  currentUser: AuthUser;
  libraries: Library[];
  settings: AppSettings;
  ffmpegAvailable: boolean | null;
  playerState: PlayerState;
  playbackSnapshot: PlaybackSnapshot | null;
  openPlaylistId: EntityId | null;
  onPlaybackStateChange: (state: PlayerState) => void;
  onPlayTrack: (track: Track, allTracks?: Track[], source?: QueueSource) => void;
  onAddToQueue: (track: Track) => void;
  onConsumeOpenPlaylist: () => void;
}

/** Mobile Browse Selection is part of this module's public API. */
export interface MobileBrowseSelection {
  artist: Artist | null;
  album: Album | null;
  tracks: Track[];
}

/** Mobile Playlist Selection is part of this module's public API. */
export interface MobilePlaylistSelection {
  playlist: Playlist | null;
  tracks: PlaylistTrack[];
}
