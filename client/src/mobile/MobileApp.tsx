/**
 * Defines mobile Mobile App behavior for the BoogieBox React client.
 */

import { useEffect, useState } from 'react';
import Player, { type PlaybackSnapshot, type PlayerEqControls } from '../components/Player';
import { api } from '../api';
import type { MobileBrowseSelection, MobilePlaylistSelection, MobileSharedProps, MobileTabId } from './mobileShell';
import MobileMiniPlayer from './components/MobileMiniPlayer';
import MobileTabBar from './components/MobileTabBar';
import MobileBrowseView from './views/MobileBrowseView';
import MobileHomeView from './views/MobileHomeView';
import MobileNowPlayingView from './views/MobileNowPlayingView';
import MobilePlaylistsView from './views/MobilePlaylistsView';
import MobileSearchView from './views/MobileSearchView';
import { hybridMobileShellStyles } from '../hybridPreview';

/** Mobile App is part of this module's public API. */
export default function MobileApp(props: MobileSharedProps) {
  const [tab, setTab] = useState<MobileTabId>('home');
  const [browseSelection, setBrowseSelection] = useState<MobileBrowseSelection>({ artist: null, album: null, tracks: [] });
  const [playlistSelection, setPlaylistSelection] = useState<MobilePlaylistSelection>({ playlist: null, tracks: [] });
  const [playbackSnapshot, setPlaybackSnapshot] = useState<PlaybackSnapshot | null>(props.playbackSnapshot);
  const [eqControls, setEqControls] = useState<PlayerEqControls | null>(null);
  const [requestedPlaylistId, setRequestedPlaylistId] = useState<string | null>(null);

  useEffect(() => {
    if (props.openPlaylistId) {
      setTab('playlists');
      props.onConsumeOpenPlaylist();
    }
  }, [props]);

  return (
    <div style={hybridMobileShellStyles.shell}>
      {tab !== 'now-playing' ? (
        <header style={hybridMobileShellStyles.header}>
          <div style={hybridMobileShellStyles.brandLockup}>
            <img src="/boogiebox.png" alt="" style={hybridMobileShellStyles.headerLogo} />
            <div>
              <div style={hybridMobileShellStyles.headerKicker}>Your library</div>
              <div style={hybridMobileShellStyles.headerBrand}>BoogieBox</div>
            </div>
          </div>
          <div
            aria-label={`Signed in as ${props.currentUser.username}`}
            style={hybridMobileShellStyles.userPill}
          >
            <div aria-hidden="true" style={hybridMobileShellStyles.userAvatar}>
              {props.currentUser.username.slice(0, 2).toUpperCase()}
            </div>
            <div style={{ minWidth: 0 }}>
              <div style={hybridMobileShellStyles.userName}>{props.currentUser.username}</div>
              <div style={hybridMobileShellStyles.userMeta}>Listening</div>
            </div>
          </div>
        </header>
      ) : null}

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
          appSettings={props.settings}
          onAppSettingsChange={props.onSettingsChange}
          hybridThemeMode={props.hybridThemeMode}
          onHybridThemeModeChange={props.onHybridThemeModeChange}
          adaptiveAccentEnabled={props.adaptiveAccentEnabled}
          onAdaptiveAccentEnabledChange={props.onAdaptiveAccentEnabledChange}
          eqControls={eqControls}
          playbackMode={props.playbackMode}
          vinylHardcore={props.vinylHardcore}
          vinylNeedleDrop={props.vinylNeedleDrop}
          vinylAnalogFxDisabled={props.vinylAnalogFxDisabled}
          vinylNeedleDropIntensity={props.vinylNeedleDropIntensity}
          onPlaybackModeChange={props.onPlaybackModeChange}
          onVinylHardcoreChange={props.onVinylHardcoreChange}
          onVinylNeedleDropChange={props.onVinylNeedleDropChange}
          onVinylAnalogFxDisabledChange={props.onVinylAnalogFxDisabledChange}
          onVinylNeedleDropIntensityChange={props.onVinylNeedleDropIntensityChange}
        />
      )}

      <Player
        state={props.playerState}
        onStateChange={props.onPlaybackStateChange}
        ffmpegAvailable={props.ffmpegAvailable}
        playbackMode={props.playbackMode}
        vinylHardcore={props.vinylHardcore}
        vinylNeedleDrop={props.vinylNeedleDrop}
        vinylAnalogFxDisabled={props.vinylAnalogFxDisabled}
        vinylNeedleDropIntensity={props.vinylNeedleDropIntensity}
        headless
        onPlaybackSnapshotChange={setPlaybackSnapshot}
        onEqControlsChange={setEqControls}
      />
      {tab !== 'now-playing' ? (
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
      ) : null}
      <MobileTabBar activeTab={tab} onChange={setTab} />
    </div>
  );
}
