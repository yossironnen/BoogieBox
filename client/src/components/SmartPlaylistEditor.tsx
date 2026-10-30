/**
 * Smart playlist rule editor: rules (with nested groups), order and limit, and a live preview.
 * Creates a smart playlist or edits a saved one's rules. Wording: wip/f01-approved-copy.md.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { api, SmartPlaylistApiError } from '../api';
import type { Genre, Library, Playlist, SmartPlaylistPreview } from '../types';
import {
  COPY, FIELDS, MAX_RULES, MAX_NODES, SORT_FIELDS,
  appendTo, canNestGroup, checkDraft, conditionSpec, countNodes, countRules, definitionToDraft, directionLabels,
  draftToDefinition, fieldSpec, newDraft, newGroup, newRule, replaceNode, withField, withOperator,
  type DraftGroup, type DraftNode, type DraftRule, type DraftSort, type SmartDraft, type SmartField, type SmartSortField,
} from '../smartPlaylistRules';
import { hybridControlStyles, hybridMediaStyles } from '../hybridPreview';
import StarRating from './StarRating';

// ─── Icons ────────────────────────────────────────────────────────────────────

/** The smart playlist glyph (a funnel), shared with the playlist views. */
export const SMART_ICON_PATH = 'M3 5h18l-7 8v6l-4 2v-8z';

export function SmartIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={SMART_ICON_PATH} />
    </svg>
  );
}

function PathIcon({ d, size = 16, accent = false }: { d: string; size?: number; accent?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"
      style={accent ? { color: 'var(--accent)', opacity: 0.85, flexShrink: 0 } : { flexShrink: 0 }}>
      <path d={d} />
    </svg>
  );
}

