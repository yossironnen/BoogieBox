import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import LibrarySettingsTab, { formatPendingStatus } from './LibrarySettingsTab';
import type { ScanJob } from '../types';

const { apiMock, platformMock } = vi.hoisted(() => ({
  apiMock: {
    libraries: {
      add: vi.fn(),
      remove: vi.fn(),
      addFolder: vi.fn(),
      removeFolder: vi.fn(),
      rename: vi.fn(),
      scan: vi.fn(),
    },
    debugTestPath: vi.fn(),
    scanJobs: { active: vi.fn(), get: vi.fn() },
  },
  platformMock: { isDesktop: false, selectFolder: vi.fn() },
}));

vi.mock('../api', () => ({ api: apiMock }));
vi.mock('../platform', () => ({ platform: platformMock }));
vi.mock('./FolderPickerModal', () => ({
  default: ({ initialPath, onSelect, onClose }: any) => (
    <div>
      picker-{initialPath || 'root'}
      <button onClick={() => onSelect('/picked/music')}>pick-folder</button>
      <button onClick={onClose}>close-picker</button>
    </div>
  ),
}));

const library = {
  id: '1',
  name: 'Music',
  path: '/music',
  primary_path: '/music',
  folders: [
    { id: 'f1', library_id: '1', path: '/music', position: 0 },
    { id: 'f2', library_id: '1', path: '/more', position: 1 },
  ],
  folder_count: 2,
  track_count: 12,
  last_scan: '2026-01-01T00:00:00Z',
} as any;

