/**
 * Artist Radio v2 controls: the split "play radio" button with its options
 * popover (desktop) / bottom sheet (mobile), and the queue "why is this
 * playing" chip. See wip/artist-radio-v2-plan.md §5.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type {
  ArtistRadioOptions,
  ArtistRadioOptionsSnapshot,
  ClientEntityId,
  RadioFocus,
  RadioMoodBucket,
  RadioReason,
} from '../types';

/** Slider position that yields the default 30/45/25 mix (mirrors the server). */
export const DEFAULT_RADIO_VARIETY = 0.45;

/** Options used by the one-click main button until the user changes them. */
export const DEFAULT_RADIO_OPTIONS: ArtistRadioOptions = {
  focus: 'similar',
  moods: [],
  variety: DEFAULT_RADIO_VARIETY,
};

/** The closed mood set, in display order. */
export const RADIO_MOODS: ReadonlyArray<{ bucket: RadioMoodBucket; label: string }> = [
  { bucket: 'chill', label: 'Chill' },
  { bucket: 'melancholic', label: 'Melancholic' },
  { bucket: 'uplifting', label: 'Uplifting' },
  { bucket: 'energetic', label: 'Energetic' },
  { bucket: 'dark', label: 'Dark' },
  { bucket: 'dreamy', label: 'Dreamy' },
  { bucket: 'romantic', label: 'Romantic' },
  { bucket: 'aggressive', label: 'Aggressive' },
];

const PREFS_KEY = 'bb.artistRadio.prefs';

/** Focus and variety persist as a per-browser convenience; moods reset to auto each time. */
export function loadRadioPrefs(): ArtistRadioOptions {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (!raw) return { ...DEFAULT_RADIO_OPTIONS };
    const parsed = JSON.parse(raw) as Partial<ArtistRadioOptions>;
    const focus: RadioFocus = parsed.focus === 'mood' ? 'mood' : 'similar';
    const variety = typeof parsed.variety === 'number' && parsed.variety >= 0 && parsed.variety <= 1
      ? parsed.variety
      : DEFAULT_RADIO_VARIETY;
    return { focus, moods: [], variety };
  } catch {
    return { ...DEFAULT_RADIO_OPTIONS };
  }
}

/** Saves focus and variety (never throws; storage may be blocked). */
export function saveRadioPrefs(options: ArtistRadioOptions): void {
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify({ focus: options.focus, variety: options.variety }));
  } catch {
    /* storage unavailable — the choice just isn't remembered */
  }
}

/** `[seed, similar, mood]` shares for the mix preview; the same formula the server uses. */
export function radioTargetShares(focus: RadioFocus, variety: number): [number, number, number] {
  const base: [number, number, number] = focus === 'mood' ? [0.1, 0.25, 0.65] : [0.3, 0.45, 0.25];
  const clamped = Math.min(1, Math.max(0, variety));
  const shift = (clamped - DEFAULT_RADIO_VARIETY) * 0.3;
  const seed = Math.min(0.45, Math.max(0.05, base[0] - shift));
  const moved = base[0] - seed;
  const shares: [number, number, number] = [seed, base[1] + moved / 2, base[2] + moved / 2];
  const total = shares[0] + shares[1] + shares[2];
  return [shares[0] / total, shares[1] / total, shares[2] / total];
}

// ── Icons (inline SVG, stroke-based, currentColor — icon-first convention) ──

type IconProps = { size?: number };

const iconProps = (size: number) => ({
  width: size,
  height: size,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
  focusable: false,
});

/** Concentric-arcs radio glyph. */
export const RadioIcon = ({ size = 16 }: IconProps) => (
  <svg {...iconProps(size)}>
    <circle cx="12" cy="12" r="2" />
    <path d="M16.2 7.8a6 6 0 0 1 0 8.4" />
    <path d="M7.8 16.2a6 6 0 0 1 0-8.4" />
    <path d="M19.1 4.9a10 10 0 0 1 0 14.2" />
    <path d="M4.9 19.1a10 10 0 0 1 0-14.2" />
  </svg>
);

