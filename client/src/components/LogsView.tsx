/**
 * Admin Logs view (Settings → Logs): the server log and the two debug logs, newest lines first, with level
 * filter, search, live follow, download and copy. Approved mockup: wip/admin-log-viewer-plan.md §4.1.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api';
import type { LogFileInfo, LogLevel, LogLine, LogList, LogSourceId } from '../types';
import { formatBytes } from '../utils';

/** Lines kept on screen while following; older ones drop off the top. */
const MAX_SHOWN = 2000;
const PAGE = 500;
const RECONNECT_MS = 3000;

const LEVEL_RANK: Record<LogLevel, number> = { trace: 0, debug: 1, info: 2, warn: 3, error: 4 };

type LevelChoice = 'all' | 'error' | 'warn' | 'info' | 'debug';

const LEVELS: { id: LevelChoice; label: string; color?: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'error', label: 'Errors', color: 'var(--danger, #e05d5d)' },
  { id: 'warn', label: 'Warnings', color: 'var(--warning, #d9a441)' },
  { id: 'info', label: 'Info', color: '#6fa8dc' },
  { id: 'debug', label: 'Debug', color: 'var(--text-muted)' },
];

const LEVEL_TEXT: Record<LogLevel, { label: string; color: string }> = {
  error: { label: 'ERROR', color: 'var(--danger, #ef7b7b)' },
  warn: { label: 'WARN', color: 'var(--warning, #e4b45a)' },
  info: { label: 'INFO', color: '#8dbbe6' },
  debug: { label: 'DEBUG', color: 'var(--text-muted)' },
  trace: { label: 'TRACE', color: 'var(--text-muted)' },
};

const SOURCES: { id: LogSourceId; label: string; icon: React.ReactNode; off: string }[] = [
  {
    id: 'server', label: 'Server log', off: '',
    icon: <><rect x="3" y="4" width="18" height="7" rx="1.5" /><rect x="3" y="13" width="18" height="7" rx="1.5" /><path d="M7 7.5h.01M7 16.5h.01" /></>,
  },
  {
    id: 'scan', label: 'Scan debug', off: 'Scan debug logging is off. Turn it on in',
    icon: <><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></>,
  },
  {
    id: 'deep', label: 'Deep analysis debug', off: 'Deep analysis debug logging is off. Turn it on in',
    icon: <path d="M3 12h4l3-8 4 16 3-8h4" />,
  },
];

function Icon({ children, size = 15, color }: { children: React.ReactNode; size?: number; color?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color ?? 'currentColor'} strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ flexShrink: 0 }}>
      {children}
    </svg>
  );
}

const ICON = {
  download: <path d="M12 4v11M7 10l5 5 5-5M5 20h14" />,
  copy: <><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h8" /></>,
  wrap: <path d="M3 6h18M3 12h15a3 3 0 0 1 0 6h-4M3 18h7M16 16l-2 2 2 2" />,
  search: <><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></>,
  file: <><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></>,
  rotated: <path d="M21 12a9 9 0 1 1-3-6.7L21 8M21 3v5h-5" />,
};

const timeFormat = new Intl.DateTimeFormat(undefined, {
  month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit',
});

function shortTime(ts?: string): string {
  if (!ts) return '';
  const date = new Date(ts);
  return Number.isNaN(date.getTime()) ? ts : timeFormat.format(date);
}

/** `boogiebox_server::post_scan` → `post_scan`; the crate root reads as `server`. */
function shortModule(target?: string): string {
  if (!target) return '';
  const last = target.split('::').pop() ?? target;
  return last === 'boogiebox_server' ? 'server' : last;
}

function fileLabel(file: LogFileInfo, source: LogSourceId): string {
  const size = formatBytes(file.size);
  if (source === 'server') return `${file.key === 'server' ? 'Current' : 'Previous (.1)'} · ${size}`;
  const date = file.key.split('.')[1] ?? file.name;
  return `${date} · ${size}`;
}

