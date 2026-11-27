/**
 * Settings › Advanced › BoogieMix "Deep analysis setup" card (wip/microsoft-store-distribution-plan.md §1,
 * copy in wip/boogiemix-setup-approved-copy.md). Admins install, cancel or remove the deep-analysis
 * environment (Python, PyTorch, Demucs) from here on Windows; Linux shows the install.sh hint instead.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles } from '../hybridPreview';
import type { BoogieMixDeepAnalysisStatus, BoogieMixSetupSnapshot } from '../types';
import { formatBytes } from '../utils';
import ConfirmModal from './ConfirmModal';

type Runtime = BoogieMixDeepAnalysisStatus['runtime'];

const POLL_MS = 2000;
const OUTPUT_LINES = 50;

export const SETUP_STEP_LABELS: Record<string, string> = {
  python: 'Finding or installing Python 3.12',
  venv: 'Creating the Python environment',
  tools: 'Installing build tools',
  torch: 'Installing PyTorch (the largest download)',
  packages: 'Installing analysis packages',
  extras: 'Installing beat tracking',
  models: 'Downloading the Demucs models',
  check: 'Checking the installation',
};

const INTRO =
  'Deep analysis needs Python, PyTorch and the Demucs models on this server. Setup downloads 2–5 GB and takes '
  + '10–30 minutes. BoogieBox keeps working while it runs.';

/** The step's wording; an unknown key falls back to the key itself. */
export function setupStepLabel(key: string): string {
  return SETUP_STEP_LABELS[key] ?? key;
}

/** Why setup failed or couldn't start, for a reason key from the server. */
export function setupReason(key: string, snap: Pick<BoogieMixSetupSnapshot, 'freeBytes' | 'exitCode'> | null): string {
  switch (key) {
    case 'jobs_running':
      return 'Deep analysis is running. Pause it and wait for the current track to finish, then try again.';
    case 'low_disk':
      return `Not enough free space: setup needs 6 GB and ${formatBytes(snap?.freeBytes ?? 0)} is free.`;
    case 'script_missing':
      return 'The setup script is missing from this install. Reinstall BoogieBox Server.';
    case 'spawn_failed':
      return "Setup couldn't start PowerShell.";
    case 'exit_code':
      return `Setup stopped with an error (code ${snap?.exitCode ?? '?'}). The output below and the server log show what went wrong.`;
    case 'wait_failed':
      return 'Setup stopped unexpectedly. The output below and the server log show what went wrong.';
    case 'verify_failed':
      return "Setup finished, but PyTorch or Demucs doesn't load. Try again, or check the output below.";
    case 'remove_failed':
      return "Couldn't remove everything. Close anything using the files and try again.";
    case 'already_running':
      return 'Setup is already running.';
    case 'not_installed':
      return "Deep analysis isn't installed.";
    default:
      return key;
  }
}

export type SetupView = 'running' | 'ready' | 'ready_legacy' | 'failed' | 'cancelled' | 'not_installed';

/** Which status the card shows. */
export function setupView(snap: BoogieMixSetupSnapshot, runtime: Runtime): SetupView {
  if (snap.state === 'running') return 'running';
  if (snap.state === 'failed') return 'failed';
  if (runtime?.enabled) return snap.installed || !snap.supported ? 'ready' : 'ready_legacy';
  if (snap.state === 'cancelled') return 'cancelled';
  return 'not_installed';
}

/** The status row's text. */
export function setupStatusText(view: SetupView, snap: BoogieMixSetupSnapshot, runtime: Runtime): string {
  switch (view) {
    case 'running': {
      const index = snap.step?.index ?? 1;
      const total = snap.step?.total ?? 8;
      return `Installing · step ${index} of ${total}: ${setupStepLabel(snap.step?.key ?? 'python')}`;
    }
    case 'ready':
      return runtime?.gpuAvailable ? 'Ready · GPU (CUDA)' : 'Ready · CPU only';
    case 'ready_legacy':
      return 'Ready · set up by an earlier installer';
    case 'failed':
      return 'Setup failed';
    case 'cancelled':
      return 'Setup cancelled';
    default:
      return 'Not installed';
  }
}

const ICON_PROPS = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
};

/** Chip glyph for the status row. */
function EngineIcon({ size = 18 }: { size?: number }) {
  return (
    <svg {...ICON_PROPS} width={size} height={size}>
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg {...ICON_PROPS} width={16} height={16}>
      <path d="M12 3v12M7 10l5 5 5-5M5 21h14" />
    </svg>
  );
}

function RetryIcon() {
  return (
    <svg {...ICON_PROPS} width={16} height={16}>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg {...ICON_PROPS} width={16} height={16}>
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg {...ICON_PROPS} width={16} height={16}>
      <path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" />
    </svg>
  );
}

type Pending = 'cancel' | 'remove' | null;

interface Props {
  /** Runtime from the deep-analysis status the page already loads. */
  runtime: Runtime;
  /** Called after setup finishes or the environment is removed, so the page reloads the runtime. */
  onChanged: () => void;
}

