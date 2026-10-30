/**
 * Smart playlist rules: the API contract, the editor's draft model, validation and the approved wording.
 * Shared by the desktop and mobile editors.
 */

// ─── API contract (schema version 1, see docs/API.md) ─────────────────────────

export type SmartField =
  | 'artist' | 'album' | 'genre' | 'year' | 'library_id' | 'rating'
  | 'added_at' | 'last_played_at' | 'play_count' | 'bpm' | 'favourite';

export type SmartSortField =
  | 'title' | 'artist' | 'album' | 'year' | 'rating' | 'added_at' | 'last_played_at' | 'play_count' | 'bpm';

export type SmartMode = 'all' | 'any';
export type SmartDirection = 'asc' | 'desc';

export interface SmartRule { field: string; operator: string; value?: unknown }
export interface SmartGroup { mode: SmartMode; children: SmartNode[] }
export type SmartNode = SmartRule | SmartGroup;
export interface SmartSortKey { field: SmartSortField; direction: SmartDirection }
export interface SmartLimit { maxTracks?: number; maxDurationSeconds?: number }

export interface SmartDefinition {
  schemaVersion: 1;
  root: SmartGroup;
  sort?: SmartSortKey[];
  limit?: SmartLimit;
}

/** `GET /api/playlists/{id}/smart-definition`. */
export interface SmartDefinitionView {
  definition: unknown;
  schemaVersion: number;
  editRevision: number;
  membershipRevision: number;
  evaluatedAt: string | null;
  supported: boolean;
  missingLibraryIds: string[];
}

export const MAX_RULES = 24;
export const MAX_NODES = 32;
/** The root is level 1; groups nest two more levels. */
export const MAX_DEPTH = 3;
export const MAX_TRACKS = 1000;
export const DEFAULT_MAX_TRACKS = 100;
export const MAX_MINUTES = 1440;
export const MAX_TEXT_CHARS = 128;
export const MAX_DAYS = 3650;

export function isGroup(node: SmartNode): node is SmartGroup {
  return Array.isArray((node as SmartGroup).children);
}

// ─── Fields, conditions and wording ───────────────────────────────────────────

/** How a condition takes its value. */
export type ValueKind =
  | 'text' | 'genre' | 'number' | 'number_range' | 'rating' | 'rating_range'
  | 'libraries' | 'days' | 'date' | 'none';

export interface ConditionSpec { value: string; label: string; kind: ValueKind }

export interface FieldSpec {
  field: SmartField;
  label: string;
  /** Inline SVG path (24×24, stroke). */
  icon: string;
  conditions: ConditionSpec[];
  /** Shown under a wrong value. */
  error: string;
  /** Accessible name / placeholder for a single value. */
  valueLabel: string;
  placeholder?: string;
  /** Text after the value ("days", "plays"). */
  suffix?: string;
}

const NUMBER_CONDITIONS = (missing: string, present: string): ConditionSpec[] => [
  { value: 'is', label: 'is', kind: 'number' },
  { value: 'gte', label: 'is at least', kind: 'number' },
  { value: 'lte', label: 'is at most', kind: 'number' },
  { value: 'between', label: 'is between', kind: 'number_range' },
  { value: 'is_missing', label: missing, kind: 'none' },
  { value: 'is_present', label: present, kind: 'none' },
];

const DATE_CONDITIONS: ConditionSpec[] = [
  { value: 'in_last_days', label: 'in the last', kind: 'days' },
  { value: 'not_in_last_days', label: 'not in the last', kind: 'days' },
  { value: 'before', label: 'before', kind: 'date' },
  { value: 'on_or_after', label: 'on or after', kind: 'date' },
];

const TEXT_CONDITIONS: ConditionSpec[] = [
  { value: 'is', label: 'is', kind: 'text' },
  { value: 'is_not', label: 'is not', kind: 'text' },
  { value: 'contains', label: 'contains', kind: 'text' },
  { value: 'not_contains', label: 'does not contain', kind: 'text' },
];