const ChevronIcon = ({ size = 14 }: IconProps) => (
  <svg {...iconProps(size)}><polyline points="6 9 12 15 18 9" /></svg>
);
const UserIcon = ({ size = 13 }: IconProps) => (
  <svg {...iconProps(size)}><circle cx="12" cy="8" r="4" /><path d="M4 21a8 8 0 0 1 16 0" /></svg>
);
export const UsersIcon = ({ size = 13 }: IconProps) => (
  <svg {...iconProps(size)}>
    <circle cx="9" cy="8" r="3.5" /><path d="M2 20a7 7 0 0 1 14 0" />
    <circle cx="17" cy="9" r="2.5" /><path d="M17 14a5 5 0 0 1 5 5" />
  </svg>
);
export const TagIcon = ({ size = 13 }: IconProps) => (
  <svg {...iconProps(size)}>
    <path d="M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L2 12V2h10l8.6 8.6a2 2 0 0 1 0 2.8Z" />
    <circle cx="7" cy="7" r="1.2" />
  </svg>
);
const PlayGlyph = ({ size = 15 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" focusable="false">
    <polygon points="6 4 20 12 6 20 6 4" />
  </svg>
);

/** One glyph per mood bucket — a closed set of eight, so a distinct icon each is fine. */
export function MoodIcon({ bucket, size = 13 }: { bucket: RadioMoodBucket; size?: number }) {
  const p = iconProps(size);
  switch (bucket) {
    case 'chill': return <svg {...p}><path d="M2 12h3l3-8 4 16 3-12 2 4h5" /></svg>;
    case 'melancholic': return <svg {...p}><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z" /></svg>;
    case 'uplifting': return (
      <svg {...p}>
        <circle cx="12" cy="12" r="4" />
        <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
      </svg>
    );
    case 'energetic': return <svg {...p}><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" /></svg>;
    case 'dark': return <svg {...p}><path d="M12 22a7 7 0 0 0 7-7c0-4-3-6-4-10-3 2-3 5-3 5s-2-1-2-4c-3 2-5 6-5 9a7 7 0 0 0 7 7Z" /></svg>;
    case 'dreamy': return <svg {...p}><path d="M18 10a6 6 0 0 0-11.7-1.5A4.5 4.5 0 0 0 7 17.5h11a3.75 3.75 0 0 0 0-7.5Z" /></svg>;
    case 'romantic': return <svg {...p}><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8l1 1.1L12 21l7.8-7.5 1-1.1a5.5 5.5 0 0 0 0-7.8Z" /></svg>;
    case 'aggressive': return <svg {...p}><path d="M13 2 4 14h7l-1 8 9-12h-7Z" /></svg>;
    default: return <TagIcon size={size} />;
  }
}

// ── Queue reason chip ───────────────────────────────────────────────────────

const REASON_COLORS: Record<RadioReason['kind'], string> = {
  seed: 'var(--accent)',
  similar: '#b79ac7',
  mood: '#86bdb7',
  style: 'var(--text-muted)',
};

const isMoodBucket = (value: string): value is RadioMoodBucket =>
  RADIO_MOODS.some((m) => m.bucket === value);

/** Human text for a reason, e.g. "Similar to Massive Attack" or "Melancholic". */
export function radioReasonText(reason: RadioReason): string {
  switch (reason.kind) {
    case 'seed': return 'Seed artist';
    case 'similar': return `Similar to ${reason.label}`;
    case 'mood': return reason.label.charAt(0).toUpperCase() + reason.label.slice(1);
    default: return reason.label.charAt(0).toUpperCase() + reason.label.slice(1);
  }
}

/** Small icon+label chip explaining why a queued track is in the radio. */
export function RadioReasonChip({ reason }: { reason: RadioReason }) {
  const shortText = reason.kind === 'seed' ? 'Seed'
    : reason.kind === 'similar' ? 'Similar'
    : radioReasonText(reason);
  return (
    <span
      data-testid="radio-reason-chip"
      title={radioReasonText(reason)}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        flex: 'none',
        maxWidth: 110,
        padding: '2px 7px',
        borderRadius: 999,
        fontSize: 11,
        color: REASON_COLORS[reason.kind],
        border: '1px solid var(--border)',
        background: 'color-mix(in srgb, var(--text) 6%, transparent)',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    >
      {reason.kind === 'seed' ? <UserIcon />
        : reason.kind === 'similar' ? <UsersIcon />
        : reason.kind === 'mood' && isMoodBucket(reason.label) ? <MoodIcon bucket={reason.label} />
        : <TagIcon />}
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{shortText}</span>
    </span>
  );
}

// ── Split button + options ──────────────────────────────────────────────────

const subtleBg = 'color-mix(in srgb, var(--text) 6%, var(--surface))';

const S = {
  wrap: { position: 'relative', display: 'inline-flex' } as React.CSSProperties,
  split: { display: 'inline-flex', borderRadius: 9, overflow: 'hidden', border: '1px solid var(--accent)' } as React.CSSProperties,
  main: {
    width: 38, height: 36, display: 'grid', placeItems: 'center', border: 0, cursor: 'pointer',
    background: 'var(--accent)', color: '#fff',
  } as React.CSSProperties,
  chev: {
    width: 26, height: 36, display: 'grid', placeItems: 'center', border: 0, cursor: 'pointer',
    borderLeft: '1px solid rgba(0,0,0,.35)', color: 'var(--accent)',
    background: 'color-mix(in srgb, var(--accent) 18%, transparent)',
  } as React.CSSProperties,
  panel: {
    background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 12, padding: 16,
    boxShadow: '0 18px 44px rgba(0,0,0,.45)', color: 'var(--text)', fontSize: 14, textAlign: 'left',
  } as React.CSSProperties,
  label: {
    fontSize: 11, letterSpacing: '0.06em', textTransform: 'uppercase', color: 'var(--text-muted)', margin: '14px 0 6px',
  } as React.CSSProperties,
  seg: { display: 'flex', gap: 4, padding: 3, borderRadius: 9, border: '1px solid var(--border)', background: subtleBg } as React.CSSProperties,
  chips: { display: 'flex', flexWrap: 'wrap', gap: 6 } as React.CSSProperties,
};

const segButton = (on: boolean): React.CSSProperties => ({
  flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6, padding: '7px 8px',
  border: 0, borderRadius: 7, cursor: 'pointer', font: 'inherit', fontSize: 13,
  background: on ? 'var(--accent)' : 'transparent', color: on ? '#fff' : 'var(--text-muted)', fontWeight: on ? 600 : 400,
});

const chipStyle = (on: boolean, dim: boolean): React.CSSProperties => ({
  display: 'inline-flex', alignItems: 'center', gap: 6, padding: '5px 10px', borderRadius: 999, cursor: 'pointer',
  font: 'inherit', fontSize: 13,
  border: `1px solid ${on ? 'var(--accent)' : 'var(--border)'}`,
  background: on ? 'color-mix(in srgb, var(--accent) 16%, transparent)' : subtleBg,
  color: on ? 'var(--text)' : 'var(--text-muted)',
  opacity: dim ? 0.6 : 1,
});

const MIX_COLORS = ['var(--accent)', '#8a6a9a', '#5f8f8a'] as const;

/** Split "play radio" button. The main half starts immediately with the defaults; the chevron opens options. */
export function ArtistRadioSplitButton({
  artistId,
  artistName,
  loading,
  onStart,
  presentation = 'popover',
}: {
  artistId: ClientEntityId;
  artistName: string;
  loading: boolean;
  onStart: (options: ArtistRadioOptions) => void;
  presentation?: 'popover' | 'sheet';
}) {
  const [open, setOpen] = useState(false);
  const [focus, setFocus] = useState<RadioFocus>('similar');
  const [variety, setVariety] = useState(DEFAULT_RADIO_VARIETY);
  const [selected, setSelected] = useState<RadioMoodBucket[]>([]);
  const [snapshot, setSnapshot] = useState<ArtistRadioOptionsSnapshot | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const panelId = `artist-radio-options-${String(artistId)}`;

  const openPanel = useCallback(() => {
    const prefs = loadRadioPrefs();
    setFocus(prefs.focus);
    setVariety(prefs.variety);
    setSelected([]);
    setOpen(true);
  }, []);

  useEffect(() => {
    if (!open) return undefined;
    let cancelled = false;
    setSnapshot(null);
    api.artistRadioOptions(artistId)
      .then((result) => { if (!cancelled) setSnapshot(result); })
      .catch(() => { if (!cancelled) setSnapshot(null); });
    return () => { cancelled = true; };
  }, [open, artistId]);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false); };
    const onPointer = (event: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onPointer);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onPointer);
    };
  }, [open]);

  const autoMoods = snapshot?.autoMoods ?? [];
  const effective = selected.length ? selected : autoMoods;
  const availability = new Map((snapshot?.moods ?? []).map((m) => [m.bucket, m.available] as const));
  const shares = radioTargetShares(focus, variety);
  const pct = shares.map((s) => Math.round(s * 100));
  const progress = snapshot?.libraryTagProgress;
  const taggedPct = progress && progress.candidates > 0
    ? Math.round((progress.tagged / progress.candidates) * 100)
    : null;

  const toggleMood = (bucket: RadioMoodBucket) => {
    setSelected((current) => {
      const base = current.length ? current : autoMoods;
      return base.includes(bucket) ? base.filter((b) => b !== bucket) : [...base, bucket];
    });
  };

  const start = () => {
    const options: ArtistRadioOptions = { focus, moods: selected, variety };
    saveRadioPrefs(options);
    setOpen(false);
    onStart(options);
  };

  const panel = (
    <div
      id={panelId}
      role="dialog"
      aria-label={`Artist Radio options for ${artistName}`}
      style={{
        ...S.panel,
        ...(presentation === 'sheet'
          ? { position: 'fixed', left: 0, right: 0, bottom: 0, zIndex: 400, borderRadius: '20px 20px 0 0', maxHeight: '80vh', overflowY: 'auto' }
          : { position: 'absolute', top: 'calc(100% + 10px)', left: 0, width: 400, maxWidth: '92vw', zIndex: 60 }),
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontWeight: 700 }}>
        <span style={{ color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' }}><RadioIcon /></span>
        Artist Radio
      </div>

      <div style={S.label} id={`${panelId}-focus`}>Focus</div>
      <div role="group" aria-labelledby={`${panelId}-focus`} style={S.seg}>
        <button type="button" aria-pressed={focus === 'similar'} style={segButton(focus === 'similar')} onClick={() => setFocus('similar')}>
          <UsersIcon size={14} /> Artist + similar
        </button>
        <button type="button" aria-pressed={focus === 'mood'} style={segButton(focus === 'mood')} onClick={() => setFocus('mood')}>
          <MoodIcon bucket="melancholic" size={14} /> Mood
        </button>
      </div>

      <div style={S.label} id={`${panelId}-mood`}>
        Mood{selected.length === 0 && autoMoods.length > 0 ? ' — auto-picked from this artist' : ''}
      </div>
      <div role="group" aria-labelledby={`${panelId}-mood`} style={S.chips}>
        {RADIO_MOODS.map(({ bucket, label }) => {
          const on = effective.includes(bucket);
          const dim = snapshot !== null && availability.get(bucket) === false;
          return (
            <button
              key={bucket}
              type="button"
              aria-pressed={on}
              title={dim ? 'Few tagged tracks yet — results may be limited' : undefined}
              style={chipStyle(on, dim)}
              onClick={() => toggleMood(bucket)}
            >
              <MoodIcon bucket={bucket} />
              {label}
              {on && selected.length === 0 ? <span style={{ fontSize: 10, color: 'var(--accent)' }}>auto</span> : null}
            </button>
          );
        })}
      </div>

      <div style={S.label}>
        <label htmlFor={`${panelId}-variety`}>Variety</label>
      </div>
      <input
        id={`${panelId}-variety`}
        type="range"
        min={0}
        max={1}
        step={0.05}
        value={variety}
        aria-valuetext={variety < 0.35 ? 'Familiar' : variety > 0.65 ? 'Adventurous' : 'Balanced'}
        onChange={(e) => setVariety(Number(e.target.value))}
        style={{ width: '100%', accentColor: 'var(--accent)' }}
      />
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, color: 'var(--text-muted)' }}>
        <span>Familiar</span><span>Adventurous</span>
      </div>

      <div style={S.label}>Mix</div>
      <div
        role="img"
        aria-label={`${artistName} ${pct[0]}%, similar artists ${pct[1]}%, mood and style ${pct[2]}%`}
        style={{ display: 'flex', height: 10, borderRadius: 6, overflow: 'hidden', border: '1px solid var(--border)' }}
      >
        {shares.map((share, i) => (
          <span key={MIX_COLORS[i]} style={{ width: `${share * 100}%`, background: MIX_COLORS[i] }} />
        ))}
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px 14px', fontSize: 12, color: 'var(--text-muted)', marginTop: 6 }}>
        <span><i style={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', marginRight: 5, background: MIX_COLORS[0] }} />{artistName} {pct[0]}%</span>
        <span><i style={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', marginRight: 5, background: MIX_COLORS[1] }} />Similar {pct[1]}%</span>
        <span><i style={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', marginRight: 5, background: MIX_COLORS[2] }} />Mood {pct[2]}%</span>
      </div>

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 16, paddingTop: 12, borderTop: '1px solid var(--border)' }}>
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-muted)' }}>
          {taggedPct !== null ? (
            <>
              <span style={{ color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' }}><TagIcon size={14} /></span>
              {taggedPct}% of your library tagged
            </>
          ) : null}
        </span>
        <button
          type="button"
          className="icon-action-btn icon-action-btn--primary"
          aria-label="Start radio"
          title="Start radio"
          disabled={loading}
          onClick={start}
        >
          <PlayGlyph />
        </button>
      </div>
    </div>
  );

  return (
    <div ref={wrapRef} style={S.wrap}>
      <div style={S.split}>
        <button
          type="button"
          style={{ ...S.main, ...(loading ? { opacity: 0.7, cursor: 'default' } : {}) }}
          data-tip={loading ? 'Building Radio…' : 'Play Artist Radio'}
          aria-label="Play Artist Radio — build a radio queue from this artist, similar artists and mood"
          disabled={loading}
          onClick={() => onStart({ ...loadRadioPrefs(), moods: [] })}
        >
          {loading ? <span className="icon-action-spinner" aria-hidden="true" /> : <RadioIcon />}
        </button>
        <button
          type="button"
          style={{ ...S.chev, ...(open ? { background: 'var(--accent)', color: '#fff' } : {}) }}
          aria-label="Radio options"
          title="Radio options"
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          onClick={() => (open ? setOpen(false) : openPanel())}
        >
          <ChevronIcon />
        </button>
      </div>
      {open ? (
        presentation === 'sheet' ? (
          <>
            <div
              data-testid="radio-sheet-scrim"
              onClick={() => setOpen(false)}
              style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,.45)', zIndex: 399 }}
            />
            {panel}
          </>
        ) : panel
      ) : null}
    </div>
  );
}
