/**
 * Settings → Advanced → Artist Radio: how track-level tags are collected for Artist Radio, plus live progress.
 * The switch for the free music sources lives in Settings → Integrations (FreeMusicSourcesSetting).
 * See wip/artist-radio-v2-plan.md §3.4 and §3.6.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles } from '../hybridPreview';
import type { RadioMetadataStatus } from '../types';
import { TagIcon, UsersIcon } from './ArtistRadioControls';

const POLL_MS = 15000;

type Mode = RadioMetadataStatus['mode'];

const MODE_OPTIONS: ReadonlyArray<{ value: Mode; label: string }> = [
  { value: 'full', label: 'Whole library, in the background' },
  { value: 'lazy', label: 'Only when a radio starts' },
  { value: 'off', label: 'Off' },
];

const iconWrap: React.CSSProperties = { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' };

/** Pure helper so the percentage rule is testable: `null` when there is nothing to measure. */
export function tagProgressPercent(checked: number, total: number): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.round((checked / total) * 100));
}

/** Card content for the Artist Radio metadata settings (the section title lives in SettingsPage). */
export default function RadioMetadataSettings({ disabled = false }: { disabled?: boolean }) {
  const [status, setStatus] = useState<RadioMetadataStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(() => {
    api.radioMetadataStatus()
      .then((next) => { setStatus(next); setError(null); })
      .catch(() => setError('Status unavailable.'));
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const save = async (updates: Record<string, string>) => {
    setSaving(true);
    try {
      await api.settings.update(updates);
      setError(null);
      refresh();
    } catch (e) {
      setError(e instanceof Error && e.message ? e.message : 'Could not save.');
    } finally {
      setSaving(false);
    }
  };

  const trackPct = status ? tagProgressPercent(status.tracksChecked, status.tracksTotal) : null;
  const artistPct = status ? tagProgressPercent(status.artistsTagged, status.artistsTotal) : null;
  const busy = disabled || saving || !status;

  return (
    <div
      style={{
        padding: '16px 20px', borderRadius: 8, marginBottom: 12,
        backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
        display: 'grid', gap: 14,
      }}
    >
      <div style={{ color: 'var(--text-muted)', fontSize: 14, lineHeight: 1.6 }}>
        Artist Radio builds queues from the artist, similar artists and mood. Mood and style tags are collected
        from online providers and improve radio quality as they arrive.
      </div>

      <label style={{ display: 'grid', gap: 6 }}>
        <span style={{ color: 'var(--text)', fontWeight: 600 }}>Track tag collection</span>
        <select
          aria-label="Track tag collection"
          value={status?.mode ?? 'full'}
          disabled={busy}
          onChange={(e) => save({ radioTrackTagSync: e.target.value })}
          style={{ ...hybridControlStyles.select, ...(busy ? hybridControlStyles.disabled : {}), maxWidth: 360 }}
        >
          {MODE_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
        </select>
        <span style={{ color: 'var(--text-muted)', fontSize: 13 }}>
          Tags come from MusicBrainz. A first full pass over a large library can take a day or more; it runs at low
          priority, resumes after restarts and pauses while a BoogieMix is being built.
        </span>
      </label>

      {status && !status.keylessEnabled && status.mode !== 'off' ? (
        <div role="note" style={{ color: 'var(--text-muted)', fontSize: 13 }}>
          Turn on Free music sources under Settings › Integrations to collect tags.
        </div>
      ) : null}

      <div style={{ display: 'grid', gap: 8 }}>
        {status ? (
          <>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 14 }}>
              <span style={iconWrap}><TagIcon size={16} /></span>
              <span>
                Tracks checked: <strong>{status.tracksChecked.toLocaleString()}</strong> of {status.tracksTotal.toLocaleString()}
                {trackPct !== null ? ` (${trackPct}%)` : ''}
              </span>
            </div>
            {trackPct !== null ? (
              <div
                role="progressbar"
                aria-label="Track tag collection progress"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={trackPct}
                style={{ height: 6, borderRadius: 6, background: 'var(--border)', overflow: 'hidden', maxWidth: 360 }}
              >
                <div style={{ width: `${trackPct}%`, height: '100%', background: 'var(--accent)' }} />
              </div>
            ) : null}
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 14 }}>
              <span style={iconWrap}><UsersIcon size={16} /></span>
              <span>
                Artists with tags: <strong>{status.artistsTagged.toLocaleString()}</strong> of {status.artistsTotal.toLocaleString()}
                {artistPct !== null ? ` (${artistPct}%)` : ''}
              </span>
            </div>
          </>
        ) : (
          <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>{error ?? 'Loading status…'}</div>
        )}
        {status && error ? <div role="alert" style={{ color: 'var(--danger, #e0554d)', fontSize: 13 }}>{error}</div> : null}
      </div>
    </div>
  );
}