export const FIELDS: FieldSpec[] = [
  {
    field: 'artist', label: 'Artist', icon: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',
    conditions: TEXT_CONDITIONS, error: 'Enter an artist name.', valueLabel: 'Artist name', placeholder: 'Artist name',
  },
  {
    field: 'album', label: 'Album', icon: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM12 12h.01M12 8a4 4 0 0 1 4 4',
    conditions: TEXT_CONDITIONS, error: 'Enter an album name.', valueLabel: 'Album name', placeholder: 'Album name',
  },
  {
    field: 'genre', label: 'Genre', icon: 'M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8zM7.5 7.5h.01',
    conditions: [
      { value: 'is', label: 'is', kind: 'genre' },
      { value: 'is_not', label: 'is not', kind: 'genre' },
      { value: 'is_missing', label: 'is empty', kind: 'none' },
      { value: 'is_present', label: 'is not empty', kind: 'none' },
    ],
    error: 'Choose a genre.', valueLabel: 'Genre', placeholder: 'Choose a genre',
  },
  {
    field: 'year', label: 'Year', icon: 'M4 5h16v16H4zM4 10h16M9 3v4M15 3v4',
    conditions: NUMBER_CONDITIONS('is unknown', 'is known'), error: 'Enter a year from 1 to 9999.', valueLabel: 'Year',
  },
  {
    field: 'library_id', label: 'Library', icon: 'M4 19V5M9 19V5M14 19l3-14 4 1-3 14z',
    conditions: [
      { value: 'in', label: 'is one of', kind: 'libraries' },
      { value: 'not_in', label: 'is none of', kind: 'libraries' },
    ],
    error: 'Choose at least one library.', valueLabel: 'Library',
  },
  {
    field: 'rating', label: 'Rating', icon: 'm12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z',
    conditions: NUMBER_CONDITIONS('is not rated', 'is rated').map(c => ({
      ...c, kind: c.kind === 'number' ? 'rating' : c.kind === 'number_range' ? 'rating_range' : c.kind,
    })),
    error: 'Choose a rating.', valueLabel: 'Rating',
  },
  {
    field: 'added_at', label: 'Date added', icon: 'M4 5h16v16H4zM4 10h16M12 13v5M9.5 15.5h5',
    conditions: DATE_CONDITIONS, error: 'Enter 1 to 3,650 days.', valueLabel: 'Days', suffix: 'days',
  },
  {
    field: 'last_played_at', label: 'Last played', icon: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM12 7v5l3 2',
    conditions: [
      ...DATE_CONDITIONS,
      { value: 'is_missing', label: 'never', kind: 'none' },
      { value: 'is_present', label: 'any time', kind: 'none' },
    ],
    error: 'Enter 1 to 3,650 days.', valueLabel: 'Days', suffix: 'days',
  },
  {
    field: 'play_count', label: 'Play count', icon: 'M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 19a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
    conditions: NUMBER_CONDITIONS('', '').slice(0, 4), error: 'Enter a whole number of plays.', valueLabel: 'Plays', suffix: 'plays',
  },
  {
    field: 'bpm', label: 'BPM', icon: 'M3 12h4l3-8 4 16 3-8h4',
    conditions: NUMBER_CONDITIONS('is unknown', 'is known'), error: 'Enter a BPM from 1 to 1000.', valueLabel: 'BPM',
  },
  {
    field: 'favourite', label: 'Favourite', icon: 'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8l1 1L12 21l7.8-7.6 1-1a5.5 5.5 0 0 0 0-7.8z',
    conditions: [
      { value: 'is', label: 'yes', kind: 'none' },
      { value: 'is_not', label: 'no', kind: 'none' },
    ],
    error: '', valueLabel: 'Favourite',
  },
];

const FIELD_BY_NAME = new Map(FIELDS.map(spec => [spec.field, spec]));

export function fieldSpec(field: string): FieldSpec | undefined {
  return FIELD_BY_NAME.get(field as SmartField);
}

export function conditionSpec(field: string, operator: string): ConditionSpec | undefined {
  return fieldSpec(field)?.conditions.find(c => c.value === operator);
}

