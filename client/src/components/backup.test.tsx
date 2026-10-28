import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { BackupSummary, BackupsOverview } from '../types';
import { getRestoreOverlay, notifyServerRestoring, resetRestoreOverlay } from '../restoreState';
import BackupSettingsSection from './BackupSettingsSection';
import RestoreBackupDialog from './RestoreBackupDialog';
import RestoreScreen from './RestoreScreen';
import SetupRestoreForm from './SetupRestoreForm';
import { describeBackupWarning } from './SidebarStatusPanel';
import { formatBackupDate, formatBytes, formatNextRun, formatRecentBackupDate, preRestoreFolderName } from './backupUi';

function summary(over: Partial<BackupSummary> = {}): BackupSummary {
  return {
    name: 'boogiebox-20261028-023000-v0.9.43',
    path: 'D:\\Data\\backups\\boogiebox-20261028-023000-v0.9.43',
    createdAt: '2026-10-28T02:30:00Z',
    reason: 'scheduled',
    appVersion: '0.9.43',
    sizeBytes: 48_200_000,
    counts: { tracks: 10412, albums: 900, playlists: 37, users: 4 },
    keyIncluded: true,
    artworkCount: 12,
    mixCount: 0,
    libraryRoots: ['\\\\nas\\music\\Library', 'D:\\Music\\Vinyl rips'],
    missingRoots: [],
    ...over,
  };
}

function overview(over: Partial<BackupsOverview> = {}): BackupsOverview {
  return {
    folder: 'D:\\Data\\backups',
    settings: { schedule: 'weekly', day: 0, time: '03:30', keep: 4, path: '', includeAllArtwork: false, includeMixes: false },
    running: null,
    lastFailure: null,
    nextRun: '2026-11-01T03:30',
    stale: false,
    backups: [
      summary(),
      summary({ name: 'boogiebox-20261027-130200-v0.9.42', createdAt: '2026-10-27T13:02:00Z', reason: 'pre-migration', appVersion: '0.9.42' }),
    ],
    ...over,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  resetRestoreOverlay();
});

describe('backup formatting', () => {
  it('formats dates, sizes and the saved folder name', () => {
    expect(formatBytes(48_200_000)).toBe('46.0 MB');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatNextRun('2026-11-01T03:30')).toBe('Sun 1 Nov, 03:30');
    const local = new Date(2026, 9, 28, 3, 30);
    expect(formatBackupDate(local.toISOString())).toBe('Wed 28 Oct 2026, 03:30');
    expect(formatBackupDate(local.toISOString(), { weekday: false })).toBe('28 Oct 2026, 03:30');
    expect(formatRecentBackupDate(local.toISOString(), new Date(2026, 9, 28, 12))).toBe('Today, 03:30');
    expect(preRestoreFolderName(new Date(Date.UTC(2026, 9, 28, 10, 15, 0)))).toBe('pre-restore-20261028-101500');
  });

  it('describes the sidebar warning only when backups failed or are overdue', () => {
    const now = Date.UTC(2026, 9, 28, 12);
    expect(describeBackupWarning({ state: 'ok', schedule: 'weekly', lastBackupAt: null, lastFailure: null }, now)).toBeNull();
    expect(describeBackupWarning({ state: 'off', schedule: 'off', lastBackupAt: null, lastFailure: null }, now)).toBeNull();
    expect(describeBackupWarning({ state: 'failed', schedule: 'weekly', lastBackupAt: new Date(2026, 9, 20, 12).toISOString(), lastFailure: { at: 'x', message: 'y' } }, now))
      .toEqual({ tone: 'danger', title: 'Backup failed', detail: 'Last good backup: 20 Oct. Open Backup settings.' });
    expect(describeBackupWarning({ state: 'stale', schedule: 'weekly', lastBackupAt: '2026-10-13T03:30:00Z', lastFailure: null }, now))
      .toEqual({ tone: 'warning', title: 'No backup for 15 days', detail: 'Backups are weekly. Open Backup settings.' });
  });
});

