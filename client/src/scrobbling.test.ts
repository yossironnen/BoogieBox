import { describe, expect, it } from 'vitest';
import { createScrobbleTracker, plausibleListenBrainzToken, scrobbleThreshold } from './scrobbling';

describe('scrobbling', () => {
  it('uses half the length or four minutes, never for short or unknown tracks', () => {
    expect(scrobbleThreshold(30)).toBeNull();
    expect(scrobbleThreshold(null)).toBeNull();
    expect(scrobbleThreshold(Number.POSITIVE_INFINITY)).toBeNull();
    expect(scrobbleThreshold(100)).toBe(50);
    expect(scrobbleThreshold(1200)).toBe(240);
  });

  it('checks ListenBrainz token shape', () => {
    expect(plausibleListenBrainzToken(' 0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0 ')).toBe(true);
    expect(plausibleListenBrainzToken('short')).toBe(false);
    expect(plausibleListenBrainzToken('has spaces in it')).toBe(false);
  });

  it('reports each play once, after it was seen below the threshold', () => {
    const tracker = createScrobbleTracker();
    expect(tracker.observe('1:0:t1', 't1', 100, 0, 1000)).toBeNull();
    expect(tracker.observe('1:0:t1', 't1', 100, 49, 1049)).toBeNull();
    expect(tracker.observe('1:0:t1', 't1', 100, 50, 1050)).toEqual({ trackId: 't1', startedAt: 1000 });
    expect(tracker.observe('1:0:t1', 't1', 100, 80, 1080)).toBeNull();

    // A new play of a short track never counts.
    expect(tracker.observe('2:1:t2', 't2', 20, 15, 2000)).toBeNull();

    // A handoff that resumes past the threshold was counted elsewhere.
    expect(tracker.observe('3:0:t3', 't3', 200, 150, 3000)).toBeNull();
    expect(tracker.observe('3:0:t3', 't3', 200, 160, 3010)).toBeNull();

    // No play key or a bad position: nothing.
    expect(tracker.observe(null, 't4', 200, 150, 4000)).toBeNull();
    expect(tracker.observe('4:0:t4', 't4', 200, Number.NaN, 4000)).toBeNull();
  });
});