const ICON = {
  close: 'M18 6 6 18M6 6l12 12',
  plus: 'M12 5v14M5 12h14',
  group: 'M3 3h18v18H3zM12 8v8M8 12h8',
  groupHead: 'M3 3h18v18H3zM8 9h8M8 15h5',
  trash: 'M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14',
  sort: 'M3 6h11M3 12h8M3 18h5M17 4v16M14 17l3 3 3-3',
  limit: 'M3 6h13M3 12h13M3 18h9M20 4v16',
  tracks: 'M9 18V5l12-2v13M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 19a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  clock: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM12 7v5l3 2',
  alert: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM12 8v5M12 16h.01',
  warn: 'M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
  sync: 'M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5',
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/** Same format as the playlist views ("2h 51m"). */
export function lengthLabel(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function trackLength(seconds: number | null): string {
  if (!seconds) return '–';
  return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}

function sameName(a: string, b: string): boolean {
  const norm = (s: string) => s.trim().replace(/\s+/g, ' ').toLowerCase();
  return norm(a) === norm(b);
}

function rangeNoun(field: SmartField): string {
  return field === 'bpm' ? 'BPM' : fieldSpec(field)!.label.toLowerCase();
}

/** Suggestions for artist/album text, looked up as the user types. */
function useNameSuggestions(field: SmartField, text: string): string[] {
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => {
    const q = text.trim();
    if ((field !== 'artist' && field !== 'album') || q.length < 2) { setNames([]); return; }
    let canceled = false;
    const timer = window.setTimeout(() => {
      api.search({ q, limit: 8, include_artists: field === 'artist', include_albums: field === 'album' })
        .then(result => {
          if (canceled) return;
          const list = field === 'artist'
            ? (result.artists ?? []).map(a => a.name)
            : (result.albums ?? []).map(a => a.title);
          setNames(Array.from(new Set(list.filter(Boolean))).slice(0, 8));
        })
        .catch(() => { if (!canceled) setNames([]); });
    }, 250);
    return () => { canceled = true; window.clearTimeout(timer); };
  }, [field, text]);
  return names;
}

type PreviewState =
  | { status: 'idle' }
  | { status: 'loading'; last: SmartPlaylistPreview | null }
  | { status: 'ready'; preview: SmartPlaylistPreview }
  | { status: 'error' };

/** Debounced preview; a newer draft discards older answers. */
function usePreview(definitionJson: string | null): { state: PreviewState; retry: () => void } {
  const [state, setState] = useState<PreviewState>({ status: 'idle' });
  const [attempt, setAttempt] = useState(0);
  const lastRef = useRef<SmartPlaylistPreview | null>(null);
  useEffect(() => {
    if (!definitionJson) {
      // Rules being edited into shape: keep showing the last result instead of a stuck "Updating".
      setState(lastRef.current ? { status: 'ready', preview: lastRef.current } : { status: 'idle' });
      return;
    }
    const controller = new AbortController();
    let timer = 0;
    const run = () => {
      api.playlists.smartPreview(JSON.parse(definitionJson), controller.signal)
        .then(preview => {
          if (controller.signal.aborted) return;
          lastRef.current = preview;
          setState({ status: 'ready', preview });
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return;
          // Too many previews at once: try again shortly rather than show an error.
          if (error instanceof SmartPlaylistApiError && error.code === 'busy') {
            timer = window.setTimeout(run, 600);
            return;
          }
          setState({ status: 'error' });
        });
    };
    setState({ status: 'loading', last: lastRef.current });
    timer = window.setTimeout(run, 300);
    return () => { controller.abort(); window.clearTimeout(timer); };
  }, [definitionJson, attempt]);
  const retry = useCallback(() => setAttempt(n => n + 1), []);
  return { state, retry };
}

// ─── Rule rows ────────────────────────────────────────────────────────────────

interface TreeContext {
  libraries: Library[];
  genres: Genre[];
  missingLibraryIds: Set<string>;
  errorFor: (rule: DraftRule) => string | undefined;
  touch: (key: string) => void;
  update: (key: string, next: DraftNode | null) => void;
  append: (groupKey: string, node: DraftNode) => void;
  canAddRule: boolean;
  canAddGroup: boolean;
  readOnly: boolean;
  /** Phone layout: stacked rows, 44px touch targets. */
  mobile: boolean;
}

/** Width for a value control; on phones wide controls fill the row. */
function valueWidth(ctx: TreeContext, px: number): React.CSSProperties {
  if (!ctx.mobile) return { width: px };
  return px >= 160 ? { flex: 1, minWidth: 0, width: '100%' } : { width: px + 24 };
}

function ValueEditor({ rule, onChange, onBlur, ctx, invalid }: {
  rule: DraftRule;
  onChange: (next: DraftRule) => void;
  onBlur: () => void;
  ctx: TreeContext;
  invalid: boolean;
}) {
  const spec = fieldSpec(rule.field)!;
  const kind = conditionSpec(rule.field, rule.operator)?.kind ?? 'none';
  const suggestions = useNameSuggestions(rule.field, kind === 'text' ? rule.text : '');
  const listId = `smart-suggest-${rule.key}`;
  const st = ctx.mobile ? MS : S;
  const inputStyle = { ...st.value, ...(invalid ? S.invalid : {}) };
  const set = (patch: Partial<DraftRule>) => onChange({ ...rule, ...patch });

  switch (kind) {
    case 'none':
      return null;
    case 'text':
      return (
        <>
          <input type="text" aria-label={spec.valueLabel} placeholder={spec.placeholder} value={rule.text}
            aria-invalid={invalid || undefined} list={listId} maxLength={128} style={{ ...inputStyle, ...valueWidth(ctx, 200) }}
            onChange={e => set({ text: e.target.value })} onBlur={onBlur} />
          <datalist id={listId}>{suggestions.map(name => <option key={name} value={name} />)}</datalist>
        </>
      );
    case 'genre':
      return (
        <>
          <input type="search" aria-label={spec.valueLabel} placeholder={spec.placeholder} value={rule.text}
            aria-invalid={invalid || undefined} list={listId} maxLength={128} style={{ ...inputStyle, ...valueWidth(ctx, 200) }}
            onChange={e => set({ text: e.target.value })} onBlur={onBlur} />
          <datalist id={listId}>{ctx.genres.map(g => <option key={g.genre} value={g.genre} />)}</datalist>
        </>
      );
    case 'number':
    case 'days':
      return (
        <>
          <input type="text" inputMode={rule.field === 'bpm' ? 'decimal' : 'numeric'} aria-label={spec.valueLabel}
            value={rule.text} aria-invalid={invalid || undefined} style={{ ...inputStyle, ...valueWidth(ctx, 90) }}
            onChange={e => set({ text: e.target.value })} onBlur={onBlur} />
          {spec.suffix && <span style={S.muted}>{spec.suffix}</span>}
        </>
      );
    case 'number_range':
      return (
        <>
          <input type="text" inputMode={rule.field === 'bpm' ? 'decimal' : 'numeric'} aria-label={`From ${rangeNoun(rule.field)}`}
            value={rule.text} aria-invalid={invalid || undefined} style={{ ...inputStyle, ...valueWidth(ctx, 76) }}
            onChange={e => set({ text: e.target.value })} onBlur={onBlur} />
          <span style={S.muted}>and</span>
          <input type="text" inputMode={rule.field === 'bpm' ? 'decimal' : 'numeric'} aria-label={`To ${rangeNoun(rule.field)}`}
            value={rule.text2} aria-invalid={invalid || undefined} style={{ ...inputStyle, ...valueWidth(ctx, 76) }}
            onChange={e => set({ text2: e.target.value })} onBlur={onBlur} />
          {spec.suffix && <span style={S.muted}>{spec.suffix}</span>}
        </>
      );
    case 'rating':
      return (
        <StarRating ariaLabel="Rating" value={rule.rating} size={ctx.mobile ? 'hero' : 'regular'}
          onChange={value => { set({ rating: value }); onBlur(); }} />
      );
    case 'rating_range':
      return (
        <>
          <StarRating ariaLabel="From rating" value={rule.rating} size={ctx.mobile ? 'hero' : undefined}
            onChange={value => { set({ rating: value }); onBlur(); }} />
          <span style={S.muted}>and</span>
          <StarRating ariaLabel="To rating" value={rule.rating2} size={ctx.mobile ? 'hero' : undefined}
            onChange={value => { set({ rating2: value }); onBlur(); }} />
        </>
      );
    case 'date':
      return (
        <input type="date" aria-label="Date" value={rule.text} aria-invalid={invalid || undefined}
          style={{ ...inputStyle, ...valueWidth(ctx, 160) }} onChange={e => set({ text: e.target.value })} onBlur={onBlur} />
      );
    case 'libraries': {
      const known = new Set(ctx.libraries.map(l => String(l.id)));
      const removed = rule.libraries.filter(id => !known.has(id));
      const toggle = (id: string) => {
        set({ libraries: rule.libraries.includes(id) ? rule.libraries.filter(x => x !== id) : [...rule.libraries, id] });
        onBlur();
      };
      return (
        <span style={{ display: 'inline-flex', flexWrap: 'wrap', gap: 6 }}>
          {ctx.libraries.map(lib => {
            const id = String(lib.id);
            const on = rule.libraries.includes(id);
            return (
              <button key={id} type="button" aria-pressed={on} style={{ ...st.chip, ...(on ? S.chipOn : {}) }}
                onClick={() => toggle(id)}>{lib.name}</button>
            );
          })}
          {removed.map(id => (
            <button key={id} type="button" aria-pressed="true" style={{ ...st.chip, ...S.chipRemoved }}
              onClick={() => toggle(id)}>
              <PathIcon d={ICON.warn} size={13} />{COPY.removedLibrary}
            </button>
          ))}
        </span>
      );
    }
  }
}

function RuleRow({ rule, canRemove, ctx }: { rule: DraftRule; canRemove: boolean; ctx: TreeContext }) {
  const spec = fieldSpec(rule.field)!;
  const error = ctx.errorFor(rule);
  const known = new Set(ctx.libraries.map(l => String(l.id)));
  const hasRemovedLibrary = rule.field === 'library_id'
    && rule.libraries.some(id => !known.has(id) || ctx.missingLibraryIds.has(id));
  let hint: string | null = null;
  if (rule.field === 'last_played_at' && rule.operator === 'not_in_last_days') hint = COPY.neverPlayedHint;
  if (rule.field === 'added_at') hint = COPY.addedAtHint;
  const change = (next: DraftRule) => ctx.update(rule.key, next);
  const st = ctx.mobile ? MS : S;
  const fieldSelect = (
    <select aria-label="Field" value={rule.field} style={{ ...st.select, ...(ctx.mobile ? { flex: 1, minWidth: 0 } : { width: 150 }) }}
      onChange={e => change(withField(rule, e.target.value as SmartField))}>
      {FIELDS.map(f => <option key={f.field} value={f.field}>{f.label}</option>)}
    </select>
  );
  const conditionSelect = (
    <select aria-label="Condition" value={rule.operator} style={{ ...st.select, ...(ctx.mobile ? { width: '100%' } : { width: 170 }) }}
      onChange={e => { change(withOperator(rule, e.target.value)); ctx.touch(rule.key); }}>
      {spec.conditions.map(c => <option key={c.value} value={c.value}>{c.label}</option>)}
    </select>
  );
  const value = <ValueEditor rule={rule} ctx={ctx} invalid={!!error} onChange={change} onBlur={() => ctx.touch(rule.key)} />;
  const removeButton = (
    <button type="button" aria-label="Remove rule" title="Remove rule" style={st.smallIconBtn}
      disabled={!canRemove || ctx.readOnly} onClick={() => ctx.update(rule.key, null)}>
      <PathIcon d={ICON.close} size={ctx.mobile ? 16 : 14} />
    </button>
  );
  const frame = { ...st.ruleRow, ...(error ? S.ruleRowInvalid : hasRemovedLibrary ? S.ruleRowWarn : {}) };
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      {ctx.mobile ? (
        <div style={frame}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <PathIcon d={spec.icon} accent />
            {fieldSelect}
            {removeButton}
          </div>
          {conditionSelect}
          {(conditionSpec(rule.field, rule.operator)?.kind ?? 'none') !== 'none' && <div style={st.valueSlot}>{value}</div>}
        </div>
      ) : (
        <div style={frame}>
          <PathIcon d={spec.icon} accent />
          {fieldSelect}
          {conditionSelect}
          <div style={S.valueSlot}>{value}</div>
          {removeButton}
        </div>
      )}
      {error && <div style={S.errorLine}>{error}</div>}
      {!error && hasRemovedLibrary && <div style={S.warnLine}>{COPY.removedLibraryHint}</div>}
      {!error && !hasRemovedLibrary && hint && <div style={st.hintLine}>{hint}</div>}
    </div>
  );
}

