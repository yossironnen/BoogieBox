import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import FolderPickerModal from './FolderPickerModal';

const { fsBrowse, fsMkdir } = vi.hoisted(() => ({
  fsBrowse: vi.fn(),
  fsMkdir: vi.fn(),
}));

vi.mock('../api', () => ({
  api: { fsBrowse, fsMkdir },
}));

const musicResult = {
  path: 'D:\\Music',
  parent: 'D:\\',
  entries: [{ name: 'Albums', path: 'D:\\Music\\Albums' }],
};

describe('FolderPickerModal', () => {
  beforeEach(() => {
    fsBrowse.mockReset();
    fsMkdir.mockReset();
    fsBrowse.mockResolvedValue(musicResult);
    fsMkdir.mockResolvedValue({ path: 'D:\\Music\\New' });
  });

  it('falls back from an invalid initial path and supports navigation and selection', async () => {
    fsBrowse
      .mockRejectedValueOnce(new Error('missing'))
      .mockResolvedValueOnce(musicResult)
      .mockResolvedValueOnce({ ...musicResult, path: 'D:\\Music\\Albums', parent: 'D:\\Music', entries: [] })
      .mockResolvedValueOnce(musicResult)
      .mockResolvedValueOnce({ path: '', parent: null, entries: [] });
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const { container } = render(
      <FolderPickerModal initialPath={'Z:\\Missing'} onSelect={onSelect} onClose={onClose} />,
    );

    expect(await screen.findByText('Albums')).toBeInTheDocument();
    expect(fsBrowse).toHaveBeenNthCalledWith(1, 'Z:\\Missing');
    expect(fsBrowse).toHaveBeenNthCalledWith(2, undefined);

    const entry = screen.getByText('Albums');
    fireEvent.mouseEnter(entry);
    fireEvent.mouseLeave(entry);
    fireEvent.click(entry);
    expect(await screen.findByText('No subfolders')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Up/ }));
    await screen.findByText('Albums');
    fireEvent.click(screen.getByRole('button', { name: /Root/ }));
    await waitFor(() => expect(screen.getByText('Drives')).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Select' })).toBeDisabled();

    fsBrowse.mockResolvedValueOnce(musicResult);
    fireEvent.click(screen.getByRole('button', { name: /Path/ }));
    const pathInput = screen.getByPlaceholderText(/e\.g\./);
    fireEvent.change(pathInput, { target: { value: ' D:\\Music ' } });
    fireEvent.keyDown(pathInput, { key: 'Enter' });
    await screen.findByText('Albums');
    fireEvent.click(screen.getByRole('button', { name: 'Select' }));
    expect(onSelect).toHaveBeenCalledWith('D:\\Music');

    fireEvent.click(container.firstElementChild as HTMLElement);
    expect(onClose).toHaveBeenCalled();
  });

  it('creates folders and validates or reports creation failures', async () => {
    fsBrowse
      .mockResolvedValueOnce(musicResult)
      .mockResolvedValueOnce({ path: 'D:\\Music\\New', parent: 'D:\\Music', entries: [] });
    render(<FolderPickerModal onSelect={vi.fn()} onClose={vi.fn()} />);
    await screen.findByText('Albums');

    fireEvent.click(screen.getByRole('button', { name: /New Folder/ }));
    const input = screen.getByPlaceholderText('New folder name');
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(screen.getByText('Folder name cannot be empty')).toBeInTheDocument();

    fireEvent.change(input, { target: { value: ' New ' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(fsMkdir).toHaveBeenCalledWith('D:\\Music', 'New'));
    expect(await screen.findByText('No subfolders')).toBeInTheDocument();

    fsBrowse.mockResolvedValueOnce(musicResult);
    fireEvent.click(screen.getByRole('button', { name: /Root/ }));
    await screen.findByText('Albums');
    fsMkdir.mockRejectedValueOnce(new Error('read only'));
    fireEvent.click(screen.getByRole('button', { name: /New Folder/ }));
    fireEvent.change(screen.getByPlaceholderText('New folder name'), { target: { value: 'Denied' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(await screen.findByText('read only')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByPlaceholderText('New folder name'), { key: 'Escape' });
    expect(screen.queryByPlaceholderText('New folder name')).not.toBeInTheDocument();
  });

  it('supports path cancellation, browse errors, and close controls', async () => {
    fsBrowse.mockResolvedValueOnce(musicResult);
    const onClose = vi.fn();
    render(<FolderPickerModal onSelect={vi.fn()} onClose={onClose} />);
    await screen.findByText('Albums');

    fireEvent.click(screen.getByRole('button', { name: /Path/ }));
    const input = screen.getByPlaceholderText(/e\.g\./);
    fireEvent.change(input, { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Go' }));
    expect(fsBrowse).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(input, { key: 'Escape' });

    fsBrowse.mockRejectedValueOnce(new Error('browse denied'));
    fireEvent.click(screen.getByText('Albums'));
    expect(await screen.findByText('browse denied')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalled();
  });

  it('shows the fallback browse failure when both initial and root paths fail', async () => {
    fsBrowse
      .mockRejectedValueOnce(new Error('invalid initial'))
      .mockRejectedValueOnce(new Error('root unavailable'));
    render(<FolderPickerModal initialPath="bad" onSelect={vi.fn()} onClose={vi.fn()} />);

    expect(await screen.findByText('root unavailable')).toBeInTheDocument();
  });
});
