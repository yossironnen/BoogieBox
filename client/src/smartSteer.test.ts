/**
 * Tests steering pill wording, turning pills on and off, and following a playlist in the queue.
 */

import { describe, expect, it } from 'vitest';
import { definitionToDraft, draftToDefinition } from './smartPlaylistRules';
import {
  STEER_CHANGED_ICON, changedLabel, isQueueFrom, pillIcon, pillLabel, pillTip, sameTarget, steerOnly, togglePill,
  withUpcoming, type SmartSteer, type SteerPill, type SteerSignal,
} from './smartSteer';
import type { PlayerState } from './components/Player';
import type { Track } from './types';

const style = (tag: string, direction: SmartSteer['direction'] = 'more'): SmartSteer => ({ kind: 'style', tag, direction });
const signal = (name: SteerSignal): SmartSteer => ({ kind: 'signal', signal: name });
const track = (id: string) => ({ id, title: id }) as unknown as Track;

describe('pill wording', () => {
  it('uses the approved labels', () => {
    expect(pillLabel(style('gothic rock'))).toBe('More gothic rock');
    expect(pillLabel(style('new wave', 'less'))).toBe('Less new wave');
    expect(pillLabel(style('black metal', 'deeper'))).toBe('Go deeper into black metal');
    expect(pillLabel({ kind: 'mood', bucket: 'dreamy', direction: 'more' })).toBe('More dreamy');
    expect(pillLabel({ kind: 'mood', bucket: 'dark', direction: 'less' })).toBe('Less dark');
    expect(pillLabel(signal('favourites'))).toBe('My favourites');
    expect(pillLabel(signal('new'))).toBe('Something new');
    expect(pillLabel(signal('faster'))).toBe('Faster');
    expect(pillLabel(signal('slower'))).toBe('Slower');
    expect(pillLabel(signal('energetic'))).toBe('More energetic');
    expect(pillLabel(signal('calmer'))).toBe('Calmer');
  });

  it('names the popular pill after where its counts come from', () => {
    expect(pillLabel(signal('popular'), 'lastfm')).toBe('Popular');
    expect(pillLabel(signal('popular'), 'local')).toBe('Most played here');
    expect(pillLabel(signal('popular'))).toBe('Most played here');
  });

  it('gives tooltips only where there is something to say', () => {
    const pill = (over: SmartSteer, count = 86): SteerPill => ({ ...over, count });
    expect(pillTip(pill(style('post-punk')))).toBe('86 tracks');
    expect(pillTip(pill(style('post-punk'), 1))).toBe('1 track');
    expect(pillTip(pill(style('rock', 'deeper'), 132))).toBe('132 tracks in narrower kinds of rock');
    expect(pillTip(pill({ kind: 'mood', bucket: 'dark', direction: 'more' }, 20))).toBe('20 tracks');
    expect(pillTip(pill(signal('popular'), 25))).toBe('25 tracks played on this server');
    expect(pillTip(pill(signal('popular'), 25), 'lastfm')).toBe('25 tracks');
    expect(pillTip(pill(signal('favourites'), 12))).toBe('12 tracks');
    expect(pillTip(pill(signal('new')))).toBe('Tracks you have not played yet');
    expect(pillTip(pill(signal('energetic')))).toBe('Uses deep analysis energy');
    expect(pillTip(pill(signal('calmer')))).toBe('Uses deep analysis energy');
    expect(pillTip(pill(signal('faster')))).toBeUndefined();
  });

  it('shares one icon across styles and gives other kinds their own', () => {
    expect(pillIcon(style('a'))).toBe(pillIcon(style('b', 'deeper')));
    const others: SmartSteer[] = [
      { kind: 'mood', bucket: 'dark' }, signal('favourites'), signal('popular'), signal('new'), signal('faster'),
      signal('calmer'), signal('energetic'),
    ];
    const icons = new Set([pillIcon(style('a')), STEER_CHANGED_ICON, ...others.map(pillIcon)]);
    expect(icons.size).toBe(others.length + 2);
    expect(pillIcon(signal('slower'))).toBe(pillIcon(signal('faster')));
  });

  it('counts changed tracks', () => {
    expect(changedLabel(1)).toBe('1 track changed');
    expect(changedLabel(0)).toBe('0 tracks changed');
    expect(changedLabel(14)).toBe('14 tracks changed');
  });
});

