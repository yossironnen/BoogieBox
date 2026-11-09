/**
 * The "Use my saved EQ" block of the Equalizer: the switch, what the song plays now, and saving or removing a
 * curve for the song or its album. Shared by the desktop popup and the mobile sheet.
 */

import React, { useEffect, useState } from 'react';
import { hybridControlStyles } from '../hybridPreview';
import type { PlayerEqControls } from './Player';

const SAVE_FAILED = "Couldn't save this EQ. Try again.";
const REMOVE_FAILED = "Couldn't remove this EQ. Try again.";

function SongIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

function AlbumIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
    </svg>
  );
}

const S = {
  card: {
    display: 'flex', flexDirection: 'column', gap: 9, padding: '10px 12px',
    // Mixed from --accent, so the block follows the adaptive accent like the buttons in it.
    border: '1px solid color-mix(in srgb, var(--accent) 32%, transparent)',
    borderRadius: 10, background: 'color-mix(in srgb, var(--accent) 9%, transparent)', minWidth: 0,
  },
  top: { display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '8px 14px' },
  switchLabel: { display: 'flex', alignItems: 'center', gap: 7, fontSize: 13, fontWeight: 600, color: 'var(--text)', cursor: 'pointer' },
  status: { display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12 },
  actions: { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 6 },
  primary: { ...hybridControlStyles.primaryButton, minHeight: 32, padding: '0 10px', fontSize: 13, fontWeight: 700, gap: 6 },
  secondary: {
    ...hybridControlStyles.secondaryButton, minHeight: 32, padding: '0 10px', fontSize: 13, fontWeight: 600, gap: 6,
    border: '1px solid var(--border)',
  },
  link: {
    minHeight: 32, padding: '0 6px', border: 0, background: 'transparent', color: 'var(--text-muted)',
    fontFamily: 'inherit', fontSize: 12, textDecoration: 'underline', cursor: 'pointer',
  },
  hint: { fontSize: 12, color: 'var(--text-muted)' },
  error: { fontSize: 12, color: 'var(--danger, #d14343)' },
} satisfies Record<string, React.CSSProperties>;

/** Mobile: 44 px targets, the switch on the right, full-width buttons. */
const M = {
  card: { gap: 8, padding: 12, borderRadius: 14 },
  top: { flexDirection: 'column', alignItems: 'stretch', gap: 8 },
  switchLabel: { justifyContent: 'space-between', minHeight: 44, fontSize: 14, fontWeight: 700 },
  status: { fontSize: 13 },
  actions: { flexDirection: 'column', alignItems: 'stretch', gap: 8 },
  button: { minHeight: 44, fontSize: 14, width: '100%' },
  link: { minHeight: 44, fontSize: 14 },
} satisfies Record<string, React.CSSProperties>;

type SavedEqPanelControls = Pick<PlayerEqControls,
  'savedEqEnabled' | 'savedEqScope' | 'savedEq' | 'onSavedEqEnabledChange' | 'onSaveSavedEq' | 'onRemoveSavedEq' | 'watchSavedEq'>;

/** Saved EQ Panel is part of this module's public API. */
export default function SavedEqPanel({ controls, mobile = false }: { controls: SavedEqPanelControls; mobile?: boolean }) {
  const { savedEqEnabled, savedEqScope, savedEq, watchSavedEq } = controls;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The panel needs to know whether the song and its album have curves, even with the switch off.
  useEffect(() => watchSavedEq(), [watchSavedEq]);

  const run = async (action: () => Promise<boolean>, failure: string) => {
    setBusy(true);
    setError(null);
    try {
      if (!(await action())) setError(failure);
    } catch {
      setError(failure);
    } finally {
      setBusy(false);
    }
  };
  const save = (scope: 'track' | 'album') => run(() => controls.onSaveSavedEq(scope), SAVE_FAILED);
  const remove = (scope: 'track' | 'album') => run(() => controls.onRemoveSavedEq(scope), REMOVE_FAILED);

  let status: { text: string; icon: React.ReactNode; color: string } | null = null;
  if (savedEq.edited) {
    status = { text: 'Not saved yet', icon: <PencilIcon />, color: 'var(--text-muted)' };
  } else if (savedEqScope === 'track') {
    status = { text: 'Using the EQ saved for this song', icon: <CheckIcon />, color: 'var(--accent)' };
  } else if (savedEqScope === 'album') {
    status = { text: 'Using the EQ saved for this album', icon: <CheckIcon />, color: 'var(--accent)' };
  }

  const button = mobile ? M.button : null;
  const disabledStyle = (disabled: boolean) => (disabled ? hybridControlStyles.disabled : null);
  const songDisabled = busy || !savedEq.canSave;
  const albumDisabled = busy || !savedEq.canSaveAlbum;

  return (
    <div data-ui-region="saved-eq" style={{ ...S.card, ...(mobile ? M.card : null) }}>
      <div style={{ ...S.top, ...(mobile ? M.top : null) }}>
        <label style={{ ...S.switchLabel, ...(mobile ? M.switchLabel : null) }}>
          {mobile ? <span>Use my saved EQ</span> : null}
          <input
            type="checkbox"
            checked={savedEqEnabled}
            onChange={(event) => controls.onSavedEqEnabledChange(event.currentTarget.checked)}
          />
          {mobile ? null : <span>Use my saved EQ</span>}
        </label>
        {status && !savedEq.browserOnly ? (
          <span role="status" style={{ ...S.status, ...(mobile ? M.status : null), color: status.color }}>
            {status.icon}
            {status.text}
          </span>
        ) : null}
      </div>

      {savedEq.browserOnly ? (
        <div style={S.hint}>EQ applies to this browser only</div>
      ) : (
        <div style={{ ...S.actions, ...(mobile ? M.actions : null) }}>
          <button
            type="button"
            style={{ ...S.primary, ...button, ...disabledStyle(songDisabled) }}
            disabled={songDisabled}
            onClick={() => save('track')}
          >
            <SongIcon />
            Save for this song
          </button>
          <button
            type="button"
            style={{ ...S.secondary, ...button, ...disabledStyle(albumDisabled) }}
            disabled={albumDisabled}
            onClick={() => save('album')}
          >
            <AlbumIcon />
            Save for this album
          </button>
          {savedEq.hasTrack ? (
            <button type="button" style={{ ...S.link, ...(mobile ? M.link : null) }} disabled={busy} onClick={() => remove('track')}>
              Remove from this song
            </button>
          ) : null}
          {savedEq.hasAlbum ? (
            <button type="button" style={{ ...S.link, ...(mobile ? M.link : null) }} disabled={busy} onClick={() => remove('album')}>
              Remove from this album
            </button>
          ) : null}
        </div>
      )}
      {error ? <div role="alert" style={S.error}>{error}</div> : null}
    </div>
  );
}
