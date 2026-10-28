/**
 * Sleep timer choices, shared by the desktop dock popover and the mobile Now Playing sheet.
 */

import React, { useId, useState } from 'react';
import {
  SLEEP_MAX_MINUTES,
  SLEEP_MIN_MINUTES,
  SLEEP_PRESET_MINUTES,
  isValidSleepMinutes,
  sleepTimerLabel,
  useSleepClock,
  type SleepTimerControls,
} from '../audio/sleepTimer';
import { hybridControlStyles } from '../hybridPreview';

export function MoonIcon({ size = 15 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" />
    </svg>
  );
}

function ClockIcon() {
  return (
    <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" />
    </svg>
  );
}

function TrackEndIcon() {
  return (
    <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 18V5l10-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="16" cy="16" r="3" />
    </svg>
  );
}

function AlbumEndIcon() {
  return (
    <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="2.5" />
    </svg>
  );
}

function CancelIcon() {
  return (
    <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M18 6 6 18" /><path d="m6 6 12 12" />
    </svg>
  );
}

/** Choices list; `onDone` runs after a choice starts or cancels a timer. */
export function SleepTimerPanel({ controls, onDone }: { controls: SleepTimerControls; onDone?: () => void }) {
  const [customOpen, setCustomOpen] = useState(false);
  const [customMinutes, setCustomMinutes] = useState('');
  const minutesId = useId();
  const now = useSleepClock(controls.timer?.mode === 'minutes');
  const custom = Number(customMinutes);
  const customValid = customMinutes.trim() !== '' && isValidSleepMinutes(custom);

  const startAndClose = (mode: Parameters<SleepTimerControls['start']>[0], minutes?: number) => {
    controls.start(mode, minutes);
    onDone?.();
  };

  return (
    <div style={S.panel}>
      {controls.timer ? (
        <div style={S.activeRow}>
          <span style={S.activeLabel} role="status">{sleepTimerLabel(controls.timer, now)}</span>
          <button
            type="button"
            style={{ ...hybridControlStyles.dangerButton, ...S.compactButton }}
            onClick={() => { controls.cancel(); onDone?.(); }}
          >
            <CancelIcon />Cancel timer
          </button>
        </div>
      ) : null}
      <div style={S.grid}>
        {SLEEP_PRESET_MINUTES.map((minutes) => (
          <button key={minutes} type="button" style={S.choice} onClick={() => startAndClose('minutes', minutes)}>
            <ClockIcon />{minutes} min
          </button>
        ))}
        <button
          type="button"
          style={{ ...S.choice, ...(customOpen ? S.choiceActive : null) }}
          aria-expanded={customOpen}
          onClick={() => setCustomOpen((open) => !open)}
        >
          <ClockIcon />Custom…
        </button>
      </div>
      {customOpen ? (
        <form
          style={S.customRow}
          onSubmit={(event) => {
            event.preventDefault();
            if (customValid) startAndClose('minutes', custom);
          }}
        >
          <label htmlFor={minutesId} style={S.customLabel}>Minutes</label>
          <input
            id={minutesId}
            type="number"
            inputMode="numeric"
            min={SLEEP_MIN_MINUTES}
            max={SLEEP_MAX_MINUTES}
            step={1}
            value={customMinutes}
            onChange={(event) => setCustomMinutes(event.currentTarget.value)}
            style={S.customInput}
            autoFocus
          />
          <button
            type="submit"
            style={{ ...hybridControlStyles.primaryButton, ...S.compactButton, ...(customValid ? null : hybridControlStyles.disabled) }}
            disabled={!customValid}
          >
            Start
          </button>
        </form>
      ) : null}
      <div style={S.endRow}>
        <button type="button" style={S.choice} onClick={() => startAndClose('endOfTrack')}>
          <TrackEndIcon />End of track
        </button>
        <button type="button" style={S.choice} onClick={() => startAndClose('endOfAlbum')}>
          <AlbumEndIcon />End of album
        </button>
      </div>
      {controls.local ? <div style={S.note}>Reloading the page clears the timer.</div> : null}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  panel: { display: 'flex', flexDirection: 'column', gap: 10 },
  activeRow: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10 },
  activeLabel: { fontSize: 13, fontWeight: 650, color: 'var(--text)', fontVariantNumeric: 'tabular-nums' },
  compactButton: { minHeight: 32, padding: '6px 11px', fontSize: 13 },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 6 },
  endRow: { display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 6 },
  choice: {
    ...hybridControlStyles.secondaryButton,
    minHeight: 36,
    padding: '7px 10px',
    fontSize: 13,
    whiteSpace: 'nowrap',
  },
  choiceActive: { background: 'var(--accent-soft)', color: 'var(--accent)' },
  customRow: { display: 'flex', alignItems: 'center', gap: 8 },
  customLabel: { fontSize: 13, color: 'var(--text-muted)' },
  customInput: {
    width: 80,
    minHeight: 32,
    padding: '4px 8px',
    borderRadius: 8,
    border: '1px solid var(--border-strong)',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 14,
  },
  note: { fontSize: 12, color: 'var(--text-muted)' },
};
