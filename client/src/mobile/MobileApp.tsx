/**
 * Defines mobile Mobile App behavior for the BoogieBox React client.
 */

import React, { useEffect, useState } from 'react';
import Player, { type PlaybackSnapshot } from '../components/Player';
import { api } from '../api';
import type { MobileBrowseSelection, MobilePlaylistSelection, MobileSharedProps, MobileTabId } from './mobileShell';
import MobileMiniPlayer from './components/MobileMiniPlayer';
import MobileTabBar from './components/MobileTabBar';
import MobileBrowseView from './views/MobileBrowseView';
import MobileHomeView from './views/MobileHomeView';
import MobileNowPlayingView from './views/MobileNowPlayingView';
import MobilePlaylistsView from './views/MobilePlaylistsView';
import MobileSearchView from './views/MobileSearchView';
import { phase2 } from '../uiPhase2';

/** Mobile App is part of this module's public API. */
export default function MobileApp(props: MobileSharedProps) {
  const [tab, setTab] = useState<MobileTabId>('home');
  const [browseSelection, setBrowseSelection] = useState<MobileBrowseSelection>({ artist: null, album: null, tracks: [] });
  const [playlistSelection, setPlaylistSelection] = useState<MobilePlaylistSelection>({ playlist: null, tracks: [] });
  const [playbackSnapshot, setPlaybackSnapshot] = useState<PlaybackSnapshot | null>(props.playbackSnapshot);
  const [requestedPlaylistId, setRequestedPlaylistId] = useState<string | null>(null);

  useEffect(() => {
    if (props.openPlaylistId) {
      setTab('playlists');
      props.onConsumeOpenPlaylist();
    }
  }, [props]);

  return (
    <div style={styles.shell}>
      <div style={styles.hero}>
        <div>
          <div style={styles.kicker}>Your library</div>
          <div style={styles.brand}>BoogieBox</div>
        </div>
        <div style={styles.userBlock}>
          <div style={styles.user}>{props.currentUser.username}</div>
          <div style={styles.userRole}>Mobile listening</div>
        </div>
      </div>

      {tab === 'home' && (
        <MobileHomeView
          onOpenAlbum={(album) => {
            setBrowseSelection({ artist: null, album, tracks: [] });
            setTab('browse');
          }}
          onOpenPlaylist={(playlistId) => {
            setPlaylistSelection({ playlist: null, tracks: [] });
            setRequestedPlaylistId(playlistId);
            setTab('playlists');
          }}
          onOpenBrowse={() => {
            setTab('browse');
          }}
          onPlayTrack={(track, allTracks) => props.onPlayTrack(track, allTracks)}
        />
      )}
      {tab === 'browse' && (
        <MobileBrowseView
          onPlayTrack={(track, allTracks) => props.onPlayTrack(track, allTracks)}
          onAddToQueue={props.onAddToQueue}
          selection={browseSelection}
          onSelectionChange={setBrowseSelection}
          playbackSnapshot={playbackSnapshot}
          libraries={props.libraries}
        />
      )}
      {tab === 'search' && (
        <MobileSearchView
          onPlayTrack={(track, allTracks) => props.onPlayTrack(track, allTracks)}
          onAddToQueue={props.onAddToQueue}
        />
      )}
      {tab === 'playlists' && (
        <MobilePlaylistsView
          initialPlaylistId={requestedPlaylistId ?? props.openPlaylistId}
          selection={playlistSelection}
          onSelectionChange={setPlaylistSelection}
          onPlayTrack={(track, allTracks) => props.onPlayTrack(track, allTracks, playlistSelection.playlist ? { type: 'playlist', id: playlistSelection.playlist.id, rememberProgress: !!playlistSelection.playlist.remember_progress } : undefined)}
          onAddToQueue={props.onAddToQueue}
        />
      )}
      {tab === 'now-playing' && (
        <MobileNowPlayingView
          currentUser={props.currentUser}
          snapshot={playbackSnapshot}
          playerState={props.playerState}
          onStateChange={props.onPlaybackStateChange}
        />
      )}

      <Player
        state={props.playerState}
        onStateChange={props.onPlaybackStateChange}
        ffmpegAvailable={props.ffmpegAvailable}
        headless
        onPlaybackSnapshotChange={setPlaybackSnapshot}
      />
      <MobileMiniPlayer
        snapshot={playbackSnapshot}
        playerState={props.playerState}
        onStateChange={props.onPlaybackStateChange}
        onOpenNowPlaying={() => setTab('now-playing')}
        onQuickRate={(rating) => {
          const track = playbackSnapshot?.currentTrack ?? props.playerState.queue[props.playerState.currentIndex];
          if (track) api.setTrackRating(track.id, rating).catch(() => {});
        }}
      />
      <MobileTabBar activeTab={tab} onChange={setTab} />
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  shell: {
    minHeight: '100%',
    background: [
      'radial-gradient(circle at top left, color-mix(in srgb, var(--accent) 14%, transparent) 0%, transparent 28%)',
      'linear-gradient(180deg, color-mix(in srgb, var(--surface) 22%, var(--bg)) 0%, var(--bg) 24%, #060607 100%)',
    ].join(','),
    color: 'var(--text)',
    paddingTop: 'env(safe-area-inset-top, 0px)',
  },
  hero: {
    position: 'sticky',
    top: 0,
    zIndex: 20,
    padding: '18px 16px 10px',
    background: 'linear-gradient(180deg, color-mix(in srgb, var(--surface) 92%, #050506) 0%, color-mix(in srgb, var(--surface) 74%, transparent) 78%, transparent 100%)',
    backdropFilter: 'blur(12px)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 40%, transparent)',
  },
  kicker: phase2.mobileKicker,
  brand: { fontSize: 24, fontWeight: 800, letterSpacing: -0.8, marginTop: 4 },
  userBlock: { textAlign: 'right' },
  user: { fontSize: 13, color: 'var(--text)', fontWeight: 700 },
  userRole: { fontSize: 11, color: 'var(--text-muted)', marginTop: 3 },
};