export const SORT_FIELDS: { field: SmartSortField; label: string; directions: [string, string] }[] = [
  { field: 'title', label: 'Title', directions: ['A to Z', 'Z to A'] },
  { field: 'artist', label: 'Artist', directions: ['A to Z', 'Z to A'] },
  { field: 'album', label: 'Album', directions: ['A to Z', 'Z to A'] },
  { field: 'year', label: 'Year', directions: ['Lowest first', 'Highest first'] },
  { field: 'rating', label: 'Rating', directions: ['Lowest first', 'Highest first'] },
  { field: 'added_at', label: 'Date added', directions: ['Oldest first', 'Newest first'] },
  { field: 'last_played_at', label: 'Last played', directions: ['Oldest first', 'Newest first'] },
  { field: 'play_count', label: 'Play count', directions: ['Lowest first', 'Highest first'] },
  { field: 'bpm', label: 'BPM', directions: ['Lowest first', 'Highest first'] },
];

export function directionLabels(field: SmartSortField): [string, string] {
  return SORT_FIELDS.find(s => s.field === field)?.directions ?? ['A to Z', 'Z to A'];
}

export const COPY = {
  between: 'The first value must be lower than the second.',
  neverPlayedHint: "Includes tracks you've never played.",
  addedAtHint: 'For tracks you had before this update, this is the date BoogieBox first scanned them.',
  removedLibrary: 'Removed library',
  removedLibraryHint: 'This library was removed. Take it out of the rule or choose another.',
  maxTracks: 'Enter 1 to 1,000 tracks.',
  maxMinutes: 'Enter 1 to 1,440 minutes.',
  ruleLimit: 'A smart playlist can have up to 24 rules.',
  depthLimit: 'Groups can go two levels deep.',
} as const;

// ─── Editor draft ─────────────────────────────────────────────────────────────

export interface DraftRule {
  kind: 'rule';
  key: string;
  field: SmartField;
  operator: string;
  /** Text, number, days or date, as typed. */
  text: string;
  /** Second number of a range. */
  text2: string;
  rating: number | null;
  rating2: number | null;
  libraries: string[];
}

export interface DraftGroup { kind: 'group'; key: string; mode: SmartMode; children: DraftNode[] }
export type DraftNode = DraftRule | DraftGroup;

export interface DraftSort { field: SmartSortField; direction: SmartDirection }

export interface SmartDraft {
  name: string;
  root: DraftGroup;
  sort: DraftSort;
  thenBy: DraftSort | null;
  maxTracks: string;
  limitLength: boolean;
  maxMinutes: string;
}

let nextKey = 0;
function key(): string {
  nextKey += 1;
  return `n${nextKey}`;
}

export function newRule(field: SmartField = 'artist'): DraftRule {
  const spec = fieldSpec(field)!;
  return {
    kind: 'rule', key: key(), field, operator: spec.conditions[0].value,
    text: '', text2: '', rating: null, rating2: null, libraries: [],
  };
}

export function newGroup(): DraftGroup {
  return { kind: 'group', key: key(), mode: 'any', children: [newRule()] };
}

export function newDraft(name = ''): SmartDraft {
  return {
    name,
    root: { kind: 'group', key: key(), mode: 'all', children: [newRule()] },
    sort: { field: 'title', direction: 'asc' },
    thenBy: null,
    maxTracks: String(DEFAULT_MAX_TRACKS),
    limitLength: false,
    maxMinutes: '',
  };
}

/** Changes a rule's field; the condition resets, a value of the same kind is kept. */
export function withField(rule: DraftRule, field: SmartField): DraftRule {
  if (field === rule.field) return rule;
  const before = conditionSpec(rule.field, rule.operator)?.kind;
  const spec = fieldSpec(field)!;
  const condition = spec.conditions.find(c => c.value === rule.operator && c.kind === before) ?? spec.conditions[0];
  const keep = condition.kind === before;
  return {
    ...rule, field, operator: condition.value,
    text: keep ? rule.text : '', text2: keep ? rule.text2 : '',
    rating: keep ? rule.rating : null, rating2: keep ? rule.rating2 : null,
    libraries: keep ? rule.libraries : [],
  };
}