function AddButtons({ group, depth, ctx }: { group: DraftGroup; depth: number; ctx: TreeContext }) {
  const nestable = canNestGroup(depth);
  const groupBlocked = !ctx.canAddGroup || !nestable;
  const st = ctx.mobile ? MS : S;
  return (
    <div style={ctx.mobile ? MS.addRow : { display: 'flex', gap: 8, flexWrap: 'wrap' }}>
      <button type="button" style={{ ...st.addBtn, ...(!ctx.canAddRule ? hybridControlStyles.disabled : {}) }}
        disabled={!ctx.canAddRule || ctx.readOnly} title={!ctx.canAddRule ? COPY.ruleLimit : undefined}
        onClick={() => ctx.append(group.key, newRule())}>
        <PathIcon d={ICON.plus} size={14} />Add rule
      </button>
      <button type="button" style={{ ...st.addBtn, ...(groupBlocked ? hybridControlStyles.disabled : {}) }}
        disabled={groupBlocked || ctx.readOnly}
        title={!nestable ? COPY.depthLimit : !ctx.canAddGroup ? COPY.ruleLimit : undefined}
        onClick={() => ctx.append(group.key, newGroup())}>
        <PathIcon d={ICON.group} size={14} />Add group
      </button>
    </div>
  );
}

function GroupChildren({ group, depth, ctx }: { group: DraftGroup; depth: number; ctx: TreeContext }) {
  // The last rule of the whole playlist can't be removed; a nested group's last rule removes the group.
  const onlyRootRule = depth === 1 && group.children.length === 1;
  return (
    <>
      {group.children.map(child => (child.kind === 'group'
        ? <GroupBox key={child.key} group={child} depth={depth + 1} ctx={ctx} />
        : <RuleRow key={child.key} rule={child} canRemove={!onlyRootRule} ctx={ctx} />))}
    </>
  );
}

function GroupBox({ group, depth, ctx }: { group: DraftGroup; depth: number; ctx: TreeContext }) {
  return (
    <div role="group" aria-label="Rule group" style={S.group}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: ctx.mobile ? 15 : 14, flexWrap: ctx.mobile ? 'wrap' : undefined }}>
        <PathIcon d={ICON.groupHead} accent />
        <span>Match</span>
        <select aria-label="Match all or any rule in this group" value={group.mode} style={ctx.mobile ? MS.modeSelect : S.modeSelect}
          onChange={e => ctx.update(group.key, { ...group, mode: e.target.value as 'all' | 'any' })}>
          <option value="all">all</option>
          <option value="any">any</option>
        </select>
        <span style={{ flex: 1 }}>of these</span>
        {ctx.mobile ? (
          <button type="button" aria-label="Remove group" title="Remove group" style={{ ...MS.smallIconBtn, marginLeft: 'auto' }}
            disabled={ctx.readOnly} onClick={() => ctx.update(group.key, null)}>
            <PathIcon d={ICON.trash} size={16} />
          </button>
        ) : (
          <button type="button" style={S.removeGroupBtn} disabled={ctx.readOnly} onClick={() => ctx.update(group.key, null)}>
            <PathIcon d={ICON.trash} size={13} />Remove group
          </button>
        )}
      </div>
      <GroupChildren group={group} depth={depth} ctx={ctx} />
      <AddButtons group={group} depth={depth} ctx={ctx} />
    </div>
  );
}

// ─── Order and limit ──────────────────────────────────────────────────────────

function SortSelects({ sort, onChange, fieldLabel, directionLabel, exclude, fieldId, mobile = false }: {
  sort: DraftSort;
  mobile?: boolean;
  onChange: (next: DraftSort) => void;
  fieldLabel?: string;
  directionLabel: string;
  exclude?: SmartSortField;
  fieldId?: string;
}) {
  const [asc, desc] = directionLabels(sort.field);
  return (
    <>
      <select id={fieldId} aria-label={fieldLabel} value={sort.field} style={mobile ? MS.select : { ...S.select, width: 130 }}
        onChange={e => onChange({ ...sort, field: e.target.value as SmartSortField })}>
        {SORT_FIELDS.filter(f => f.field !== exclude).map(f => <option key={f.field} value={f.field}>{f.label}</option>)}
      </select>
      <select aria-label={directionLabel} value={sort.direction} style={mobile ? MS.select : { ...S.select, width: 128 }}
        onChange={e => onChange({ ...sort, direction: e.target.value as 'asc' | 'desc' })}>
        <option value="asc">{asc}</option>
        <option value="desc">{desc}</option>
      </select>
    </>
  );
}

// ─── Preview ──────────────────────────────────────────────────────────────────

function previewOf(state: PreviewState): SmartPlaylistPreview | null {
  return state.status === 'ready' ? state.preview : state.status === 'loading' ? state.last : null;
}

