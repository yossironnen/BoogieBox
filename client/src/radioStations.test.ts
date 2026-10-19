import { describe, expect, it, vi } from 'vitest';
import { isHttpUrl, isLibraryTrackId, isLiveTrack, isRadioTrackId, radioStationProblem, radioStationTrack } from './radioStations';

vi.mock('./api', () => ({
  api: { radioStations: { streamUrl: (id: string) => `/api/radio-stations/${id}/stream` } },
}));

describe('radioStations', () => {
  const station = { id: 's1', name: 'Jazz FM', streamUrl: 'http://jazz.example/live', homepageUrl: null };

  it('builds a live queue item that streams through the relay', () => {
    const track = radioStationTrack(station);
    expect(track.id).toBe('radio:s1');
    expect(track.title).toBe('Jazz FM');
    expect(track.duration).toBeNull();
    expect(track.stream_url_override).toBe('/api/radio-stations/s1/stream');
    expect(isLiveTrack(track)).toBe(true);
    expect(isRadioTrackId(track.id)).toBe(true);
  });

  it('tells library tracks from synthetic playables', () => {
    expect(isLibraryTrackId('42')).toBe(true);
    expect(isLibraryTrackId(42)).toBe(true);
    expect(isLibraryTrackId('boogiemix:7')).toBe(false);
    expect(isLibraryTrackId('radio:s1')).toBe(false);
    expect(isLibraryTrackId('')).toBe(false);
    expect(isLibraryTrackId(null)).toBe(false);
  });

  it('recognises radio ids and live items', () => {
    expect(isRadioTrackId('t1')).toBe(false);
    expect(isRadioTrackId(null)).toBe(false);
    expect(isLiveTrack(null)).toBe(false);
    expect(isLiveTrack({ id: 't1' })).toBe(false);
    expect(isLiveTrack({ id: 't1', live: true })).toBe(true);
  });

  it('validates URLs and station fields like the server', () => {
    expect(isHttpUrl('https://a.example/live?x=1')).toBe(true);
    expect(isHttpUrl(' http://192.168.1.5:8000/s ')).toBe(true);
    expect(isHttpUrl('ftp://a.example')).toBe(false);
    expect(isHttpUrl('http://exa mple.com')).toBe(false);
    expect(isHttpUrl('http://')).toBe(false);
    expect(radioStationProblem({ name: 'A', streamUrl: 'http://a.b/s', homepageUrl: '' })).toBeNull();
    expect(radioStationProblem({ name: ' ', streamUrl: 'http://a.b', homepageUrl: '' })).toMatch(/name/);
    expect(radioStationProblem({ name: 'x'.repeat(201), streamUrl: 'http://a.b', homepageUrl: '' })).toMatch(/name/);
    expect(radioStationProblem({ name: 'A', streamUrl: 'a.b', homepageUrl: '' })).toMatch(/Stream URL/);
    expect(radioStationProblem({ name: 'A', streamUrl: 'http://a.b', homepageUrl: 'mailto:x' })).toMatch(/Homepage/);
  });
});