/** Changes a rule's condition, carrying the value over where it still fits. */
export function withOperator(rule: DraftRule, operator: string): DraftRule {
  const before = conditionSpec(rule.field, rule.operator)?.kind;
  const after = conditionSpec(rule.field, operator)?.kind;
  const next: DraftRule = { ...rule, operator };
  if (before === after) return next;
  const numeric = (k?: ValueKind) => k === 'number' || k === 'number_range';
  const stars = (k?: ValueKind) => k === 'rating' || k === 'rating_range';
  if (!(numeric(before) && numeric(after)) && !(stars(before) && stars(after))) {
    next.text = '';
    next.text2 = '';
  }
  if (!(stars(before) && stars(after))) {
    next.rating = null;
    next.rating2 = null;
  }
  return next;
}

export function countRules(group: DraftGroup): number {
  return group.children.reduce((n, child) => n + (child.kind === 'group' ? countRules(child) : 1), 0);
}

export function countNodes(group: DraftGroup): number {
  return group.children.reduce((n, child) => n + 1 + (child.kind === 'group' ? countNodes(child) : 0), 0);
}

/** Can `group` (at `depth`, root = 1) take another nested group? */
export function canNestGroup(depth: number): boolean {
  return depth < MAX_DEPTH;
}

// ─── Validation ───────────────────────────────────────────────────────────────

const WHOLE = /^\d+$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

function wholeIn(text: string, min: number, max: number): number | null {
  const t = text.trim();
  if (!WHOLE.test(t)) return null;
  const n = Number(t);
  return n >= min && n <= max ? n : null;
}

function bpmValue(text: string): number | null {
  const t = text.trim();
  if (!/^\d+(\.\d+)?$/.test(t)) return null;
  const n = Number(t);
  return n >= 1 && n <= 1000 ? n : null;
}

function isCalendarDate(text: string): boolean {
  if (!DATE.test(text)) return false;
  const [y, m, d] = text.split('-').map(Number);
  const date = new Date(Date.UTC(y, m - 1, d));
  return y >= 1 && date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}

function numberValue(field: SmartField, text: string): number | null {
  switch (field) {
    case 'year': return wholeIn(text, 1, 9999);
    case 'play_count': return wholeIn(text, 0, 2147483647);
    default: return bpmValue(text);
  }
}

/** The rule's API value, or the message to show under it. */
export function ruleValue(rule: DraftRule): { value?: unknown; error?: string } {
  const spec = fieldSpec(rule.field)!;
  const kind = conditionSpec(rule.field, rule.operator)?.kind ?? 'none';
  switch (kind) {
    case 'none':
      return {};
    case 'text':
    case 'genre': {
      const text = rule.text.trim();
      return text && text.length <= MAX_TEXT_CHARS ? { value: text } : { error: spec.error };
    }
    case 'number': {
      const n = numberValue(rule.field, rule.text);
      return n === null ? { error: spec.error } : { value: n };
    }
    case 'number_range': {
      const a = numberValue(rule.field, rule.text);
      const b = numberValue(rule.field, rule.text2);
      if (a === null || b === null) return { error: spec.error };
      return a > b ? { error: COPY.between } : { value: [a, b] };
    }
    case 'rating':
      return rule.rating === null ? { error: spec.error } : { value: rule.rating };
    case 'rating_range': {
      if (rule.rating === null || rule.rating2 === null) return { error: spec.error };
      return rule.rating > rule.rating2 ? { error: COPY.between } : { value: [rule.rating, rule.rating2] };
    }
    case 'libraries':
      return rule.libraries.length ? { value: rule.libraries } : { error: spec.error };
    case 'days': {
      const n = wholeIn(rule.text, 1, MAX_DAYS);
      return n === null ? { error: spec.error } : { value: n };
    }
    case 'date':
      return isCalendarDate(rule.text.trim()) ? { value: rule.text.trim() } : { error: 'Choose a date.' };
  }
}

export interface DraftCheck {
  /** Message per rule key. */
  ruleErrors: Map<string, string>;
  maxTracksError?: string;
  maxMinutesError?: string;
  valid: boolean;
}

function collectRuleErrors(group: DraftGroup, out: Map<string, string>): void {
  for (const child of group.children) {
    if (child.kind === 'group') collectRuleErrors(child, out);
    else {
      const { error } = ruleValue(child);
      if (error) out.set(child.key, error);
    }
  }
}

