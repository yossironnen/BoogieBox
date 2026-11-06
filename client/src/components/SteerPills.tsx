/**
 * The row of steering pills on an open smart playlist (desktop and mobile), with the messages that belong to it.
 */

import React from 'react';
import type { SteerState } from '../hooks/useSteer';
import {
  MAX_ACTIVE_PILLS, STEER_CHANGED_ICON, STEER_COPY, changedLabel, pillIcon, pillLabel, pillTip, sameTarget,
  type SmartSteer, type SteerPill,
} from '../smartSteer';

function Glyph({ d, size }: { d: string; size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ flexShrink: 0 }}>
      <path d={d} />
    </svg>
  );
}

const CLOSE_ICON = 'M18 6 6 18M6 6l12 12';

/** "N tracks changed" for the playlist's details row. Nothing while no pill is on. */
export function SteerChangedChip({ steer, style }: { steer: SteerState; style?: React.CSSProperties }) {
  if (!steer.view || steer.view.active.length === 0) return null;
  return (
    <span role="status" style={style}>
      <span style={{ display: 'inline-flex', color: 'var(--accent)', opacity: 0.85 }}>
        <Glyph d={STEER_CHANGED_ICON} size={14} />
      </span>
      {changedLabel(steer.view.addedTrackIds.length)}
    </span>
  );
}

/** Background and dot for a row that's in the playlist only because of the pills. The row must be positioned. */
export const STEER_ADDED_ROW = 'color-mix(in srgb, var(--accent) 8%, transparent)';

export function SteerAddedDot() {
  return (
    <span aria-hidden="true" data-testid="steer-added-dot" style={{
      position: 'absolute', left: 3, top: 'calc(50% - 3.5px)', width: 7, height: 7, borderRadius: '50%',
      background: 'var(--accent)', zIndex: 1,
    }} />
  );
}

export default function SteerPills({ steer, mobile = false, queueIsThisPlaylist, onUpdateQueue }: {
  steer: SteerState;
  mobile?: boolean;
  /** The player's queue came from this playlist. */
  queueIsThisPlaylist: boolean;
  /** Replaces what comes next in the queue with the playlist's tracks. */
  onUpdateQueue: () => void;
}) {
  const { view, active, busy } = steer;
  if (!view) return null;
  const source = view.popularSource;
  const full = active.length >= MAX_ACTIVE_PILLS;
  // A pill being offered may be the one just tapped; it shows once, as on.
  const offered: SteerPill[] = view.pills.filter(pill => !active.some(on => sameTarget(on, pill)));
  const showRow = active.length > 0 || offered.length > 0;
  const showQueuePrompt = steer.queueStale && queueIsThisPlaylist;
  if (!showRow && !view.fewTags && !steer.failed) return null;

  const height = mobile ? 44 : 32;
  const pillStyle = (on: boolean, dimmed: boolean): React.CSSProperties => ({
    display: 'inline-flex', alignItems: 'center', gap: 7, height, padding: mobile ? '0 14px' : '0 12px',
    borderRadius: 999, fontFamily: 'inherit', fontSize: mobile ? 14 : 13, fontWeight: 600, whiteSpace: 'nowrap',
    flexShrink: 0, cursor: dimmed ? 'not-allowed' : busy ? 'progress' : 'pointer',
    color: 'var(--text)',
    background: on ? 'color-mix(in srgb, var(--accent) 32%, transparent)' : 'var(--surface)',
    borderWidth: 1, borderStyle: 'solid',
    borderColor: on ? 'var(--accent)' : 'var(--border)',
    opacity: dimmed ? 0.45 : 1,
  });
  const smallButton: React.CSSProperties = {
    minHeight: mobile ? 44 : 32, padding: '0 12px', borderRadius: 8, fontFamily: 'inherit', fontSize: 13, fontWeight: 600,
    color: 'var(--text)', background: 'transparent', borderWidth: 1, borderStyle: 'solid', borderColor: 'var(--border)',
    cursor: 'pointer', flexShrink: 0,
  };
  const note: React.CSSProperties = { fontSize: 13, lineHeight: 1.5, color: 'var(--text-muted)' };
  const messageRow: React.CSSProperties = { display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 10, fontSize: 13 };

  const renderPill = (pill: SmartSteer, on: boolean, tip: string | undefined) => {
    const dimmed = !on && full;
    return (
      <button key={`${on ? 'on' : 'offer'}-${pill.kind}-${pill.tag ?? pill.bucket ?? pill.signal}-${pill.direction ?? ''}`}
        type="button" className="steer-pill" aria-pressed={on} disabled={dimmed || busy}
        title={dimmed ? STEER_COPY.limit : tip} style={pillStyle(on, dimmed)} onClick={() => steer.toggle(pill)}>
        <Glyph d={pillIcon(pill)} size={mobile ? 15 : 14} />
        {pillLabel(pill, source)}
        {on && <Glyph d={CLOSE_ICON} size={12} />}
      </button>
    );
  };

  return (
    <div style={{
      display: 'flex', flexDirection: 'column', gap: 10,
      padding: mobile ? '0 0 14px' : '0 16px 12px',
    }}>
      {showRow && (
        <div role="group" aria-label={STEER_COPY.group} style={{
          display: 'flex', alignItems: 'center', gap: 8,
          ...(mobile ? { overflowX: 'auto', WebkitOverflowScrolling: 'touch' } : { flexWrap: 'wrap' }),
        }}>
          {active.map(pill => renderPill(pill, true, undefined))}
          {offered.map(pill => renderPill(pill, false, pillTip(pill, source)))}
          {active.length > 0 && (
            <button type="button" className="steer-pill" disabled={busy} style={{
              ...pillStyle(false, false), background: 'transparent', borderStyle: 'dashed', color: 'var(--text-muted)',
            }} onClick={steer.reset}>
              <Glyph d={STEER_CHANGED_ICON} size={mobile ? 15 : 14} />
              {STEER_COPY.reset}
            </button>
          )}
        </div>
      )}
      {view.fewTags && <div style={note}>{STEER_COPY.fewTags}</div>}
      {steer.failed && (
        <div role="alert" style={messageRow}>
          <span style={{ flex: 1, minWidth: 180 }}>{STEER_COPY.failed}</span>
          <button type="button" style={smallButton} onClick={steer.retry}>{STEER_COPY.tryAgain}</button>
        </div>
      )}
      {showQueuePrompt && (
        <div role="status" style={{
          ...messageRow, padding: '10px 12px', borderRadius: 10, background: 'var(--surface)',
          borderWidth: 1, borderStyle: 'solid', borderColor: 'var(--border)',
        }}>
          <span style={{ flex: 1, minWidth: 180 }}>{STEER_COPY.queueStale}</span>
          <button type="button" style={{
            ...smallButton, background: 'var(--accent)', borderColor: 'var(--accent)', color: '#fff', fontWeight: 700,
          }} onClick={() => { onUpdateQueue(); steer.answerQueue(); }}>
            {STEER_COPY.updateNext}
          </button>
          <button type="button" style={smallButton} onClick={steer.answerQueue}>{STEER_COPY.notNow}</button>
        </div>
      )}
    </div>
  );
}
