/**
 * Tests the smart playlist draft model: validation, conversion to and from the API definition, tree edits.
 */

import { describe, expect, it } from 'vitest';
import {
  COPY, MAX_RULES, appendTo, canNestGroup, checkDraft, countRules, definitionToDraft, draftToDefinition,
  newDraft, newGroup, newRule, replaceNode, ruleValue, starsLabel, withField, withOperator,
  type DraftRule, type SmartDraft,
} from './smartPlaylistRules';

function withRule(rule: Partial<DraftRule> & Pick<DraftRule, 'field'>): SmartDraft {
  const draft = newDraft('Mix');
  const base = newRule(rule.field);
  draft.root.children = [{ ...base, ...rule }];
  return draft;
}

describe('smart playlist rules', () => {
  it('turns a valid draft into the API definition', () => {
    const draft = newDraft('Late night');
    const genre = { ...newRule('genre'), text: ' Trip Hop ' };
    const group = newGroup();
    group.children = [
      { ...newRule('year'), operator: 'between', text: '1994', text2: '2001' },
      { ...newRule('rating'), operator: 'gte', rating: 4.5 },
    ];
    draft.root.children = [genre, group, { ...newRule('favourite') }];
    draft.sort = { field: 'last_played_at', direction: 'asc' };
    draft.thenBy = { field: 'title', direction: 'desc' };
    draft.maxTracks = '50';
    draft.limitLength = true;
    draft.maxMinutes = '90';

    expect(checkDraft(draft).valid).toBe(true);
    expect(draftToDefinition(draft)).toEqual({
      schemaVersion: 1,
      root: {
        mode: 'all',
        children: [
          { field: 'genre', operator: 'is', value: 'Trip Hop' },
          {
            mode: 'any',
            children: [
              { field: 'year', operator: 'between', value: [1994, 2001] },
              { field: 'rating', operator: 'gte', value: 4.5 },
            ],
          },
          { field: 'favourite', operator: 'is' },
        ],
      },
      sort: [{ field: 'last_played_at', direction: 'asc' }, { field: 'title', direction: 'desc' }],
      limit: { maxTracks: 50, maxDurationSeconds: 5400 },
    });
  });

  it('reports the approved message for each wrong value', () => {
    const cases: [Partial<DraftRule> & Pick<DraftRule, 'field'>, string][] = [
      [{ field: 'artist', text: '  ' }, 'Enter an artist name.'],
      [{ field: 'album', text: '' }, 'Enter an album name.'],
      [{ field: 'genre', text: '' }, 'Choose a genre.'],
      [{ field: 'year', text: '0' }, 'Enter a year from 1 to 9999.'],
      [{ field: 'year', operator: 'between', text: '2001', text2: '1994' }, COPY.between],
      [{ field: 'library_id', libraries: [] }, 'Choose at least one library.'],
      [{ field: 'rating', rating: null }, 'Choose a rating.'],
      [{ field: 'added_at', text: '3651' }, 'Enter 1 to 3,650 days.'],
      [{ field: 'last_played_at', operator: 'before', text: '2026-02-30' }, 'Choose a date.'],
      [{ field: 'play_count', text: '2.5' }, 'Enter a whole number of plays.'],
      [{ field: 'bpm', text: '1001' }, 'Enter a BPM from 1 to 1000.'],
    ];
    for (const [rule, message] of cases) {
      const draft = withRule(rule);
      const check = checkDraft(draft);
      expect(check.valid, message).toBe(false);
      expect([...check.ruleErrors.values()]).toEqual([message]);
    }
    expect(ruleValue({ ...newRule('last_played_at'), operator: 'is_missing' })).toEqual({});
    expect(ruleValue({ ...newRule('bpm'), text: '120.5' })).toEqual({ value: 120.5 });
  });

  it('checks the limits', () => {
    const draft = withRule({ field: 'artist', text: 'Air' });
    draft.maxTracks = '1001';
    draft.limitLength = true;
    draft.maxMinutes = '0';
    const check = checkDraft(draft);
    expect(check.maxTracksError).toBe(COPY.maxTracks);
    expect(check.maxMinutesError).toBe(COPY.maxMinutes);
    expect(check.valid).toBe(false);
  });

  it('round-trips stored rules and refuses ones it cannot edit', () => {
    const stored = {
      schemaVersion: 1,
      root: {
        mode: 'any',
        children: [
          { field: 'library_id', operator: 'not_in', value: ['lib1', 'lib2'] },
          { mode: 'all', children: [{ field: 'rating', operator: 'between', value: [3, 4.5] }] },
          { field: 'last_played_at', operator: 'not_in_last_days', value: 90 },
        ],
      },
      sort: [{ field: 'rating', direction: 'desc' }],
      limit: { maxTracks: 25, maxDurationSeconds: 3600 },
    };
    const draft = definitionToDraft(stored, 'Stored')!;
    expect(draft.name).toBe('Stored');
    expect(draft.maxMinutes).toBe('60');
    expect(draft.thenBy).toBeNull();
    expect(draftToDefinition(draft)).toEqual(stored);

    expect(definitionToDraft({ ...stored, schemaVersion: 2 }, 'x')).toBeNull();
    expect(definitionToDraft({ ...stored, root: { mode: 'all', children: [{ field: 'mood', operator: 'is', value: 'x' }] } }, 'x')).toBeNull();
  });

  it('keeps a value when the field or condition still fits it', () => {
    const year = { ...newRule('year'), operator: 'gte', text: '1990' };
    expect(withField(year, 'bpm')).toMatchObject({ field: 'bpm', operator: 'gte', text: '1990' });
    expect(withField(year, 'artist')).toMatchObject({ field: 'artist', operator: 'is', text: '' });
    expect(withOperator(year, 'between').text).toBe('1990');
    expect(withOperator(year, 'is_missing').text).toBe('');
    const rated = { ...newRule('rating'), rating: 4 };
    expect(withOperator(rated, 'between').rating).toBe(4);
  });

  it('removes a group with its last rule and bounds nesting', () => {
    const draft = newDraft();
    const group = newGroup();
    draft.root = appendTo(draft.root, draft.root.key, group);
    expect(countRules(draft.root)).toBe(2);
    const only = group.children[0];
    const after = replaceNode(draft.root, only.key, null);
    expect(after.children).toHaveLength(1);
    expect(canNestGroup(1)).toBe(true);
    expect(canNestGroup(2)).toBe(true);
    expect(canNestGroup(3)).toBe(false);
    expect(MAX_RULES).toBe(24);
    expect(starsLabel(1)).toBe('1 star');
    expect(starsLabel(4.5)).toBe('4.5 stars');
  });
});
