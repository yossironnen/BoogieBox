/**
 * Settings → Advanced → Subsonic apps (admin): turns the OpenSubsonic-compatible `/rest` API on and tunes it.
 * Users set their own Subsonic password under User Settings → Connected apps.
 * See wip/opensubsonic-api-implementation-plan.md §9.1.
 */

import React, { useEffect, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles } from '../hybridPreview';
import { CopyValue, SubsonicIcon } from './ConnectedAppsPanel';
import { InlineStatus, ToggleRow } from './settingsControls';

export const DEFAULT_IGNORED_ARTICLES = 'The El La Los Las Le Les';
const TRANSCODE_LIMITS = [1, 2, 3, 4, 5, 6, 7, 8];

/** Card content for the Subsonic apps settings. */
export default function SubsonicSettings() {
  const [enabled, setEnabled] = useState(false);
  const [maxTranscodes, setMaxTranscodes] = useState('3');
  const [articles, setArticles] = useState(DEFAULT_IGNORED_ARTICLES);
  const [savedArticles, setSavedArticles] = useState(DEFAULT_IGNORED_ARTICLES);
  const [serverUrls, setServerUrls] = useState<string[]>([]);
  const [result, setResult] = useState<string | null>(null);

  useEffect(() => {
    api.settings.get().then((s) => {
      setEnabled(s.subsonicApiEnabled === 'true');
      setMaxTranscodes(s.subsonicMaxTranscodes || '3');
      const words = s.subsonicIgnoredArticles || DEFAULT_IGNORED_ARTICLES;
      setArticles(words);
      setSavedArticles(words);
    }).catch(() => {});
    api.subsonic.credentials().then((c) => setServerUrls(c.serverUrls)).catch(() => {});
  }, []);

  const save = async (updates: Record<string, string>) => {
    try {
      await api.settings.update(updates);
      setResult('Saved');
      return true;
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not save'}`);
      return false;
    }
  };

  const commitArticles = async () => {
    const next = articles.trim().split(/\s+/).filter(Boolean).join(' ');
    if (next === savedArticles) return;
    if (await save({ subsonicIgnoredArticles: next })) {
      setSavedArticles(next || DEFAULT_IGNORED_ARTICLES);
      setArticles(next || DEFAULT_IGNORED_ARTICLES);
    }
  };

  return (
    <div style={S.card}>
      <ToggleRow
        title="Enable Subsonic API"
        checked={enabled}
        onChange={() => {
          const next = !enabled;
          setEnabled(next);
          void save({ subsonicApiEnabled: next ? 'true' : 'false' });
        }}
        description="Lets Subsonic-compatible apps (Symfonium, Feishin, Supersonic, Amperfy and more) browse and play this library. Each user sets their own Subsonic password under User Settings → Connected apps."
      />

      <div style={S.fields}>
        <label style={S.fieldLabel}>
          Simultaneous conversions
          <select
            value={maxTranscodes}
            onChange={(event) => {
              setMaxTranscodes(event.target.value);
              void save({ subsonicMaxTranscodes: event.target.value });
            }}
            style={{ ...hybridControlStyles.select, minWidth: 120 }}
          >
            {TRANSCODE_LIMITS.map((n) => <option key={n} value={String(n)}>{n}</option>)}
          </select>
          <span style={S.help}>How many tracks apps can have converted to MP3 at the same time.</span>
        </label>
        <label style={S.fieldLabel}>
          Ignored articles
          <input
            type="text"
            value={articles}
            maxLength={420}
            onChange={(event) => setArticles(event.target.value)}
            onBlur={() => void commitArticles()}
            onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
            style={{ ...hybridControlStyles.field, minHeight: 38, padding: '7px 10px', fontSize: 14, minWidth: 280 }}
          />
          <span style={S.help}>Words skipped when apps sort artists (The Beatles is listed under B).</span>
        </label>
      </div>

      {serverUrls.length > 0 && (
        <div style={{ display: 'grid', gap: 6, marginBottom: 12 }}>
          {serverUrls.map((url) => <CopyValue key={url} icon={<SubsonicIcon.Server />} label="Server address" value={url} />)}
        </div>
      )}
      <div style={S.help}>
        Apps send the password over your network. For access from outside your home network, put BoogieBox
        behind HTTPS (a reverse proxy or VPN).
      </div>
      <InlineStatus busy={false} busyText="" result={result} />
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  card: {
    padding: '16px 20px',
    borderRadius: 8,
    marginBottom: 12,
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
  },
  fields: { display: 'flex', flexWrap: 'wrap', gap: 20, margin: '4px 0 16px' },
  fieldLabel: { display: 'flex', flexDirection: 'column', gap: 4, fontSize: 14, color: 'var(--text-muted)' },
  help: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.5 },
};