describe('LibrarySettingsTab', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    platformMock.isDesktop = false;
    apiMock.libraries.add.mockResolvedValue({ ok: true });
    apiMock.libraries.remove.mockResolvedValue({ ok: true });
    apiMock.libraries.addFolder.mockResolvedValue({ ok: true });
    apiMock.libraries.removeFolder.mockResolvedValue({ ok: true });
    apiMock.libraries.rename.mockResolvedValue({ ok: true });
    apiMock.libraries.scan.mockResolvedValue({ jobId: 'j1' });
    apiMock.scanJobs.active.mockResolvedValue([]);
    apiMock.scanJobs.get.mockResolvedValue({
      id: 'j1', library_id: '1', status: 'done', files_found: 10,
      files_scanned: 10, errors: 0, started_at: '2026-01-01', finished_at: '2026-01-01',
    });
  });

  it('queues, removes, browses, tests, and creates multi-folder libraries', async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    render(<LibrarySettingsTab libraries={[]} onRefresh={onRefresh} />);
    expect(screen.getByText('No libraries added yet.')).toBeInTheDocument();
    const path = screen.getByPlaceholderText(/Folder path/i);
    fireEvent.change(path, { target: { value: ' /one ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Queue Folder' }));
    expect(screen.getByText('/one')).toBeInTheDocument();
    fireEvent.change(path, { target: { value: '/two' } });
    fireEvent.change(screen.getByPlaceholderText(/Name \(optional\)/i), { target: { value: ' Collection ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(apiMock.libraries.add).toHaveBeenCalledWith(['/one', '/two'], 'Collection'));
    expect(onRefresh).toHaveBeenCalled();

    fireEvent.change(path, { target: { value: '/test' } });
    apiMock.debugTestPath
      .mockResolvedValueOnce({ exists: true, isDirectory: true, displayName: 'test' })
      .mockResolvedValueOnce({ exists: true, isDirectory: false, error: 'file' })
      .mockResolvedValueOnce({ exists: false, isDirectory: false, normalized: '/missing' });
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText(/Path OK/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText(/not a directory/)).toBeInTheDocument();
    expect(screen.getByText('file')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText(/Path not found/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));
    expect(screen.getByText('picker-/test')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'pick-folder' }));
    await waitFor(() => expect(path).toHaveValue('/picked/music'));
  });

  it('handles create/test/picker failures and queued-folder removal', async () => {
    apiMock.libraries.add.mockRejectedValueOnce(new Error('Add failed'));
    apiMock.debugTestPath.mockRejectedValueOnce(new Error('Test failed'));
    render(<LibrarySettingsTab libraries={[]} />);
    const path = screen.getByPlaceholderText(/Folder path/i);
    fireEvent.change(path, { target: { value: '/queued' } });
    fireEvent.click(screen.getByRole('button', { name: 'Queue Folder' }));
    fireEvent.click(screen.getByTitle('Remove queued folder'));
    expect(screen.queryByText('/queued')).not.toBeInTheDocument();

    fireEvent.change(path, { target: { value: '/bad' } });
    fireEvent.keyDown(path, { key: 'Enter' });
    expect(await screen.findByText('Add failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText('Test failed')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));
    fireEvent.click(screen.getByRole('button', { name: 'close-picker' }));
    expect(path).toHaveValue('/bad');
  });

  it('renames, adds/removes folders and libraries, and reports action errors', async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    render(<LibrarySettingsTab libraries={[library]} onRefresh={onRefresh} />);
    await waitFor(() => expect(apiMock.scanJobs.active).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const name = screen.getByLabelText(/Library name for/i);
    fireEvent.change(name, { target: { value: ' ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('Library name is required')).toBeInTheDocument();
    fireEvent.change(name, { target: { value: 'Renamed' } });
    fireEvent.keyDown(name, { key: 'Enter' });
    await waitFor(() => expect(apiMock.libraries.rename).toHaveBeenCalledWith('1', 'Renamed'));

    const folder = screen.getByPlaceholderText('Add another folder');
    fireEvent.change(folder, { target: { value: ' /third ' } });
    fireEvent.keyDown(folder, { key: 'Enter' });
    await waitFor(() => expect(apiMock.libraries.addFolder).toHaveBeenCalledWith('1', '/third'));

    fireEvent.click(screen.getAllByTitle('Remove folder')[0]);
    expect(screen.getByText('Remove this folder from the library?')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(apiMock.libraries.removeFolder).not.toHaveBeenCalled();
    fireEvent.click(screen.getAllByTitle('Remove folder')[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Remove Folder' }));
    await waitFor(() => expect(apiMock.libraries.removeFolder).toHaveBeenCalledWith('1', 'f1'));
    fireEvent.click(screen.getByTitle('Remove library'));
    fireEvent.click(screen.getByRole('button', { name: 'Remove Library' }));
    await waitFor(() => expect(apiMock.libraries.remove).toHaveBeenCalledWith('1'));

    apiMock.libraries.addFolder.mockRejectedValueOnce(new Error('Folder failed'));
    fireEvent.change(folder, { target: { value: '/failed' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add Folder' }));
    expect(await screen.findByText('Folder failed')).toBeInTheDocument();
  });

  it('uses native desktop folder selection and starts scans', async () => {
    platformMock.isDesktop = true;
    platformMock.selectFolder.mockResolvedValue('/desktop/music');
    render(<LibrarySettingsTab libraries={[{ ...library, folders: undefined, folder_count: 1 }]} />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Browse' })[1]);
    await waitFor(() => expect(platformMock.selectFolder).toHaveBeenCalled());
    expect(screen.getByPlaceholderText('Add another folder')).toHaveValue('/desktop/music');
    fireEvent.click(screen.getByRole('button', { name: 'Scan' }));
    await waitFor(() => expect(apiMock.libraries.scan).toHaveBeenCalledWith('1'));
    expect(screen.getByText('Queued...')).toBeInTheDocument();
  });

  it('formats every pending scan queue state', () => {
    const pending = {
      id: 'job-1',
      library_id: '1',
      status: 'pending',
      files_found: 0,
      files_scanned: 0,
      errors: 0,
      started_at: new Date().toISOString(),
      finished_at: null,
    } as ScanJob;
    expect(formatPendingStatus(pending)).toBe('Queued...');
    expect(formatPendingStatus({ ...pending, started_at: 'invalid' })).toBe('Queued...');
    expect(formatPendingStatus({ ...pending, started_at: new Date(Date.now() - 125_000).toISOString() }))
      .toMatch(/Queued - waiting for scan worker \(2m\)/);
    expect(formatPendingStatus({
      ...pending,
      queue_position: 3,
      running_job: {
        id: 'run-1',
        library_id: 'other-library',
        started_at: new Date().toISOString(),
        library_name: 'Other Library',
      },
    })).toBe('Queue #3 - Running now: Other Library (job #run-1)');
    expect(formatPendingStatus({
      ...pending,
      queue_position: 0,
      running_job: {
        id: 'run-2',
        library_id: 'main-library',
        started_at: new Date().toISOString(),
        library_name: 'Main',
      },
    })).toBe('Queued - Running now: Main (job #run-2)');
  });

  it('reports rename, folder removal, library removal, scan, and native picker failures', async () => {
    platformMock.isDesktop = true;
    platformMock.selectFolder.mockRejectedValue(new Error('Picker failed'));
    apiMock.libraries.rename.mockRejectedValue(new Error('Rename failed'));
    apiMock.libraries.removeFolder.mockRejectedValue(new Error('Remove folder failed'));
    apiMock.libraries.remove.mockRejectedValue(new Error('Remove library failed'));
    apiMock.libraries.scan.mockRejectedValue(new Error('Scan failed'));
    render(<LibrarySettingsTab libraries={[library]} />);

    fireEvent.click(screen.getAllByRole('button', { name: 'Browse' })[0]);
    expect(await screen.findByText('Picker failed')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    fireEvent.change(screen.getByLabelText(/Library name for/i), { target: { value: 'Broken' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Rename failed')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByLabelText(/Library name for/i), { key: 'Escape' });

    fireEvent.click(screen.getAllByTitle('Remove folder')[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Remove Folder' }));
    expect(await screen.findByText('Remove folder failed')).toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Remove library'));
    fireEvent.click(screen.getByRole('button', { name: 'Remove Library' }));
    expect(await screen.findByText('Remove library failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Scan' }));
    expect(await screen.findByText('Scan failed')).toBeInTheDocument();
  });
});