function PreviewPanel({ state, onRetry, mobile = false, panelRef }: {
  state: PreviewState;
  onRetry: () => void;
  mobile?: boolean;
  panelRef?: React.Ref<HTMLElement>;
}) {
  const preview = previewOf(state);
  return (
    <aside ref={panelRef} id="smart-preview" aria-labelledby="smart-preview-title" style={mobile ? MS.preview : S.preview}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
        <div id="smart-preview-title" style={S.sectionTitle}>Preview</div>
        <div role="status" style={{ fontSize: 12, color: 'var(--text-muted)' }}>
          {state.status === 'loading' ? 'Updating preview…' : ''}
        </div>
      </div>
      {state.status === 'error' && (
        <div role="alert" style={{ ...S.stateBox, ...S.stateBoxError }}>
          <span style={{ color: 'var(--danger, #d65c5c)' }}><PathIcon d={ICON.alert} size={28} /></span>
          <div style={{ fontSize: 14, fontWeight: 650 }}>Couldn&rsquo;t update the preview.</div>
          <button type="button" style={S.addBtn} onClick={onRetry}>Try again</button>
        </div>
      )}
      {state.status !== 'error' && preview && (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 }}>
            <div style={S.tile}>
              <PathIcon d={ICON.tracks} size={20} accent />
              <div style={S.tileValue}>{preview.trackCount.toLocaleString()}</div>
              <div style={S.tileLabel}>Tracks</div>
            </div>
            <div style={S.tile}>
              <PathIcon d={ICON.clock} size={20} accent />
              <div style={S.tileValue}>{lengthLabel(preview.totalDuration)}</div>
              <div style={S.tileLabel}>Total length</div>
            </div>
          </div>
          {preview.trackCount === 0 ? (
            <div role="status" style={S.stateBox}>
              <span style={{ color: 'var(--accent)', opacity: 0.85 }}><SmartIcon size={28} /></span>
              <div style={{ fontSize: 14, fontWeight: 650 }}>No tracks match these rules.</div>
              <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>You can still save it. Tracks appear when they match.</div>
            </div>
          ) : (
            <>
              <ol aria-label="First tracks" style={S.sample}>
                {preview.tracks.map((track, i) => (
                  <li key={String(track.id)} style={S.sampleRow}>
                    <span style={{ width: 20, color: 'var(--text-muted)', textAlign: 'right', flexShrink: 0 }}>{i + 1}</span>
                    <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {track.title || track.file_name}
                      {track.artist && <span style={{ color: 'var(--text-muted)' }}> · {track.artist}</span>}
                    </span>
                    <span style={{ color: 'var(--text-muted)', flexShrink: 0, fontVariantNumeric: 'tabular-nums' }}>{trackLength(track.duration)}</span>
                  </li>
                ))}
              </ol>
              {preview.trackCount > preview.tracks.length && (
                <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>Showing the first 20.</div>
              )}
            </>
          )}
        </>
      )}
    </aside>
  );
}

/** Phone footer: "N tracks · <duration>" that shows or hides the preview. */
function PreviewSummary({ state, expanded, onToggle }: { state: PreviewState; expanded: boolean; onToggle: () => void }) {
  const preview = previewOf(state);
  if (!preview && state.status !== 'error') return null;
  return (
    <button type="button" aria-expanded={expanded} aria-controls="smart-preview" style={MS.summaryBtn} onClick={onToggle}>
      <PathIcon d={ICON.tracks} accent />
      <span style={{ flex: 1, minWidth: 0, textAlign: 'left' }}>
        {preview && state.status !== 'error' ? (
          <>
            <span style={{ fontWeight: 700 }}>{preview.trackCount.toLocaleString()} tracks</span>
            <span style={{ color: 'var(--text-muted)' }}> · {lengthLabel(preview.totalDuration)}</span>
          </>
        ) : <span style={{ color: 'var(--danger, #d65c5c)' }}>Couldn&rsquo;t update the preview.</span>}
      </span>
      <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>Show tracks</span>
    </button>
  );
}

// ─── Editor ───────────────────────────────────────────────────────────────────

export interface SmartPlaylistEditorProps {
  /** The smart playlist whose rules are edited; omitted to create one. */
  playlist?: Playlist | null;
  /** Names already in use (create and rename checks). */
  existingNames: string[];
  onClose: () => void;
  onSaved: (playlist: Playlist) => void;
  /** `mobile`: a full-height sheet with stacked rules and the preview behind "Show tracks". */
  variant?: 'desktop' | 'mobile';
}

type LoadState = 'loading' | 'ready' | 'unsupported' | 'failed';

