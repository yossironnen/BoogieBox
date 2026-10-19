/**
 * Browse → Radio: internet radio stations everyone can play; admins add, edit and delete them. The same
 * stations appear in Subsonic apps.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import ConfirmModal from './ConfirmModal';
import { RadioIcon } from './FavouriteIcons';
import { radioStationProblem, type RadioStation, type RadioStationInput } from '../radioStations';

interface Props {
  isAdmin: boolean;
  onPlayStation: (station: RadioStation) => void;
  /** Station currently playing, if any (shows the playing state). */
  playingStationId?: string | null;
}

const PlayGlyph = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><polygon points="6,4 20,12 6,20" /></svg>
);
const PlusGlyph = () => (
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
    <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);
const EditGlyph = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
  </svg>
);
const TrashGlyph = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <polyline points="3 6 5 6 21 6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
  </svg>
);
const LinkGlyph = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><polyline points="15 3 21 3 21 9" /><line x1="10" y1="14" x2="21" y2="3" />
  </svg>
);

const BLANK: RadioStationInput = { name: '', streamUrl: '', homepageUrl: '' };

type Editing = { id: string | null; input: RadioStationInput };

export default function RadioStationsView({ isAdmin, onPlayStation, playingStationId = null }: Props) {
  const [stations, setStations] = useState<RadioStation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<RadioStation | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    api.radioStations.list()
      .then(setStations)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : 'Could not load radio stations'))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => { load(); }, [load]);

  const save = async () => {
    if (!editing) return;
    const problem = radioStationProblem(editing.input);
    if (problem) { setFormError(problem); return; }
    setSaving(true);
    setFormError(null);
    try {
      const input = {
        name: editing.input.name.trim(),
        streamUrl: editing.input.streamUrl.trim(),
        homepageUrl: editing.input.homepageUrl.trim(),
      };
      if (editing.id) await api.radioStations.update(editing.id, input);
      else await api.radioStations.create(input);
      setEditing(null);
      load();
    } catch (err: unknown) {
      setFormError(err instanceof Error ? err.message : 'Could not save the station');
    } finally {
      setSaving(false);
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    setSaving(true);
    try {
      await api.radioStations.remove(deleting.id);
      setDeleting(null);
      load();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Could not delete the station');
      setDeleting(null);
    } finally {
      setSaving(false);
    }
  };

  const field = (key: keyof RadioStationInput, label: string, placeholder: string) => (
    <label style={S.field}>
      <span style={S.fieldLabel}>{label}</span>
      <input
        style={S.input}
        value={editing?.input[key] ?? ''}
        placeholder={placeholder}
        onChange={(e) => {
          const value = e.target.value;
          setEditing((prev) => (prev ? { ...prev, input: { ...prev.input, [key]: value } } : prev));
        }}
      />
    </label>
  );

  return (
    <div style={S.root}>
      <div style={S.toolbar}>
        <span style={S.heading}><span style={{ color: 'var(--accent)', opacity: 0.85 }}><RadioIcon size={16} /></span>{stations.length.toLocaleString()} station{stations.length === 1 ? '' : 's'}</span>
        {isAdmin && !editing && (
          <button type="button" style={S.addBtn} onClick={() => { setEditing({ id: null, input: { ...BLANK } }); setFormError(null); }}>
            <PlusGlyph /> Add station
          </button>
        )}
      </div>

      {editing && (
        <div style={S.form} role="form" aria-label={editing.id ? 'Edit station' : 'Add station'}>
          {field('name', 'Name', 'Jazz FM')}
          {field('streamUrl', 'Stream URL', 'https://…')}
          {field('homepageUrl', 'Homepage (optional)', 'https://…')}
          {formError && <div role="alert" style={S.formError}>{formError}</div>}
          <div style={S.formActions}>
            <button type="button" style={S.secondaryBtn} onClick={() => setEditing(null)} disabled={saving}>Cancel</button>
            <button type="button" style={S.primaryBtn} onClick={() => { void save(); }} disabled={saving}>
              {saving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </div>
      )}

      {error && <div role="alert" style={S.notice}>{error}</div>}
      {loading && !error && <div style={S.notice}>Loading…</div>}
      {!loading && !error && stations.length === 0 && (
        <div style={S.empty}>
          <span style={{ color: 'var(--accent)', opacity: 0.85 }}><RadioIcon size={32} /></span>
          <div>{isAdmin ? 'No radio stations yet. Add one to get started.' : 'No radio stations yet.'}</div>
        </div>
      )}

      <div style={S.grid}>
        {stations.map((station) => {
          const playing = playingStationId === station.id;
          return (
            <div key={station.id} style={{ ...S.card, ...(playing ? S.cardPlaying : {}) }}>
              <button
                type="button"
                style={S.cardPlay}
                aria-label={`Play ${station.name}`}
                title={`Play ${station.name}`}
                onClick={() => onPlayStation(station)}
              >
                <PlayGlyph size={16} />
              </button>
              <div style={S.cardText}>
                <div style={S.cardTitle}>{station.name}</div>
                {station.homepageUrl && (
                  <a href={station.homepageUrl} target="_blank" rel="noopener noreferrer" style={S.homepage}>
                    <LinkGlyph /> Website
                  </a>
                )}
              </div>
              {isAdmin && (
                <div style={S.cardActions}>
                  <button type="button" style={S.iconBtn} aria-label={`Edit ${station.name}`} title="Edit"
                    onClick={() => {
                      setEditing({ id: station.id, input: { name: station.name, streamUrl: station.streamUrl, homepageUrl: station.homepageUrl ?? '' } });
                      setFormError(null);
                    }}
                  >
                    <EditGlyph />
                  </button>
                  <button type="button" style={S.iconBtn} aria-label={`Delete ${station.name}`} title="Delete" onClick={() => setDeleting(station)}>
                    <TrashGlyph />
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {deleting && (
        <ConfirmModal
          title="Delete station"
          message={`Delete "${deleting.name}"? It disappears for every user and from Subsonic apps.`}
          confirmLabel="Delete"
          tone="danger"
          busy={saving}
          busyLabel="Deleting…"
          onConfirm={() => { void confirmDelete(); }}
          onCancel={() => setDeleting(null)}
        />
      )}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  root: { display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0, overflowY: 'auto', padding: '12px 20px 24px' },
  toolbar: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 14 },
  heading: { display: 'inline-flex', alignItems: 'center', gap: 8, fontWeight: 600, color: 'var(--text-muted)' },
  addBtn: { display: 'inline-flex', alignItems: 'center', gap: 6, padding: '7px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', cursor: 'pointer', fontFamily: 'inherit', fontWeight: 600 },
  form: { display: 'flex', flexDirection: 'column', gap: 10, padding: 14, marginBottom: 16, borderRadius: 12, border: '1px solid var(--border)', background: 'var(--surface)', maxWidth: 560 },
  field: { display: 'flex', flexDirection: 'column', gap: 4 },
  fieldLabel: { fontSize: 12, fontWeight: 600, color: 'var(--text-muted)' },
  input: { padding: '8px 10px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 14 },
  formError: { color: '#ef4444', fontSize: 13 },
  formActions: { display: 'flex', justifyContent: 'flex-end', gap: 8 },
  primaryBtn: { padding: '7px 14px', borderRadius: 8, border: 'none', background: 'var(--accent)', color: 'var(--bg)', cursor: 'pointer', fontFamily: 'inherit', fontWeight: 700 },
  secondaryBtn: { padding: '7px 14px', borderRadius: 8, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', cursor: 'pointer', fontFamily: 'inherit' },
  notice: { color: 'var(--text-muted)', padding: '12px 0' },
  empty: { display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, padding: '48px 0', color: 'var(--text-muted)' },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))', gap: 12 },
  card: { display: 'flex', alignItems: 'center', gap: 12, padding: 12, borderRadius: 12, border: '1px solid var(--border)', background: 'color-mix(in srgb, var(--surface) 80%, transparent)', minWidth: 0 },
  cardPlaying: { borderColor: 'color-mix(in srgb, var(--accent) 60%, var(--border))' },
  cardPlay: { width: 40, height: 40, borderRadius: '50%', border: 'none', background: 'var(--accent)', color: 'var(--bg)', cursor: 'pointer', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 },
  cardText: { flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 },
  cardTitle: { fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  homepage: { display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 12, color: 'var(--text-muted)', textDecoration: 'none' },
  cardActions: { display: 'flex', gap: 2, flexShrink: 0 },
  iconBtn: { padding: 6, borderRadius: 6, border: 'none', background: 'transparent', color: 'var(--text-muted)', cursor: 'pointer', display: 'inline-flex' },
};