/** Same rule as the server: an event passes on its level; a plain line follows the line above it. */
function passes(line: LogLine, level: LevelChoice, query: string, previous: boolean): boolean {
  const levelOk = level === 'all'
    ? true
    : line.level ? LEVEL_RANK[line.level] >= LEVEL_RANK[level] : previous;
  if (!levelOk) return false;
  if (!query) return true;
  if (!line.level) return previous || line.text.toLowerCase().includes(query);
  return line.text.toLowerCase().includes(query) || (line.target ?? '').toLowerCase().includes(query);
}

function asText(line: LogLine): string {
  return line.level ? `${line.ts ?? ''} ${LEVEL_TEXT[line.level].label} ${line.target ?? ''}: ${line.text}` : line.text;
}

export default function LogsView({ onOpenDiagnostics }: { onOpenDiagnostics: () => void }) {
  const [list, setList] = useState<LogList | null>(null);
  const [source, setSource] = useState<LogSourceId>('server');
  const [fileKey, setFileKey] = useState<string | null>(null);
  const [level, setLevel] = useState<LevelChoice>('all');
  const [search, setSearch] = useState('');
  const [query, setQuery] = useState('');
  const [follow, setFollow] = useState(false);
  const [wrap, setWrap] = useState(false);
  const [lines, setLines] = useState<LogLine[]>([]);
  const [start, setStart] = useState(0);
  const [end, setEnd] = useState(0);
  const [more, setMore] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [rotated, setRotated] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const endRef = useRef(0);
  endRef.current = end;
  const loadSeq = useRef(0);

  const loadList = useCallback(() => api.admin.logs.list().then(setList).catch(() => setFailed(true)), []);
  useEffect(() => { void loadList(); }, [loadList]);

  const current = list?.sources.find(s => s.id === source) ?? null;
  const file = current?.files.find(f => f.key === fileKey) ?? null;

  // A new source (or a fresh list) selects its current file.
  useEffect(() => {
    if (!current) return;
    if (!current.files.some(f => f.key === fileKey)) setFileKey(current.files[0]?.key ?? null);
  }, [current, fileKey]);

  useEffect(() => {
    const timer = window.setTimeout(() => setQuery(search.trim().toLowerCase()), 300);
    return () => window.clearTimeout(timer);
  }, [search]);

  const filterParams = useMemo(() => ({
    level: level === 'all' ? undefined : level as LogLevel,
    q: query || undefined,
  }), [level, query]);

  const loadPage = useCallback(async () => {
    const seq = ++loadSeq.current;
    if (!fileKey) { setLines([]); setLoading(false); return; }
    setLoading(true);
    setFailed(false);
    setRotated(false);
    try {
      const page = await api.admin.logs.lines(fileKey, { ...filterParams, limit: PAGE });
      if (seq !== loadSeq.current) return;
      setLines(page.lines);
      setStart(page.start);
      setEnd(page.end);
      setMore(page.more);
      setTruncated(page.truncated);
    } catch {
      if (seq === loadSeq.current) { setLines([]); setFailed(true); }
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, [fileKey, filterParams]);
  useEffect(() => { void loadPage(); }, [loadPage]);

  const loadOlder = async () => {
    if (!fileKey) return;
    const seq = loadSeq.current;
    try {
      const page = await api.admin.logs.lines(fileKey, { ...filterParams, before: start, limit: PAGE });
      if (seq !== loadSeq.current) return;
      setLines(prev => [...page.lines, ...prev]);
      setStart(page.start);
      setMore(page.more);
      setTruncated(page.truncated);
    } catch {
      setFailed(true);
    }
  };

  // Follow: one EventSource from the last offset read; reconnects itself from where it got to.
  useEffect(() => {
    if (!follow || !fileKey || loading || typeof EventSource === 'undefined') return undefined;
    let source: EventSource | null = null;
    let retry = 0;
    let closed = false;
    const open = () => {
      source = new EventSource(api.admin.logs.followUrl(fileKey, endRef.current), { withCredentials: true });
      source.addEventListener('lines', (event) => {
        const data = JSON.parse((event as MessageEvent).data) as { lines: LogLine[]; end: number };
        setEnd(data.end);
        setLines(prev => {
          const next = [...prev];
          let previous = prev.length > 0;
          for (const line of data.lines) {
            previous = passes(line, level, query, previous);
            if (previous) next.push(line);
          }
          return next.length > MAX_SHOWN ? next.slice(next.length - MAX_SHOWN) : next;
        });
      });
      source.addEventListener('rotated', (event) => {
        const data = JSON.parse((event as MessageEvent).data) as { key: string };
        setRotated(true);
        setEnd(0);
        if (data.key !== fileKey) {
          // A debug log moved to a new day: pick it up in the list and keep following it.
          void loadList();
          setFileKey(data.key);
        }
      });
      source.onerror = () => {
        source?.close();
        if (!closed) retry = window.setTimeout(open, RECONNECT_MS);
      };
    };
    open();
    return () => { closed = true; window.clearTimeout(retry); source?.close(); };
  }, [follow, fileKey, loading, level, query, loadList]);

  // While following, stay at the newest line.
  useEffect(() => {
    if (!follow) return;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines, follow]);

  const copyLines = () => { void navigator.clipboard?.writeText(lines.map(asText).join('\n')); };
  const copyPath = () => { if (list?.folder) void navigator.clipboard?.writeText(list.folder); };

  const sourceOff = current && !current.enabled;
  const showOff = !!sourceOff && (current?.files.length ?? 0) === 0;
  const offText = SOURCES.find(s => s.id === source)?.off ?? '';
  const filtering = level !== 'all' || !!query;

  return (
    <section aria-label="Logs" style={S.panel}>
      <div style={S.row}>
        <div role="tablist" aria-label="Log" style={S.sourceTabs}>
          {SOURCES.map(s => {
            const info = list?.sources.find(x => x.id === s.id);
            const active = s.id === source;
            return (
              <button key={s.id} type="button" role="tab" aria-selected={active}
                style={{ ...S.sourceTab, ...(active ? S.sourceTabActive : {}) }}
                onClick={() => { setSource(s.id); setFileKey(null); setFollow(false); }}>
                <Icon size={14}>{s.icon}</Icon>{s.label}
                {info && !info.enabled && <span style={S.offBadge}>Off</span>}
              </button>
            );
          })}
        </div>
        {current && current.files.length > 0 && (
          <label style={S.fileLabel}>File
            <select value={fileKey ?? ''} style={S.select} onChange={e => { setFileKey(e.target.value); setFollow(false); }}>
              {current.files.map(f => <option key={f.key} value={f.key}>{fileLabel(f, source)}</option>)}
            </select>
          </label>
        )}
        <div style={{ flex: 1 }} />
        {fileKey && (
          <>
            <a href={api.admin.logs.downloadUrl(fileKey)} download aria-label="Download" title="Download" style={S.iconBtn}>
              <Icon size={16}>{ICON.download}</Icon>
            </a>
            <button type="button" aria-label="Copy lines" title="Copy lines" style={S.iconBtn} onClick={copyLines}>
              <Icon size={16}>{ICON.copy}</Icon>
            </button>
            <button type="button" aria-label="Wrap lines" title="Wrap lines" aria-pressed={wrap}
              style={{ ...S.iconBtn, ...(wrap ? S.iconBtnOn : {}) }} onClick={() => setWrap(v => !v)}>
              <Icon size={16}>{ICON.wrap}</Icon>
            </button>
          </>
        )}
      </div>

      {showOff ? (
        <div role="status" style={S.emptyBox}>
          <Icon size={30} color="var(--accent)">{ICON.file}</Icon>
          <div style={{ fontSize: 14 }}>
            {offText}{' '}
            <a href="#advanced-debug" onClick={(e) => { e.preventDefault(); onOpenDiagnostics(); }}>Advanced → Diagnostics</a>
            {source === 'scan' ? ' to record scans.' : ' to record deep analysis.'}
          </div>
        </div>
      ) : (
        <>
          <div style={S.row}>
            <div role="group" aria-label="Level" style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
              {LEVELS.map(l => (
                <button key={l.id} type="button" aria-pressed={level === l.id}
                  style={{ ...S.chip, ...(level === l.id ? S.chipOn : {}) }} onClick={() => setLevel(l.id)}>
                  {l.color && <span aria-hidden="true" style={{ ...S.dot, background: l.color }} />}{l.label}
                </button>
              ))}
            </div>
            <label style={S.search}>
              <Icon size={14}>{ICON.search}</Icon>
              <input type="search" aria-label="Search logs" placeholder="Search logs" value={search}
                onChange={e => setSearch(e.target.value)} style={S.searchInput} />
            </label>
            <label style={S.followLabel}>
              <input type="checkbox" role="switch" checked={follow} disabled={!file?.current}
                onChange={e => setFollow(e.target.checked)} style={S.switch} />Follow
            </label>
          </div>

          {rotated && (
            <div role="status" style={S.notice}>
              <Icon size={14} color="var(--accent)">{ICON.rotated}</Icon>The log file was replaced; following the new one.
            </div>
          )}

          <div ref={scrollRef} style={S.lines} data-testid="log-lines">
            {more && !loading && (
              <div style={{ textAlign: 'center', padding: '2px 0 8px' }}>
                <button type="button" style={S.olderBtn} onClick={() => { void loadOlder(); }}>Load older lines</button>
              </div>
            )}
            {failed && <div role="alert" style={S.message}>Couldn&rsquo;t read this log.</div>}
            {file && !failed && !loading && lines.length === 0 && (
              <div role="status" style={S.message}>
                {filtering ? 'No lines match.' : 'This log is empty.'}
              </div>
            )}
            {lines.map(line => (
              <div key={line.offset} style={{ ...S.line, ...(wrap ? S.wrap : S.nowrap), ...(line.level === 'error' ? S.errorRow : line.level === 'warn' ? S.warnRow : {}) }}>
                <span title={line.ts} style={S.time}>{shortTime(line.ts)}</span>
                <span style={{ ...S.level, color: line.level ? LEVEL_TEXT[line.level].color : undefined }}>
                  {line.level ? LEVEL_TEXT[line.level].label : ''}
                </span>
                <span style={S.module}>{shortModule(line.target)}</span>
                <span style={line.level ? S.text : S.plainText}>{line.text}</span>
              </div>
            ))}
          </div>
          {truncated && <div style={S.footerNote}>Older lines are in the downloaded file.</div>}
        </>
      )}

      <div style={S.footer}>
        {file && <span>Showing {lines.length.toLocaleString()} lines · {formatBytes(file.size)} file</span>}
        <span style={{ flex: 1 }} />
        {list?.folder && (
          <>
            <span>Log folder</span>
            <code style={S.path}>{list.folder}</code>
            <button type="button" style={S.smallBtn} onClick={copyPath}>Copy path</button>
          </>
        )}
      </div>
    </section>
  );
}

const mono = "'JetBrains Mono', Consolas, 'SFMono-Regular', monospace";

const S: Record<string, React.CSSProperties> = {
  panel: {
    display: 'flex', flexDirection: 'column', gap: 14, padding: '18px 20px', borderRadius: 18,
    border: '1px solid var(--border)', background: 'var(--surface)', minHeight: 520,
  },
  row: { display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' },
  sourceTabs: { display: 'flex', gap: 6, padding: 4, borderRadius: 999, border: '1px solid var(--border)', flexWrap: 'wrap' },
  sourceTab: {
    display: 'flex', alignItems: 'center', gap: 7, padding: '7px 13px', borderRadius: 999, border: '1px solid transparent',
    background: 'none', color: 'var(--text-muted)', fontSize: 13, fontWeight: 700, cursor: 'pointer', fontFamily: 'inherit',
  },
  sourceTabActive: { borderColor: 'var(--border)', background: 'color-mix(in srgb, var(--accent) 12%, var(--bg))', color: 'var(--text)' },
  offBadge: { marginLeft: 4, padding: '1px 7px', borderRadius: 999, background: 'color-mix(in srgb, var(--border) 80%, transparent)', fontSize: 11 },
  fileLabel: { display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: 'var(--text-muted)' },
  select: {
    padding: '7px 10px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)',
    color: 'var(--text)', fontSize: 13, fontFamily: 'inherit',
  },
  iconBtn: {
    width: 36, height: 36, borderRadius: 10, border: '1px solid var(--border)', background: 'var(--bg)',
    color: 'var(--text)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer', padding: 0,
  },
  iconBtnOn: { borderColor: 'var(--accent)', color: 'var(--accent)' },
  chip: {
    display: 'flex', alignItems: 'center', gap: 6, padding: '6px 11px', borderRadius: 999, border: '1px solid var(--border)',
    background: 'none', color: 'var(--text-muted)', fontSize: 13, fontWeight: 700, cursor: 'pointer', fontFamily: 'inherit',
  },
  chipOn: { borderColor: 'var(--accent)', background: 'color-mix(in srgb, var(--accent) 14%, var(--bg))', color: 'var(--text)' },
  dot: { width: 8, height: 8, borderRadius: 999, flexShrink: 0 },
  search: {
    flex: 1, minWidth: 200, display: 'flex', alignItems: 'center', gap: 8, padding: '7px 12px',
    border: '1px solid var(--border)', borderRadius: 10, color: 'var(--text-muted)',
  },
  searchInput: { flex: 1, minWidth: 0, background: 'none', border: 'none', color: 'var(--text)', fontSize: 14, outline: 'none', fontFamily: 'inherit' },
  followLabel: { display: 'flex', alignItems: 'center', gap: 8, fontSize: 14, fontWeight: 700 },
  switch: { accentColor: 'var(--accent)', width: 18, height: 18 },
  notice: {
    display: 'flex', alignItems: 'center', gap: 8, alignSelf: 'flex-start', padding: '6px 10px', borderRadius: 8,
    background: 'color-mix(in srgb, var(--accent) 10%, var(--bg))', fontSize: 13,
  },
  lines: {
    flex: 1, minHeight: 360, maxHeight: '62vh', overflow: 'auto', border: '1px solid var(--border)', borderRadius: 12,
    background: 'color-mix(in srgb, var(--bg) 85%, black)', fontFamily: mono, fontSize: 12.5, lineHeight: 1.75, padding: '10px 0',
  },
  line: { display: 'flex', gap: 12, padding: '0 14px' },
  nowrap: { whiteSpace: 'nowrap' },
  wrap: { whiteSpace: 'pre-wrap', wordBreak: 'break-word' },
  errorRow: { background: 'color-mix(in srgb, var(--danger, #e05d5d) 12%, transparent)' },
  warnRow: { background: 'color-mix(in srgb, var(--warning, #d9a441) 9%, transparent)' },
  time: { color: 'var(--text-muted)', flexShrink: 0, width: 132 },
  level: { flexShrink: 0, width: 46, fontWeight: 500 },
  module: { color: 'var(--text-muted)', flexShrink: 0, width: 104, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  text: { color: 'var(--text)', minWidth: 0 },
  plainText: { color: 'var(--text-muted)', fontStyle: 'italic', minWidth: 0 },
  olderBtn: {
    padding: '5px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--surface)',
    color: 'var(--text)', fontSize: 12.5, fontWeight: 700, cursor: 'pointer', fontFamily: 'inherit',
  },
  message: { padding: '24px 16px', textAlign: 'center', fontFamily: 'inherit', fontSize: 14, color: 'var(--text)' },
  emptyBox: {
    display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10, padding: '34px 20px',
    border: '1px dashed var(--border)', borderRadius: 12, textAlign: 'center',
  },
  footerNote: { fontSize: 13, color: 'var(--text-muted)' },
  footer: { display: 'flex', alignItems: 'center', gap: 14, flexWrap: 'wrap', fontSize: 13, color: 'var(--text-muted)' },
  path: { fontFamily: mono, color: 'var(--text)', wordBreak: 'break-all' },
  smallBtn: {
    padding: '5px 10px', borderRadius: 8, border: '1px solid var(--border)', background: 'none',
    color: 'var(--text)', fontSize: 12.5, fontWeight: 700, cursor: 'pointer', fontFamily: 'inherit',
  },
};
