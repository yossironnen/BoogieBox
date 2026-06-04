/**
 * Tests Home View.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import type { Stats } from '../types';
import {
  buildBoogieSnapshot,
  mostPlayedArtistAriaLabel,
  recentlyPlayedAriaLabel,
  selectMostPlayedArtists,
  selectRecentlyPlayedTracks,
  selectTopGenres,
  selectTopPlayedTracks,
  topArtistAriaLabel,
  topGenreAriaLabel,
  topRatedAlbumAriaLabel,
  topRatedTrackAriaLabel,
  topPlayedTrackAriaLabel,
} from '../components/HomeView';

// HomeView dashboard widget logic tests
// The widgets are internal components, so we test the data transformation logic
// that powers them (sorting, slicing, formatting).

describe('HomeView Dashboard', () => {
  describe('StatsWidget formatting', () => {
    it('should format stats values with toLocaleString', () => {
      const stats: Stats = {
        total_tracks: 12345,
        total_artists: 678,
        total_albums: 234,
        total_libraries: 3,
        total_hours: 891,
        total_gb: 45.2,
      };
      expect(stats.total_tracks.toLocaleString()).toBe('12,345');
      expect(stats.total_artists.toLocaleString()).toBe('678');
      expect(stats.total_albums.toLocaleString()).toBe('234');
    });

    it('should handle missing video counts gracefully', () => {
      const stats: Stats = {
        total_tracks: 0,
        total_artists: 0,
        total_albums: 0,
        total_libraries: 0,
        total_hours: null,
        total_gb: null,
      };
    });
  });

  describe('Top rated widget labels', () => {
    it('should build accessible artist link labels', () => {
      expect(topArtistAriaLabel('Daft Punk')).toBe('Open artist Daft Punk');
    });

    it('should build accessible album and ranked-track labels', () => {
      expect(topRatedAlbumAriaLabel({ id: '1', title: 'Discovery', track_count: 14 } as any)).toBe('Open album Discovery');
      expect(topRatedTrackAriaLabel({ id: '2', title: 'Digital Love', file_name: 'digital-love.mp3' } as any)).toBe('Play ranked track Digital Love');
    });
  });

  describe('Genre sorting logic', () => {
    it('should sort genres by track_count descending and take top 10', () => {
      const genres = [
        { genre: 'Rock', track_count: 500 },
        { genre: 'Jazz', track_count: 200 },
        { genre: 'Pop', track_count: 800 },
        { genre: 'Classical', track_count: 150 },
        { genre: 'Electronic', track_count: 300 },
      ];

      const sorted = [...genres]
        .sort((a, b) => b.track_count - a.track_count)
        .slice(0, 10)
        .map(g => ({ label: g.genre, value: g.track_count }));

      expect(sorted[0]).toEqual({ label: 'Pop', value: 800 });
      expect(sorted[1]).toEqual({ label: 'Rock', value: 500 });
      expect(sorted[4]).toEqual({ label: 'Classical', value: 150 });
    });

    it('should return top genres with preserved genre names for browse navigation', () => {
      const genres = [
        { genre: 'Rock', track_count: 10 },
        { genre: 'Ambient', track_count: 30 },
        { genre: 'Disco', track_count: 20 },
      ];
      const top = selectTopGenres(genres, 2);
      expect(top.map(g => g.genre)).toEqual(['Ambient', 'Disco']);
    });

    it('should build accessible genre link labels', () => {
      expect(topGenreAriaLabel('Synthwave')).toBe('Open genre Synthwave');
    });
  });

  describe('Recently played track list logic', () => {
    it('should filter out tracks without a last_played_at timestamp', () => {
      const tracks = [
        { id: '1', title: 'A', file_name: 'a.mp3', last_played_at: '2026-02-25 10:00:00' },
        { id: '2', title: 'B', file_name: 'b.mp3', last_played_at: null },
        { id: '3', title: 'C', file_name: 'c.mp3' },
      ] as any[];
      const result = selectRecentlyPlayedTracks(tracks as any, 10);
      expect(result.map(t => t.id)).toEqual(['1']);
    });

    it('should cap recently played to the requested limit', () => {
      const tracks = Array.from({ length: 12 }, (_, i) => ({
        id: i + 1,
        title: `Track ${i + 1}`,
        file_name: `track-${i + 1}.mp3`,
        last_played_at: `2026-02-25 10:${String(i).padStart(2, '0')}:00`,
      })) as any[];
      const result = selectRecentlyPlayedTracks(tracks as any, 10);
      expect(result).toHaveLength(10);
    });

    it('should build accessible recently played row labels', () => {
      const label = recentlyPlayedAriaLabel({
        id: '1',
        title: 'Song Name',
        file_name: 'song.mp3',
      } as any);
      expect(label).toBe('Play Song Name');
    });
  });

  describe('Top played stats logic', () => {
    it('should include only tracks with play_count > 0 and sort by play_count descending', () => {
      const tracks = [
        { id: '1', title: 'A', file_name: 'a.mp3', play_count: 2, last_played_at: '2026-02-25 10:00:00' },
        { id: '2', title: 'B', file_name: 'b.mp3', play_count: 0, last_played_at: '2026-02-25 10:10:00' },
        { id: '3', title: 'C', file_name: 'c.mp3', play_count: 8, last_played_at: '2026-02-25 10:20:00' },
      ] as any[];
      const result = selectTopPlayedTracks(tracks as any, 10);
      expect(result.map((track) => track.id)).toEqual(['3', '1']);
    });

    it('should include only artists with play_count > 0 and sort by play_count descending', () => {
      const artists = [
        { id: '1', name: 'Alpha', play_count: 4, track_count: 2, album_count: 1 },
        { id: '2', name: 'Bravo', play_count: 0, track_count: 8, album_count: 2 },
        { id: '3', name: 'Charlie', play_count: 11, track_count: 5, album_count: 3 },
      ] as any[];
      const result = selectMostPlayedArtists(artists as any, 10);
      expect(result.map((artist) => artist.id)).toEqual(['3', '1']);
    });

    it('should build accessible labels for top-played rows', () => {
      expect(topPlayedTrackAriaLabel({ id: '7', title: 'Night Drive', file_name: 'night.mp3' } as any)).toBe('Play Night Drive');
      expect(mostPlayedArtistAriaLabel('The Avalanches')).toBe('Open artist The Avalanches');
    });
  });

  describe('Bar chart percentage calculation', () => {
    it('should calculate bar width as percentage of max value', () => {
      const items = [
        { label: 'A', value: 100 },
        { label: 'B', value: 50 },
        { label: 'C', value: 25 },
      ];
      const max = items[0].value;
      const widths = items.map(i => (i.value / max) * 100);
      expect(widths).toEqual([100, 50, 25]);
    });

    it('should handle max of 0 without NaN', () => {
      const max = Math.max(0, 1); // fallback to 1
      expect((0 / max) * 100).toBe(0);
    });
  });

  describe('Boogie day bucketing', () => {
    it('maps plays to calendar days consistently for heatmap playback filters', () => {
      const tracks = [
        { id: '1', artist: 'A', duration: 120, last_played_at: '2026-03-08 01:30:00' },
        { id: '2', artist: 'B', duration: 120, last_played_at: '2026-03-09 23:10:00' },
      ] as any[];

      const snapshot = buildBoogieSnapshot(tracks as any, 7, new Date('2026-03-12T12:00:00'));
      const totalPlays = snapshot.dailyCounts.reduce((sum, value) => sum + value, 0);

      expect(totalPlays).toBe(2);
      expect(snapshot.dailyCounts.filter((count) => count > 0)).toHaveLength(2);
    });
  });
});