describe('turning pills on and off', () => {
  it('strips what the server does not store', () => {
    expect(steerOnly({ ...style('sludge'), count: 9 } as SteerPill)).toEqual(style('sludge'));
    expect(steerOnly(signal('new'))).toEqual(signal('new'));
    expect(steerOnly({ kind: 'mood', bucket: 'dark', direction: 'less' })).toEqual({ kind: 'mood', bucket: 'dark', direction: 'less' });
  });

  it('knows which pills are about the same thing', () => {
    expect(sameTarget(style('a'), style('a', 'less'))).toBe(true);
    expect(sameTarget(style('a'), style('b'))).toBe(false);
    expect(sameTarget(style('a'), signal('new'))).toBe(false);
    expect(sameTarget({ kind: 'mood', bucket: 'dark' }, { kind: 'mood', bucket: 'dark', direction: 'less' })).toBe(true);
    expect(sameTarget(signal('faster'), signal('slower'))).toBe(true);
    expect(sameTarget(signal('energetic'), signal('calmer'))).toBe(true);
    expect(sameTarget(signal('new'), signal('popular'))).toBe(false);
  });

  it('adds, removes, replaces, and stops at three', () => {
    expect(togglePill([], style('a'))).toEqual([style('a')]);
    expect(togglePill([style('a')], style('a'))).toEqual([]);
    expect(togglePill([style('a')], style('a', 'less'))).toEqual([style('a', 'less')]);
    expect(togglePill([signal('faster')], signal('slower'))).toEqual([signal('slower')]);
    const full = [style('a'), style('b'), style('c')];
    expect(togglePill(full, style('d'))).toBe(full);
    expect(togglePill(full, style('b'))).toEqual([style('a'), style('c')]);
    expect(togglePill(full, style('b', 'less'))).toEqual([style('a'), style('c'), style('b', 'less')]);
  });
});

describe('following the playlist in the queue', () => {
  const state = (over: Partial<PlayerState> = {}): PlayerState => ({
    queue: [track('a'), track('b'), track('c')],
    currentIndex: 1,
    isPlaying: true,
    playToken: 7,
    queueSource: { type: 'playlist', id: 'p1' },
    ...over,
  });

  it('recognises the queue by playlist', () => {
    expect(isQueueFrom({ type: 'playlist', id: 'p1' }, 'p1')).toBe(true);
    expect(isQueueFrom({ type: 'playlist', id: 'p2' }, 'p1')).toBe(false);
    expect(isQueueFrom({ type: 'album', id: 'p1' }, 'p1')).toBe(false);
    expect(isQueueFrom(null, 'p1')).toBe(false);
    expect(isQueueFrom(undefined, 'p1')).toBe(false);
  });

  it('replaces only what comes next and restarts nothing', () => {
    const next = withUpcoming(state(), 'p1', [track('b'), track('x'), track('a'), track('y')]);
    expect(next.queue.map(t => t.id)).toEqual(['a', 'b', 'x', 'y']);
    expect(next.currentIndex).toBe(1);
    expect(next.playToken).toBe(7);
    expect(next.isPlaying).toBe(true);
  });

  it('leaves another source alone', () => {
    const other = state({ queueSource: { type: 'album', id: 'p1' } });
    expect(withUpcoming(other, 'p1', [track('x')])).toBe(other);
  });
});

describe('the rules editor keeps pills', () => {
  const base = {
    schemaVersion: 1 as const,
    root: { mode: 'all' as const, children: [{ field: 'genre', operator: 'is', value: 'Rock' }] },
    sort: [{ field: 'title' as const, direction: 'asc' as const }],
    limit: { maxTracks: 100 },
  };

  it('carries them from the definition through the draft', () => {
    const steer = [style('sludge'), signal('new')];
    const draft = definitionToDraft({ ...base, steer }, 'Rock');
    expect(draft?.steer).toEqual(steer);
    expect(draftToDefinition(draft!).steer).toEqual(steer);
  });

  it('adds nothing when there are none', () => {
    const draft = definitionToDraft(base, 'Rock');
    expect(draft?.steer).toBeUndefined();
    expect(draftToDefinition(draft!)).not.toHaveProperty('steer');
    expect(draftToDefinition({ ...draft!, steer: [] })).not.toHaveProperty('steer');
  });
});