export function checkDraft(draft: SmartDraft): DraftCheck {
  const ruleErrors = new Map<string, string>();
  collectRuleErrors(draft.root, ruleErrors);
  const maxTracksError = wholeIn(draft.maxTracks, 1, MAX_TRACKS) === null ? COPY.maxTracks : undefined;
  const maxMinutesError = draft.limitLength && wholeIn(draft.maxMinutes, 1, MAX_MINUTES) === null
    ? COPY.maxMinutes : undefined;
  const valid = ruleErrors.size === 0 && !maxTracksError && !maxMinutesError
    && countRules(draft.root) > 0 && countRules(draft.root) <= MAX_RULES && countNodes(draft.root) <= MAX_NODES;
  return { ruleErrors, maxTracksError, maxMinutesError, valid };
}

// ─── Draft ⇄ definition ───────────────────────────────────────────────────────

function groupToApi(group: DraftGroup): SmartGroup {
  return {
    mode: group.mode,
    children: group.children.map(child => {
      if (child.kind === 'group') return groupToApi(child);
      const { value } = ruleValue(child);
      const rule: SmartRule = { field: child.field, operator: child.operator };
      if (value !== undefined) rule.value = value;
      return rule;
    }),
  };
}

/** The definition a valid draft describes (check it with {@link checkDraft} first). */
export function draftToDefinition(draft: SmartDraft): SmartDefinition {
  const sort: SmartSortKey[] = [{ ...draft.sort }];
  if (draft.thenBy && draft.thenBy.field !== draft.sort.field) sort.push({ ...draft.thenBy });
  const limit: SmartLimit = { maxTracks: Number(draft.maxTracks.trim()) };
  if (draft.limitLength) limit.maxDurationSeconds = Number(draft.maxMinutes.trim()) * 60;
  return { schemaVersion: 1, root: groupToApi(draft.root), sort, limit };
}

function str(value: unknown): string {
  return typeof value === 'number' || typeof value === 'string' ? String(value) : '';
}

function ruleFromApi(rule: SmartRule): DraftRule | null {
  const spec = fieldSpec(rule.field);
  const condition = spec?.conditions.find(c => c.value === rule.operator);
  if (!spec || !condition) return null;
  const draft: DraftRule = { ...newRule(spec.field), operator: condition.value };
  const value = rule.value;
  switch (condition.kind) {
    case 'number_range':
      if (Array.isArray(value)) { draft.text = str(value[0]); draft.text2 = str(value[1]); }
      break;
    case 'rating':
      draft.rating = typeof value === 'number' ? value : null;
      break;
    case 'rating_range':
      if (Array.isArray(value)) {
        draft.rating = typeof value[0] === 'number' ? value[0] : null;
        draft.rating2 = typeof value[1] === 'number' ? value[1] : null;
      }
      break;
    case 'libraries':
      draft.libraries = Array.isArray(value) ? value.map(str).filter(Boolean) : [];
      break;
    case 'none':
      break;
    default:
      draft.text = str(value);
  }
  return draft;
}

function groupFromApi(group: SmartGroup): DraftGroup | null {
  const children: DraftNode[] = [];
  for (const child of group.children ?? []) {
    const node = isGroup(child) ? groupFromApi(child) : ruleFromApi(child);
    if (!node) return null;
    children.push(node);
  }
  return { kind: 'group', key: key(), mode: group.mode === 'any' ? 'any' : 'all', children };
}

const SORT_NAMES = new Set(SORT_FIELDS.map(s => s.field));

function sortFromApi(sort: unknown): DraftSort | null {
  const s = sort as Partial<SmartSortKey> | undefined;
  if (!s || !SORT_NAMES.has(s.field as SmartSortField)) return null;
  return { field: s.field as SmartSortField, direction: s.direction === 'desc' ? 'desc' : 'asc' };
}

// ─── Presets (Home) ───────────────────────────────────────────────────────────

/** Rules and a name that open in the editor ready to save. */
export interface SmartPreset { name: string; definition: SmartDefinition }

