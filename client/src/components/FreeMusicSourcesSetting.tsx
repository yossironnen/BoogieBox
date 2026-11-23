/**
 * Settings → Integrations → Free music sources: the main metadata switch (`radioKeylessProviders`). Off, the server
 * asks none of MusicBrainz, ListenBrainz, Wikipedia/Wikidata or TheAudioDB; answers already saved still show.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';

/** Card content for the switch (the section title lives in SettingsPage). */
export default function FreeMusicSourcesSetting({ disabled = false }: { disabled?: boolean }) {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(() => {
    api.radioMetadataStatus()
      .then((next) => { setEnabled(next.keylessEnabled); setError(null); })
      .catch(() => setError('Status unavailable.'));
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const save = async (on: boolean) => {
    setSaving(true);
    try {
      await api.settings.update({ radioKeylessProviders: on ? 'true' : 'false' });
      setError(null);
      refresh();
    } catch (e) {
      setError(e instanceof Error && e.message ? e.message : 'Could not save.');
    } finally {
      setSaving(false);
    }
  };

  const busy = disabled || saving || enabled === null;

  return (
    <div
      style={{
        padding: '16px 20px', borderRadius: 8, marginBottom: 12,
        backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
        display: 'grid', gap: 10,
      }}
    >
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 10 }}>
        <input
          type="checkbox"
          aria-label="Use MusicBrainz, ListenBrainz, Wikipedia and TheAudioDB"
          checked={enabled ?? true}
          disabled={busy}
          onChange={(e) => save(e.target.checked)}
          style={{ marginTop: 3, accentColor: 'var(--accent)' }}
        />
        <span style={{ display: 'grid', gap: 2 }}>
          <span style={{ color: 'var(--text)', fontWeight: 600 }}>Use MusicBrainz, ListenBrainz, Wikipedia and TheAudioDB</span>
          <span style={{ color: 'var(--text-muted)', fontSize: 13 }}>
            No account or key needed. Adds artist and album info, top songs, similar artists, genre and mood tags,
            popularity, and Refresh Metadata search results. Artist, album and track names are sent to
            musicbrainz.org, listenbrainz.org, wikipedia.org, wikidata.org and theaudiodb.com.
          </span>
        </span>
      </label>
      {error ? <div role="alert" style={{ color: 'var(--danger, #e0554d)', fontSize: 13 }}>{error}</div> : null}
    </div>
  );
}
