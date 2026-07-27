/**
 * Tests Setup View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import SetupView from './SetupView';

const { apiMock, platformMock } = vi.hoisted(() => ({
  apiMock: {
    systemStatus: vi.fn(),
    systemSetup: vi.fn(),
  },
  platformMock: {
    isDesktop: true,
    selectFolder: vi.fn(),
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

vi.mock('../platform', () => ({
  platform: platformMock,
}));
vi.mock('./FolderPickerModal', () => ({
  default: ({ initialPath, onSelect, onClose }: any) => (
    <div>
      picker-{initialPath || 'root'}
      <button onClick={() => onSelect('/web/data')}>select-web-folder</button>
      <button onClick={onClose}>close-web-folder</button>
    </div>
  ),
}));

describe('SetupView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    platformMock.isDesktop = true;
    apiMock.systemStatus.mockResolvedValue({
      ffmpegAvailable: true,
      setupRequired: true,
      suggestedDbFolder: 'C:\\Users\\Yossi\\AppData\\Local\\BoogieBox',
    });
    apiMock.systemSetup.mockResolvedValue({ ok: true });
    platformMock.selectFolder.mockResolvedValue(null);
  });

  it('prefills the server suggested database folder and still allows manual editing', async () => {
    const onComplete = vi.fn();
    render(<SetupView onComplete={onComplete} />);

    const input = screen.getByLabelText(/Database folder/i);
    await waitFor(() => expect(input).toHaveValue('C:\\Users\\Yossi\\AppData\\Local\\BoogieBox'));
    expect(screen.getByRole('heading', { name: 'Set up your server' })).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Setup progress' })).toHaveTextContent('1. Choose storage');
    expect(screen.getByText(/does not move your music/i)).toBeInTheDocument();

    fireEvent.change(input, { target: { value: '\\\\server\\share\\boogieboxdb' } });
    fireEvent.click(screen.getByRole('button', { name: /Set up BoogieBox/i }));

    await waitFor(() => expect(apiMock.systemSetup).toHaveBeenCalledWith('\\\\server\\share\\boogieboxdb'));
    expect(onComplete).toHaveBeenCalled();
  });

  it('uses the selected native folder when Browse returns one', async () => {
    platformMock.selectFolder.mockResolvedValueOnce('D:\\BoogieBoxData');

    render(<SetupView onComplete={vi.fn()} />);

    await waitFor(() => expect(screen.getByLabelText(/Database folder/i)).toHaveValue('C:\\Users\\Yossi\\AppData\\Local\\BoogieBox'));
    fireEvent.click(screen.getByRole('button', { name: /Browse/i }));

    await waitFor(() => expect(platformMock.selectFolder).toHaveBeenCalledWith('C:\\Users\\Yossi\\AppData\\Local\\BoogieBox'));
    expect(screen.getByLabelText(/Database folder/i)).toHaveValue('D:\\BoogieBoxData');
  });

  it('shows an error when the folder picker cannot open', async () => {
    platformMock.selectFolder.mockRejectedValueOnce(new Error('Folder picker failed'));

    render(<SetupView onComplete={vi.fn()} />);

    await waitFor(() => expect(screen.getByLabelText(/Database folder/i)).toHaveValue('C:\\Users\\Yossi\\AppData\\Local\\BoogieBox'));
    fireEvent.click(screen.getByRole('button', { name: /Browse/i }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Folder picker failed');
  });

  it('keeps the fallback default if the suggestion request fails', async () => {
    apiMock.systemStatus.mockRejectedValueOnce(new Error('offline'));

    render(<SetupView onComplete={vi.fn()} />);

    expect(screen.getByLabelText(/Database folder/i)).toHaveValue('C:\\Users\\Public\\BoogieBox');
  });

  it('uses and cancels the web folder picker, including an empty initial path', async () => {
    platformMock.isDesktop = false;
    apiMock.systemStatus.mockResolvedValue({ setupRequired: true, suggestedDbFolder: '   ' });
    render(<SetupView onComplete={vi.fn()} />);
    const input = screen.getByLabelText(/Database folder/i);
    fireEvent.change(input, { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: /Browse/i }));
    expect(screen.getByText('picker-root')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'close-web-folder' }));
    expect(input).toHaveValue('');

    fireEvent.click(screen.getByRole('button', { name: /Browse/i }));
    fireEvent.click(screen.getByRole('button', { name: 'select-web-folder' }));
    await waitFor(() => expect(input).toHaveValue('/web/data'));
  });

  it('ignores blank submission and reports setup failures with a fallback message', async () => {
    apiMock.systemSetup.mockRejectedValue({});
    render(<SetupView onComplete={vi.fn()} />);
    const input = screen.getByLabelText(/Database folder/i);
    fireEvent.change(input, { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: /Set up BoogieBox/i }));
    expect(apiMock.systemSetup).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: '/broken' } });
    fireEvent.click(screen.getByRole('button', { name: /Set up BoogieBox/i }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Setup failed');
  });
});