describe('BackupSettingsSection', () => {
  beforeEach(() => {
    vi.spyOn(api.backups, 'overview').mockResolvedValue(overview());
  });

  it('shows status tiles, the list and backs up now', async () => {
    const create = vi.spyOn(api.backups, 'create').mockResolvedValue({ ok: true });
    render(<BackupSettingsSection />);
    expect(await screen.findByText('Sun 1 Nov, 03:30')).toBeInTheDocument();
    expect(screen.getByText('1 of 4')).toBeInTheDocument();
    expect(screen.getByText('Before upgrade')).toBeInTheDocument();
    expect(screen.getAllByText('Passwords included')).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: 'Back up now' }));
    await waitFor(() => expect(create).toHaveBeenCalled());
  });

  it('shows the failure banner with the last good backup', async () => {
    vi.spyOn(api.backups, 'overview').mockResolvedValue(overview({
      lastFailure: { at: '2026-10-28T03:30:00Z', message: 'The backup folder could not be written to: E:\\B.' },
    }));
    render(<BackupSettingsSection />);
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('The last backup failed');
    expect(alert).toHaveTextContent('The backup folder could not be written to: E:\\B. Your last good backup is from');
  });

  it('saves the schedule', async () => {
    const save = vi.spyOn(api.backups, 'saveSettings').mockImplementation(async (settings) => overview({ settings }));
    render(<BackupSettingsSection />);
    fireEvent.click(await screen.findByRole('button', { name: 'Daily' }));
    expect(screen.queryByLabelText('Day')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Keep'), { target: { value: '9' } });
    fireEvent.click(screen.getAllByRole('button', { name: 'Save' })[0]);
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ schedule: 'daily', keep: 9 })));
    expect(await screen.findByText('Saved')).toBeInTheDocument();
  });

  it('restores a backup and shows the done screen state', async () => {
    const restore = vi.spyOn(api.backups, 'restore').mockResolvedValue({ ok: true, savedTo: 'x' });
    render(<BackupSettingsSection />);
    fireEvent.click((await screen.findAllByRole('button', { name: 'Restore' }))[0]);
    const dialog = screen.getByRole('alertdialog', { name: 'Restore this backup?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Restore' }));
    await waitFor(() => expect(restore).toHaveBeenCalledWith('boogiebox-20261028-023000-v0.9.43', []));
    await waitFor(() => expect(getRestoreOverlay()).toEqual({ phase: 'done', backupDate: '2026-10-28T02:30:00Z' }));
  });

  it('keeps the dialog open with the server message when a restore fails', async () => {
    vi.spyOn(api.backups, 'restore').mockResolvedValue({ ok: false, pending: true, error: 'Restart BoogieBox to finish the restore.' });
    render(<BackupSettingsSection />);
    fireEvent.click((await screen.findAllByRole('button', { name: 'Restore' }))[0]);
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Restore' }));
    expect(await screen.findByText('Restart BoogieBox to finish the restore.')).toBeInTheDocument();
    expect(getRestoreOverlay()).toBeNull();
  });

  it('confirms before deleting', async () => {
    const remove = vi.spyOn(api.backups, 'remove').mockResolvedValue({ ok: true });
    render(<BackupSettingsSection />);
    const del = (await screen.findAllByRole('button', { name: /^Delete backup from / }))[0];
    fireEvent.click(del);
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith('boogiebox-20261028-023000-v0.9.43'));
  });
});