export default function SmartPlaylistEditor({ playlist, existingNames, onClose, onSaved, variant = 'desktop' }: SmartPlaylistEditorProps) {
  const editing = !!playlist;
  const mobile = variant === 'mobile';
  const [showTracks, setShowTracks] = useState(false);
  const previewRef = useRef<HTMLElement>(null);
  const [draft, setDraft] = useState<SmartDraft>(() => newDraft());
  const [load, setLoad] = useState<LoadState>(editing ? 'loading' : 'ready');
  const [editRevision, setEditRevision] = useState(0);
  const [missingLibraryIds, setMissingLibraryIds] = useState<Set<string>>(new Set());
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [genres, setGenres] = useState<Genre[]>([]);
  const [touched, setTouched] = useState<Set<string>>(new Set());
  const [showAllErrors, setShowAllErrors] = useState(false);
  const [nameError, setNameError] = useState('');
  const [saveError, setSaveError] = useState('');
  const [conflictRevision, setConflictRevision] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const trigger = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    dialog?.showModal?.();
    return () => { dialog?.close?.(); if (trigger?.isConnected) trigger.focus(); };
  }, []);

  useEffect(() => {
    api.libraries.list().then(setLibraries).catch(() => setLibraries([]));
    api.genres().then(setGenres).catch(() => setGenres([]));
  }, []);

  const loadDefinition = useCallback(async () => {
    if (!playlist) return;
    setLoad('loading');
    try {
      const view = await api.playlists.smartDefinition(playlist.id);
      const next = view.supported ? definitionToDraft(view.definition, playlist.name) : null;
      setEditRevision(view.editRevision);
      setMissingLibraryIds(new Set(view.missingLibraryIds));
      if (!next) { setLoad('unsupported'); return; }
      setDraft(next);
      setTouched(new Set());
      setShowAllErrors(true);
      setLoad('ready');
    } catch (error) {
      // The server logs its own failures; this records the client-side ones (network, unexpected answers).
      console.error('Could not load saved smart playlist rules', error);
      setLoad('failed');
    }
  }, [playlist]);

  useEffect(() => { void loadDefinition(); }, [loadDefinition]);
  // On phones the keyboard would cover the sheet; the dialog focuses the close button instead.
  useEffect(() => { if (load === 'ready' && !editing && !mobile) nameRef.current?.focus(); }, [load, editing, mobile]);
  useEffect(() => { if (showTracks) previewRef.current?.scrollIntoView?.({ block: 'nearest' }); }, [showTracks]);

  const check = useMemo(() => checkDraft(draft), [draft]);
  const definitionJson = useMemo(
    () => (load === 'ready' && check.valid ? JSON.stringify(draftToDefinition(draft)) : null),
    [draft, check.valid, load],
  );
  const { state: previewState, retry } = usePreview(definitionJson);

  const readOnly = load !== 'ready';
  const ruleCount = countRules(draft.root);
  const nodeCount = countNodes(draft.root);
  const ctx: TreeContext = {
    libraries,
    genres,
    missingLibraryIds,
    errorFor: rule => (showAllErrors || touched.has(rule.key) ? check.ruleErrors.get(rule.key) : undefined),
    touch: k => setTouched(prev => (prev.has(k) ? prev : new Set(prev).add(k))),
    update: (k, next) => setDraft(d => ({ ...d, root: replaceNode(d.root, k, next) })),
    append: (groupKey, node) => setDraft(d => ({ ...d, root: appendTo(d.root, groupKey, node) })),
    canAddRule: ruleCount < MAX_RULES && nodeCount < MAX_NODES,
    canAddGroup: ruleCount < MAX_RULES && nodeCount + 2 <= MAX_NODES,
    readOnly,
    mobile,
  };
  const st = mobile ? MS : S;

  const visibleErrors = (showAllErrors ? check.ruleErrors.size : [...check.ruleErrors.keys()].filter(k => touched.has(k)).length) > 0
    || check.maxTracksError || check.maxMinutesError;
  const canSave = load === 'ready' && check.valid && !!draft.name.trim() && !saving;

  const save = async (revision = editRevision) => {
    if (!canSave) {
      setShowAllErrors(true);
      if (!draft.name.trim()) nameRef.current?.focus();
      return;
    }
    const name = draft.name.trim();
    const renamed = !playlist || !sameName(name, playlist.name);
    if (renamed && existingNames.some(existing => sameName(existing, name))) {
      setNameError('A playlist with this name already exists');
      return;
    }
    setSaving(true);
    setSaveError('');
    setNameError('');
    try {
      const definition = draftToDefinition(draft);
      if (!playlist) {
        onSaved(await api.playlists.createSmart(name, definition));
        return;
      }
      const view = await api.playlists.updateSmartDefinition(playlist.id, definition, revision);
      setEditRevision(view.editRevision);
      setConflictRevision(null);
      let saved = playlist;
      if (renamed) {
        saved = await api.playlists.update(playlist.id, name, playlist.description ?? '');
      }
      onSaved(saved);
    } catch (error) {
      if (error instanceof SmartPlaylistApiError && error.code === 'edit_conflict') {
        setConflictRevision((error.body as { editRevision?: number } | null)?.editRevision ?? revision);
      } else if (error instanceof SmartPlaylistApiError && error.code === 'unsupported') {
        setLoad('unsupported');
      } else if (error instanceof Error && /already exists/i.test(error.message)) {
        setNameError('A playlist with this name already exists');
      } else {
        setSaveError('Could not save the rules.');
      }
    } finally {
      setSaving(false);
    }
  };

  const title = editing ? 'Edit rules' : 'New smart playlist';

  return createPortal(
    <dialog ref={dialogRef} aria-labelledby="smart-editor-title"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClick={e => { if (e.target === e.currentTarget) onClose(); }}
      style={st.overlay}>
      <div style={st.dialog}>
        {mobile && <div aria-hidden="true" style={MS.handle} />}
        <div style={st.header}>
          <div>
            {!mobile && <div style={S.eyebrow}>Smart playlist</div>}
            <div id="smart-editor-title" style={S.title}>{title}</div>
          </div>
          <button type="button" aria-label="Close rules" title="Close rules" style={st.closeBtn} onClick={onClose}>
            <PathIcon d={ICON.close} size={mobile ? 18 : 16} />
          </button>
        </div>
        <div style={mobile ? MS.scroll : undefined}>

        {load === 'loading' && <div style={S.loading}>Loading…</div>}
        {load === 'failed' && (
          <div role="alert" style={{ ...S.loading, color: 'var(--danger, #d65c5c)' }}>Could not load saved rules.</div>
        )}

        {(load === 'ready' || load === 'unsupported') && (
          <div className={mobile ? undefined : 'smart-editor-body'}>
            <fieldset disabled={readOnly} style={st.main}>
              {load === 'unsupported' && (
                <div role="alert" style={S.errorLine}>
                  These rules were made by a newer version of BoogieBox and can&rsquo;t be edited here.
                </div>
              )}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <label htmlFor="smart-name" style={S.label}>Playlist name</label>
                <input id="smart-name" ref={nameRef} type="text" value={draft.name} maxLength={200}
                  style={{ ...st.nameInput, ...(nameError ? S.invalid : {}) }}
                  aria-invalid={nameError ? true : undefined}
                  onChange={e => { setNameError(''); setDraft(d => ({ ...d, name: e.target.value })); }} />
                {nameError && <div style={S.errorLine}>{nameError}</div>}
              </div>

              <section aria-labelledby="smart-rules-title" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                <div id="smart-rules-title" style={S.sectionTitle}>Rules</div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: mobile ? 15 : 14, flexWrap: 'wrap' }}>
                  <span>Tracks that match</span>
                  <select aria-label="Match all or any rule" value={draft.root.mode} style={st.modeSelect}
                    onChange={e => setDraft(d => ({ ...d, root: { ...d.root, mode: e.target.value as 'all' | 'any' } }))}>
                    <option value="all">all</option>
                    <option value="any">any</option>
                  </select>
                  <span>of these rules</span>
                </div>
                <GroupChildren group={draft.root} depth={1} ctx={ctx} />
                <AddButtons group={draft.root} depth={1} ctx={ctx} />
              </section>

              {mobile ? (
                <details open style={MS.orderBox}>
                  <summary style={MS.orderSummary}>
                    <PathIcon d={ICON.sort} accent />Order and limit
                  </summary>
                  <div style={{ padding: '0 12px 12px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                    <label htmlFor="smart-sort" style={S.label}>Order by</label>
                    <SortSelects mobile fieldId="smart-sort" sort={draft.sort} directionLabel="Direction"
                      onChange={sort => setDraft(d => ({ ...d, sort, thenBy: d.thenBy?.field === sort.field ? null : d.thenBy }))} />
                    {draft.thenBy ? (
                      <>
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                          <span style={S.label}>then by</span>
                          <button type="button" aria-label="Remove second order" title="Remove second order" style={MS.smallIconBtn}
                            onClick={() => setDraft(d => ({ ...d, thenBy: null }))}>
                            <PathIcon d={ICON.close} size={16} />
                          </button>
                        </div>
                        <SortSelects mobile sort={draft.thenBy} fieldLabel="Then by" directionLabel="Then by direction"
                          exclude={draft.sort.field} onChange={thenBy => setDraft(d => ({ ...d, thenBy }))} />
                      </>
                    ) : (
                      <button type="button" style={{ ...MS.addBtn, alignSelf: 'flex-start' }}
                        onClick={() => setDraft(d => ({
                          ...d,
                          thenBy: { field: SORT_FIELDS.find(f => f.field !== d.sort.field)!.field, direction: 'asc' },
                        }))}>
                        <PathIcon d={ICON.plus} size={15} />Add a second order
                      </button>
                    )}
                    <div style={MS.lineRow}>
                      <PathIcon d={ICON.limit} accent />
                      <label htmlFor="smart-max">Up to</label>
                      <input id="smart-max" type="text" inputMode="numeric" value={draft.maxTracks}
                        aria-invalid={check.maxTracksError ? true : undefined}
                        style={{ ...MS.value, width: 96, ...(check.maxTracksError ? S.invalid : {}) }}
                        onChange={e => setDraft(d => ({ ...d, maxTracks: e.target.value }))} />
                      <span style={MS.muted}>tracks</span>
                    </div>
                    <label style={{ display: 'flex', alignItems: 'center', gap: 10, minHeight: 44, fontSize: 15 }}>
                      <input type="checkbox" checked={draft.limitLength} style={{ accentColor: 'var(--accent)', width: 20, height: 20 }}
                        onChange={e => setDraft(d => ({ ...d, limitLength: e.target.checked }))} />
                      Limit the total length to
                    </label>
                    {draft.limitLength && (
                      <div style={MS.lineRow}>
                        <input type="text" inputMode="numeric" aria-label="Minutes" value={draft.maxMinutes}
                          aria-invalid={check.maxMinutesError ? true : undefined}
                          style={{ ...MS.value, width: 96, ...(check.maxMinutesError ? S.invalid : {}) }}
                          onChange={e => setDraft(d => ({ ...d, maxMinutes: e.target.value }))} />
                        <span style={MS.muted}>minutes</span>
                      </div>
                    )}
                    {check.maxTracksError && <div style={S.errorLine}>{check.maxTracksError}</div>}
                    {check.maxMinutesError && <div style={S.errorLine}>{check.maxMinutesError}</div>}
                  </div>
                </details>
              ) : (
                <section aria-labelledby="smart-order-title" style={S.orderSection}>
                  <div id="smart-order-title" style={S.sectionTitle}>Order and limit</div>
                  <div style={S.lineRow}>
                    <PathIcon d={ICON.sort} accent />
                    <label htmlFor="smart-sort" style={{ minWidth: 64 }}>Order by</label>
                    <SortSelects fieldId="smart-sort" sort={draft.sort} directionLabel="Direction"
                      onChange={sort => setDraft(d => ({ ...d, sort, thenBy: d.thenBy?.field === sort.field ? null : d.thenBy }))} />
                    {draft.thenBy ? (
                      <>
                        <span style={S.muted}>then by</span>
                        <SortSelects sort={draft.thenBy} fieldLabel="Then by" directionLabel="Then by direction"
                          exclude={draft.sort.field} onChange={thenBy => setDraft(d => ({ ...d, thenBy }))} />
                        <button type="button" aria-label="Remove second order" title="Remove second order" style={S.smallIconBtn}
                          onClick={() => setDraft(d => ({ ...d, thenBy: null }))}>
                          <PathIcon d={ICON.close} size={14} />
                        </button>
                      </>
                    ) : (
                      <button type="button" style={S.addBtn}
                        onClick={() => setDraft(d => ({
                          ...d,
                          thenBy: { field: SORT_FIELDS.find(f => f.field !== d.sort.field)!.field, direction: 'asc' },
                        }))}>
                        <PathIcon d={ICON.plus} size={14} />Add a second order
                      </button>
                    )}
                  </div>
                  <div style={S.lineRow}>
                    <PathIcon d={ICON.limit} accent />
                    <label htmlFor="smart-max" style={{ minWidth: 64 }}>Up to</label>
                    <input id="smart-max" type="text" inputMode="numeric" value={draft.maxTracks}
                      aria-invalid={check.maxTracksError ? true : undefined}
                      style={{ ...S.value, width: 90, ...(check.maxTracksError ? S.invalid : {}) }}
                      onChange={e => setDraft(d => ({ ...d, maxTracks: e.target.value }))} />
                    <span style={S.muted}>tracks</span>
                    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, marginLeft: 12, minHeight: 36 }}>
                      <input type="checkbox" checked={draft.limitLength} style={{ accentColor: 'var(--accent)', width: 16, height: 16 }}
                        onChange={e => setDraft(d => ({ ...d, limitLength: e.target.checked }))} />
                      Limit the total length to
                    </label>
                    <input type="text" inputMode="numeric" aria-label="Minutes" value={draft.maxMinutes} disabled={!draft.limitLength}
                      aria-invalid={check.maxMinutesError ? true : undefined}
                      style={{ ...S.value, width: 80, ...(check.maxMinutesError ? S.invalid : {}) }}
                      onChange={e => setDraft(d => ({ ...d, maxMinutes: e.target.value }))} />
                    <span style={S.muted}>minutes</span>
                  </div>
                  {check.maxTracksError && <div style={S.errorLine}>{check.maxTracksError}</div>}
                  {check.maxMinutesError && <div style={S.errorLine}>{check.maxMinutesError}</div>}
                </section>
              )}
            </fieldset>
            {(!mobile || showTracks) && (
              <PreviewPanel state={previewState} onRetry={retry} mobile={mobile} panelRef={previewRef} />
            )}
          </div>
        )}

        {conflictRevision !== null && (
          <div style={st.conflict}>
            <div role="alert" style={S.conflictText}>
              <span style={{ color: 'var(--warning, #d49a45)' }}><PathIcon d={ICON.sync} size={18} /></span>
              These rules were changed on another device while you were editing.
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, padding: '12px 16px', flexWrap: 'wrap' }}>
              <button type="button" style={S.cancelBtn}
                onClick={() => { setConflictRevision(null); void loadDefinition(); }}>Load the newer rules</button>
              <button type="button" style={S.saveBtn} onClick={() => void save(conflictRevision)}>Save mine anyway</button>
            </div>
          </div>
        )}

        </div>

        <div style={st.footer}>
          {load === 'ready' && visibleErrors && !check.valid && (
            <span style={{ ...S.errorLine, marginRight: mobile ? undefined : 'auto' }}>Fix the marked rules to save.</span>
          )}
          {saveError && <span role="alert" style={{ ...S.errorLine, marginRight: mobile ? undefined : 'auto' }}>{saveError}</span>}
          {mobile && load === 'ready' && (
            <PreviewSummary state={previewState} expanded={showTracks} onToggle={() => setShowTracks(v => !v)} />
          )}
          {!mobile && <button type="button" style={S.cancelBtn} onClick={onClose}>Cancel</button>}
          {load !== 'unsupported' && (
            <button type="button" aria-disabled={!canSave} style={{ ...st.saveBtn, ...(!canSave ? hybridControlStyles.disabled : {}) }}
              onClick={() => void save()}>
              <SmartIcon size={16} />{editing ? 'Save rules' : 'Create playlist'}
            </button>
          )}
        </div>
      </div>
    </dialog>,
    document.body,
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