/** Deep analysis setup card. */
export default function BoogieMixSetupCard({ runtime, onChanged }: Props) {
  const [snap, setSnap] = useState<BoogieMixSetupSnapshot | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending>(null);
  const [showOutput, setShowOutput] = useState(false);
  const wasRunning = useRef(false);
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;

  const load = useCallback(async () => {
    try {
      setSnap(await api.boogiemix.deepAnalysisSetup());
    } catch {
      // Keep the last snapshot; the next poll or page visit tries again.
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const running = snap?.state === 'running';
  useEffect(() => {
    if (wasRunning.current && !running) onChangedRef.current();
    wasRunning.current = running;
    if (!running) return undefined;
    const timer = setInterval(() => { void load(); }, POLL_MS);
    return () => clearInterval(timer);
  }, [running, load]);

  const act = async (action: () => Promise<BoogieMixSetupSnapshot>, after?: () => void) => {
    setBusy(true);
    setActionError(null);
    try {
      setSnap(await action());
      after?.();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setPending(null);
    }
  };

  const cardStyle: React.CSSProperties = {
    padding: '16px 20px', borderRadius: 8, marginBottom: 12,
    backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
  };

  if (!snap) {
    return (
      <div style={cardStyle} data-testid="boogiemix-setup-card">
        <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>Deep analysis setup</div>
        <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>{INTRO}</div>
      </div>
    );
  }

  const view = setupView(snap, runtime);
  const statusText = setupStatusText(view, snap, runtime);
  const ready = view === 'ready' || view === 'ready_legacy';
  const statusColor = ready ? 'var(--success)' : view === 'failed' ? 'var(--danger)' : 'var(--text)';
  const canInstall = snap.supported && (view === 'not_installed' || view === 'cancelled' || view === 'failed');
  const lines = snap.lines.slice(-OUTPUT_LINES);

  return (
    <div style={cardStyle} data-testid="boogiemix-setup-card">
      <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>Deep analysis setup</div>
      <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>{INTRO}</div>

      <div role="status" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 14, fontWeight: 600, color: statusColor }}>
        <span style={{ color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' }}><EngineIcon /></span>
        <span>{statusText}</span>
      </div>
      {view === 'running' && (
        <progress
          aria-label="Setup progress"
          max={snap.step?.total ?? 8}
          value={(snap.step?.index ?? 1) - 1}
          style={{ width: '100%', marginTop: 8, accentColor: 'var(--accent)' }}
        />
      )}
      {view === 'failed' && snap.error && (
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 6, lineHeight: 1.6 }}>{setupReason(snap.error, snap)}</div>
      )}
      {canInstall && snap.freeBytes != null && (
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 6 }}>
          Needs 6 GB free · {formatBytes(snap.freeBytes)} free now
        </div>
      )}

      {!snap.supported ? (
        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 10, lineHeight: 1.6 }}>
          On Linux, deep analysis is set up by the installer: <code>sudo ./install.sh --with-boogiemix</code>
        </div>
      ) : (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
          {canInstall && (
            <button
              type="button"
              style={hybridControlStyles.primaryButton}
              disabled={busy}
              onClick={() => { void act(() => api.boogiemix.installDeepAnalysis()); }}
            >
              {view === 'failed' ? <RetryIcon /> : <DownloadIcon />}
              {view === 'failed' ? 'Try again' : 'Install'}
            </button>
          )}
          {view === 'running' && (
            <button type="button" style={hybridControlStyles.secondaryButton} disabled={busy} onClick={() => setPending('cancel')}>
              <StopIcon />Cancel
            </button>
          )}
          {view === 'ready' && snap.installed && (
            <button type="button" style={hybridControlStyles.secondaryButton} disabled={busy} onClick={() => setPending('remove')}>
              <TrashIcon />Remove
            </button>
          )}
        </div>
      )}
      {actionError && (
        <div style={{ fontSize: 13, color: 'var(--danger)', marginTop: 8, lineHeight: 1.6 }}>{setupReason(actionError, snap)}</div>
      )}

      {snap.supported && (snap.state !== 'idle' || lines.length > 0) && (
        <div style={{ marginTop: 12 }}>
          <button
            type="button"
            style={{ ...hybridControlStyles.secondaryButton, minHeight: 30, padding: '5px 10px', fontSize: 13 }}
            aria-expanded={showOutput}
            onClick={() => setShowOutput(v => !v)}
          >
            {showOutput ? 'Hide setup output' : 'Show setup output'}
          </button>
          {showOutput && (
            <pre
              data-testid="boogiemix-setup-output"
              style={{
                marginTop: 8, maxHeight: 260, overflow: 'auto', padding: '8px 10px', borderRadius: 6,
                background: 'var(--bg)', border: '1px solid var(--border)', fontSize: 12, lineHeight: 1.5,
                whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: 'var(--text-muted)',
              }}
            >
              {lines.length > 0 ? lines.join('\n') : 'No output yet.'}
            </pre>
          )}
        </div>
      )}

      {pending === 'cancel' && (
        <ConfirmModal
          title="Cancel setup?"
          message="What was installed so far is deleted."
          confirmLabel="Cancel setup"
          cancelLabel="Keep installing"
          tone="danger"
          busy={busy}
          onConfirm={() => { void act(() => api.boogiemix.cancelDeepAnalysisSetup()); }}
          onCancel={() => setPending(null)}
        />
      )}
      {pending === 'remove' && (
        <ConfirmModal
          title="Remove deep analysis?"
          message="This deletes the Python environment and the Demucs models (2–5 GB). Analysis already stored for your tracks is kept."
          confirmLabel="Remove"
          tone="danger"
          busy={busy}
          onConfirm={() => { void act(() => api.boogiemix.removeDeepAnalysis(), () => onChangedRef.current()); }}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