describe('RestoreBackupDialog', () => {
  it('needs a new location for every missing music folder', () => {
    const onConfirm = vi.fn();
    render(
      <RestoreBackupDialog
        backup={summary({ missingRoots: ['\\\\nas\\music\\Library'] })}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText('Music folders not found on this server')).toBeInTheDocument();
    expect(screen.getByText('D:\\Music\\Vinyl rips')).toBeInTheDocument();
    for (const check of ['The database is intact', 'The checksum matches', 'Made by this version or an older one', 'The password key is included']) {
      expect(screen.getByText(check)).toBeInTheDocument();
    }
    const restore = screen.getByRole('button', { name: 'Restore' });
    expect(restore).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Now'), { target: { value: '/mnt/nas/music/Library' } });
    fireEvent.click(restore);
    expect(onConfirm).toHaveBeenCalledWith([{ from: '\\\\nas\\music\\Library', to: '/mnt/nas/music/Library' }]);
  });
});

describe('RestoreScreen', () => {
  it('waits for another browser\'s restore, then reloads', async () => {
    const status = vi.spyOn(api, 'systemStatus')
      .mockResolvedValueOnce({ ffmpegAvailable: true, setupRequired: false, restoring: true, restoreStep: 'Restoring the backup' })
      .mockResolvedValue({ ffmpegAvailable: true, setupRequired: false, restoring: false });
    const onReload = vi.fn();
    vi.useFakeTimers();
    try {
      render(<RestoreScreen overlay={{ phase: 'restoring', startedHere: false }} onReload={onReload} />);
      expect(screen.getByRole('heading', { name: 'Restoring a backup…' })).toBeInTheDocument();
      await act(async () => { await Promise.resolve(); });
      expect(screen.getByRole('status')).toHaveTextContent('Restoring the backup…');
      await act(async () => { vi.advanceTimersByTime(1600); await Promise.resolve(); });
      expect(status).toHaveBeenCalledTimes(2);
      expect(onReload).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the completed restore with a sign-in button', () => {
    const onReload = vi.fn();
    render(<RestoreScreen overlay={{ phase: 'done', backupDate: null }} onReload={onReload} />);
    expect(screen.getByRole('heading', { name: 'Restore complete' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(onReload).toHaveBeenCalled();
  });

  it('turns a 503 restoring answer into the restoring screen state', () => {
    notifyServerRestoring();
    expect(getRestoreOverlay()).toEqual({ phase: 'restoring', startedHere: false });
  });
});

describe('SetupRestoreForm', () => {
  it('inspects the backup, maps music folders and restores', async () => {
    vi.spyOn(api.backups, 'setupInspect').mockResolvedValue(summary({ missingRoots: ['\\\\nas\\music\\Library'] }));
    const setupRestore = vi.spyOn(api.backups, 'setupRestore').mockResolvedValue({ ok: true });
    const onComplete = vi.fn();
    render(<SetupRestoreForm defaultDbFolder="/var/lib/boogiebox" pickFolder={vi.fn().mockResolvedValue(null)} onComplete={onComplete} />);
    const backup = screen.getByLabelText('Backup folder');
    fireEvent.change(backup, { target: { value: '/media/usb/boogiebox-20261028-023000-v0.9.43' } });
    fireEvent.blur(backup);
    expect(await screen.findByText('Where is your music on this machine?')).toBeInTheDocument();
    expect(screen.getByText('10,412 tracks')).toBeInTheDocument();
    const restore = screen.getByRole('button', { name: 'Restore BoogieBox' });
    expect(restore).toBeDisabled();
    expect(screen.getByRole('alert')).toHaveTextContent('Choose a folder that exists on this machine.');
    fireEvent.change(screen.getByLabelText('New folder for \\\\nas\\music\\Library'), { target: { value: '/mnt/nas/music/Library' } });
    fireEvent.click(restore);
    await waitFor(() => expect(setupRestore).toHaveBeenCalledWith(
      '/media/usb/boogiebox-20261028-023000-v0.9.43',
      '/var/lib/boogiebox',
      [{ from: '\\\\nas\\music\\Library', to: '/mnt/nas/music/Library' }],
    ));
    expect(onComplete).toHaveBeenCalled();
  });
});