// Borders that change colour (invalid values, chosen chips) use longhands: React drops a `border` shorthand
// when a toggled `borderColor` is removed again, which left marked rows with a stray border.
function edge(color: string, style: React.CSSProperties['borderStyle'] = 'solid'): React.CSSProperties {
  return { borderWidth: 1, borderStyle: style, borderColor: color };
}

const control: React.CSSProperties = {
  minHeight: 36, padding: '4px 10px', borderRadius: 8, ...edge('var(--border)'),
  background: 'var(--surface)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 14, boxSizing: 'border-box',
};

const S: Record<string, React.CSSProperties> = {
  overlay: {
    ...hybridMediaStyles.overlay, width: '100%', maxWidth: '100%', height: '100%', maxHeight: '100%', margin: 0,
    padding: 16, border: 0, boxSizing: 'border-box', color: 'var(--text)', display: 'flex',
    alignItems: 'flex-start', justifyContent: 'center',
  },
  dialog: {
    ...hybridMediaStyles.dialog, width: 'min(1180px, 100%)', maxHeight: 'calc(100dvh - 32px)', overflowY: 'auto',
    boxSizing: 'border-box', display: 'flex', flexDirection: 'column', padding: 0,
  },
  header: { padding: '20px 24px 6px', display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 },
  eyebrow: { fontSize: 11, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--accent)', marginBottom: 4 },
  title: { fontSize: 19, fontWeight: 750, letterSpacing: -0.3 },
  closeBtn: { ...hybridControlStyles.iconButton, width: 36, minWidth: 36, height: 36 },
  loading: { padding: '32px 24px', color: 'var(--text-muted)', fontSize: 14 },
  main: { minWidth: 0, margin: 0, border: 0, padding: '14px 24px 22px', display: 'flex', flexDirection: 'column', gap: 20 },
  label: { fontSize: 13, fontWeight: 650, color: 'var(--text-muted)' },
  nameInput: { ...hybridControlStyles.field, border: undefined, ...edge('var(--border)'), minHeight: 40 },
  sectionTitle: { fontSize: 13, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8 },
  modeSelect: { ...control, fontWeight: 650 },
  select: { ...control, padding: '4px 8px' },
  value: { ...control },
  invalid: { borderColor: 'var(--danger, #d65c5c)' },
  muted: { color: 'var(--text-muted)', fontSize: 14 },
  valueSlot: { flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' },
  ruleRow: {
    display: 'flex', alignItems: 'center', gap: 8, padding: 8, borderRadius: 10, flexWrap: 'wrap',
    background: 'var(--surface-subtle)', ...edge('var(--divider-subtle, var(--border))'),
  },
  ruleRowInvalid: { borderColor: 'var(--danger, #d65c5c)' },
  ruleRowWarn: { borderColor: 'var(--warning, #d49a45)' },
  group: {
    display: 'flex', flexDirection: 'column', gap: 8, padding: 12, borderRadius: 12,
    border: '1px dashed color-mix(in srgb, var(--accent) 45%, var(--border))', background: 'color-mix(in srgb, var(--surface) 70%, var(--bg))',
  },
  removeGroupBtn: { ...hybridControlStyles.secondaryButton, minHeight: 32, padding: '4px 10px', fontSize: 13, color: 'var(--text-muted)' },
  addBtn: { ...hybridControlStyles.secondaryButton, minHeight: 34, padding: '6px 12px', fontSize: 13 },
  smallIconBtn: { ...hybridControlStyles.iconButton, width: 34, minWidth: 34, height: 34, flexShrink: 0 },
  chip: {
    minHeight: 32, padding: '4px 10px', borderRadius: 999, ...edge('var(--border)'), background: 'var(--surface)',
    color: 'var(--text-muted)', fontFamily: 'inherit', fontSize: 13, cursor: 'pointer', display: 'inline-flex', alignItems: 'center', gap: 6,
  },
  chipOn: { borderColor: 'var(--accent)', background: 'var(--accent-soft)', color: 'var(--text)' },
  chipRemoved: { borderStyle: 'dashed', borderColor: 'var(--warning, #d49a45)', color: 'var(--warning, #d49a45)' },
  errorLine: { fontSize: 12, color: 'var(--danger, #d65c5c)', paddingLeft: 8 },
  warnLine: { fontSize: 12, color: 'var(--warning, #d49a45)', paddingLeft: 8 },
  hintLine: { fontSize: 12, color: 'var(--text-muted)', paddingLeft: 36 },
  orderSection: { display: 'flex', flexDirection: 'column', gap: 10, paddingTop: 16, borderTop: '1px solid var(--border)' },
  lineRow: { display: 'flex', alignItems: 'center', gap: 8, fontSize: 14, flexWrap: 'wrap' },
  preview: {
    minWidth: 0, borderLeft: '1px solid var(--border)', background: 'color-mix(in srgb, var(--surface) 82%, var(--bg))',
    padding: '14px 20px 22px', display: 'flex', flexDirection: 'column', gap: 14,
  },
  tile: { display: 'flex', flexDirection: 'column', gap: 6, padding: 12, borderRadius: 10, background: 'var(--surface-subtle)' },
  tileValue: { fontSize: 20, fontWeight: 750, fontVariantNumeric: 'tabular-nums' },
  tileLabel: { fontSize: 12, color: 'var(--text-muted)' },
  sample: { listStyle: 'none', margin: 0, padding: 0, borderRadius: 10, border: '1px solid var(--border)', background: 'var(--surface)' },
  sampleRow: { display: 'flex', alignItems: 'center', gap: 10, padding: '8px 12px', borderBottom: '1px solid var(--divider-subtle, var(--border))', fontSize: 13 },
  stateBox: {
    padding: '22px 16px', borderRadius: 12, border: '1px solid var(--border)', display: 'flex', flexDirection: 'column',
    alignItems: 'center', gap: 10, textAlign: 'center',
  },
  stateBoxError: { borderColor: 'color-mix(in srgb, var(--danger, #d65c5c) 45%, var(--border))' },
  conflict: { margin: '0 24px 8px', borderRadius: 12, border: '1px solid var(--border)', overflow: 'hidden' },
  conflictText: {
    padding: '14px 16px', display: 'flex', gap: 10, alignItems: 'flex-start', fontSize: 14, lineHeight: 1.5,
    background: 'color-mix(in srgb, var(--warning, #d49a45) 10%, transparent)', borderBottom: '1px solid color-mix(in srgb, var(--warning, #d49a45) 40%, var(--border))',
  },
  footer: { padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: 10, flexWrap: 'wrap' },
  cancelBtn: { ...hybridControlStyles.secondaryButton },
  saveBtn: { ...hybridControlStyles.primaryButton },
};

// Phones: 44px targets and 16px text (iOS zooms smaller inputs). The mobile shell has no Hybrid tokens,
// so surfaces fall back to base theme tokens like MobileBottomSheet does.
const touch: React.CSSProperties = { ...control, minHeight: 44, padding: '4px 12px', borderRadius: 10, fontSize: 16 };

const MS: Record<string, React.CSSProperties> = {
  ...S,
  overlay: {
    background: 'var(--overlay, rgba(0,0,0,0.6))', width: '100%', maxWidth: '100%', height: '100%', maxHeight: '100%',
    margin: 0, padding: 0, border: 0, boxSizing: 'border-box', color: 'var(--text)', display: 'flex',
    alignItems: 'flex-end', justifyContent: 'center',
  },
  dialog: {
    width: '100%', height: 'calc(100dvh - env(safe-area-inset-top, 0px) - 12px)', boxSizing: 'border-box',
    display: 'flex', flexDirection: 'column', overflow: 'hidden', borderRadius: '18px 18px 0 0',
    borderTop: '1px solid var(--border-strong, var(--border))', background: 'var(--surface-raised, var(--surface))',
    boxShadow: 'var(--shadow-raised, 0 -12px 40px rgba(0,0,0,0.45))',
  },
  handle: { alignSelf: 'center', width: 38, height: 4, borderRadius: 999, margin: '10px 0 4px', background: 'var(--border-strong, var(--border))' },
  header: { padding: '6px 16px 4px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 },
  closeBtn: {
    width: 44, minWidth: 44, height: 44, display: 'grid', placeItems: 'center', padding: 0, borderRadius: 12,
    border: '1px solid var(--divider-subtle, var(--border))', background: 'var(--surface-subtle, var(--bg))', color: 'var(--text-muted)',
  },
  scroll: { flex: 1, minHeight: 0, overflowY: 'auto', overscrollBehaviorY: 'contain' },
  main: { minWidth: 0, margin: 0, border: 0, padding: '12px 16px 16px', display: 'flex', flexDirection: 'column', gap: 16 },
  nameInput: { ...touch, width: '100%', padding: '8px 12px' },
  modeSelect: { ...touch, fontWeight: 650 },
  select: { ...touch, padding: '4px 10px', width: '100%' },
  value: touch,
  muted: { color: 'var(--text-muted)', fontSize: 15 },
  valueSlot: { display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', fontSize: 15 },
  ruleRow: {
    display: 'flex', flexDirection: 'column', gap: 8, padding: 12, borderRadius: 14,
    background: 'var(--surface-subtle, var(--bg))', ...edge('var(--divider-subtle, var(--border))'),
  },
  hintLine: { fontSize: 12, color: 'var(--text-muted)', paddingLeft: 8 },
  addRow: { display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 8 },
  addBtn: {
    minHeight: 48, padding: '0 12px', borderRadius: 12, border: '1px solid var(--divider-subtle, var(--border))',
    background: 'var(--surface-subtle, var(--bg))', color: 'var(--text)', fontFamily: 'inherit', fontSize: 15, fontWeight: 650,
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6,
  },
  smallIconBtn: {
    width: 44, minWidth: 44, height: 44, display: 'grid', placeItems: 'center', padding: 0, flexShrink: 0, borderRadius: 10,
    border: '1px solid var(--divider-subtle, var(--border))', background: 'var(--surface-subtle, var(--bg))', color: 'var(--text-muted)',
  },
  chip: { ...S.chip, minHeight: 44, padding: '4px 14px', fontSize: 15 },
  orderBox: { borderRadius: 14, border: '1px solid var(--divider-subtle, var(--border))', background: 'var(--surface-subtle, var(--bg))' },
  orderSummary: { minHeight: 48, padding: '0 12px', display: 'flex', alignItems: 'center', gap: 8, fontSize: 15, fontWeight: 700, cursor: 'pointer' },
  lineRow: { display: 'flex', alignItems: 'center', gap: 8, fontSize: 15, flexWrap: 'wrap' },
  preview: { minWidth: 0, padding: '0 16px 16px', display: 'flex', flexDirection: 'column', gap: 12 },
  conflict: { ...S.conflict, margin: '0 16px 12px' },
  footer: {
    padding: '12px 16px calc(env(safe-area-inset-bottom, 0px) + 14px)', borderTop: '1px solid var(--border)',
    display: 'flex', flexDirection: 'column', alignItems: 'stretch', gap: 10,
  },
  summaryBtn: {
    minHeight: 44, padding: '0 14px', borderRadius: 12, border: '1px solid var(--divider-subtle, var(--border))',
    background: 'var(--surface-subtle, var(--bg))', color: 'var(--text)', fontFamily: 'inherit', fontSize: 15,
    display: 'flex', alignItems: 'center', gap: 10,
  },
  saveBtn: { ...hybridControlStyles.primaryButton, minHeight: 48, fontSize: 16 },
};