/** One genre, or any of several, newest additions first. */
export function genrePreset(genres: string[]): SmartPreset {
  return {
    name: genres.join(', '),
    definition: {
      schemaVersion: 1,
      root: { mode: genres.length > 1 ? 'any' : 'all', children: genres.map(genre => ({ field: 'genre', operator: 'is', value: genre })) },
      sort: [{ field: 'added_at', direction: 'desc' }],
    },
  };
}

export type SmartIdeaId = 'recently_added' | 'forgotten_favourites' | 'never_played' | 'most_played';

export const SMART_IDEAS: { id: SmartIdeaId; description: string; preset: SmartPreset }[] = [
  {
    id: 'recently_added', description: 'Added in the last 30 days',
    preset: { name: 'Recently added', definition: {
      schemaVersion: 1,
      root: { mode: 'all', children: [{ field: 'added_at', operator: 'in_last_days', value: 30 }] },
      sort: [{ field: 'added_at', direction: 'desc' }],
    } },
  },
  {
    id: 'forgotten_favourites', description: 'Rated 4+, not played in 90 days',
    preset: { name: 'Forgotten favourites', definition: {
      schemaVersion: 1,
      root: { mode: 'all', children: [
        { field: 'rating', operator: 'gte', value: 4 },
        { field: 'last_played_at', operator: 'not_in_last_days', value: 90 },
      ] },
      sort: [{ field: 'rating', direction: 'desc' }],
    } },
  },
  {
    id: 'never_played', description: "Tracks you haven't played yet",
    preset: { name: 'Never played', definition: {
      schemaVersion: 1,
      root: { mode: 'all', children: [{ field: 'last_played_at', operator: 'is_missing' }] },
      sort: [{ field: 'added_at', direction: 'desc' }],
    } },
  },
  {
    id: 'most_played', description: 'Your 50 most played tracks',
    preset: { name: 'Most played', definition: {
      schemaVersion: 1,
      root: { mode: 'all', children: [{ field: 'play_count', operator: 'gte', value: 1 }] },
      sort: [{ field: 'play_count', direction: 'desc' }],
      limit: { maxTracks: 50 },
    } },
  },
];

/** The editor draft for stored rules; `null` when this build can't edit them. */
export function definitionToDraft(definition: unknown, name: string): SmartDraft | null {
  const def = definition as Partial<SmartDefinition> | null;
  if (!def || def.schemaVersion !== 1 || !def.root || !isGroup(def.root)) return null;
  const root = groupFromApi(def.root);
  if (!root) return null;
  const sort = Array.isArray(def.sort) ? def.sort : [];
  const limit = def.limit ?? {};
  return {
    name,
    root,
    sort: sortFromApi(sort[0]) ?? { field: 'title', direction: 'asc' },
    thenBy: sortFromApi(sort[1]),
    maxTracks: String(limit.maxTracks ?? DEFAULT_MAX_TRACKS),
    limitLength: limit.maxDurationSeconds != null,
    maxMinutes: limit.maxDurationSeconds != null ? String(Math.round(limit.maxDurationSeconds / 60)) : '',
  };
}

// ─── Tree edits ───────────────────────────────────────────────────────────────

/** Replaces the node with `key` anywhere in the tree (`null` removes it). */
export function replaceNode(group: DraftGroup, nodeKey: string, next: DraftNode | null): DraftGroup {
  const children: DraftNode[] = [];
  for (const child of group.children) {
    if (child.key === nodeKey) {
      if (next) children.push(next);
    } else if (child.kind === 'group') {
      const updated = replaceNode(child, nodeKey, next);
      // A group never stays empty: removing its last rule removes the group.
      if (updated.children.length) children.push(updated);
    } else {
      children.push(child);
    }
  }
  return { ...group, children };
}

/** Appends `node` to the group with `groupKey`. */
export function appendTo(group: DraftGroup, groupKey: string, node: DraftNode): DraftGroup {
  if (group.key === groupKey) return { ...group, children: [...group.children, node] };
  return {
    ...group,
    children: group.children.map(child => (child.kind === 'group' ? appendTo(child, groupKey, node) : child)),
  };
}

/** Readable star count: "1 star", "4.5 stars". */
export function starsLabel(value: number): string {
  return value === 1 ? '1 star' : `${value} stars`;
}
