import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import RadioStationsView from './RadioStationsView';

const { radioApi } = vi.hoisted(() => ({
  radioApi: { list: vi.fn(), create: vi.fn(), update: vi.fn(), remove: vi.fn(), streamUrl: vi.fn() },
}));
vi.mock('../api', () => ({ api: { radioStations: radioApi } }));

const jazz = { id: 's1', name: 'Jazz FM', streamUrl: 'http://jazz.example/live', homepageUrl: 'https://jazz.example' };

describe('RadioStationsView', () => {
  beforeEach(() => {
    Object.values(radioApi).forEach((m) => m.mockReset());
    radioApi.list.mockResolvedValue([jazz]);
    radioApi.create.mockResolvedValue(jazz);
    radioApi.update.mockResolvedValue(jazz);
    radioApi.remove.mockResolvedValue({ ok: true });
  });

  it('lists and plays stations; users get no edit controls', async () => {
    const onPlayStation = vi.fn();
    render(<RadioStationsView isAdmin={false} onPlayStation={onPlayStation} playingStationId="s1" />);
    expect(await screen.findByText('Jazz FM')).toBeInTheDocument();
    expect(screen.getByText('1 station')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Website/ })).toHaveAttribute('href', 'https://jazz.example');
    fireEvent.click(screen.getByRole('button', { name: 'Play Jazz FM' }));
    expect(onPlayStation).toHaveBeenCalledWith(jazz);
    expect(screen.queryByRole('button', { name: /Add station/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Edit Jazz FM' })).not.toBeInTheDocument();
  });

  it('lets admins add, edit and delete stations with validation', async () => {
    render(<RadioStationsView isAdmin onPlayStation={vi.fn()} />);
    await screen.findByText('Jazz FM');

    fireEvent.click(screen.getByRole('button', { name: /Add station/ }));
    fireEvent.change(screen.getByPlaceholderText('Jazz FM'), { target: { value: 'Rock' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Stream URL');
    fireEvent.change(screen.getAllByPlaceholderText('https://…')[0], { target: { value: ' https://rock.example/s ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(radioApi.create).toHaveBeenCalledWith({ name: 'Rock', streamUrl: 'https://rock.example/s', homepageUrl: '' }));
    await waitFor(() => expect(radioApi.list).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole('button', { name: 'Edit Jazz FM' }));
    expect(screen.getByDisplayValue('http://jazz.example/live')).toBeInTheDocument();
    fireEvent.change(screen.getByDisplayValue('Jazz FM'), { target: { value: 'Jazz 24' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(radioApi.update).toHaveBeenCalledWith('s1', expect.objectContaining({ name: 'Jazz 24' })));

    fireEvent.click(screen.getByRole('button', { name: 'Delete Jazz FM' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(radioApi.remove).toHaveBeenCalledWith('s1'));
  });

  it('reports server errors and empty lists', async () => {
    radioApi.list.mockResolvedValueOnce([]);
    const { unmount } = render(<RadioStationsView isAdmin onPlayStation={vi.fn()} />);
    expect(await screen.findByText(/No radio stations yet/)).toBeInTheDocument();
    radioApi.create.mockRejectedValueOnce(new Error('Stream URL must be an http:// or https:// address'));
    fireEvent.click(screen.getByRole('button', { name: /Add station/ }));
    fireEvent.change(screen.getByPlaceholderText('Jazz FM'), { target: { value: 'X' } });
    fireEvent.change(screen.getAllByPlaceholderText('https://…')[0], { target: { value: 'http://x.example' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('http://');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    unmount();

    radioApi.list.mockRejectedValueOnce(new Error('offline'));
    render(<RadioStationsView isAdmin={false} onPlayStation={vi.fn()} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('offline');
  });
});

describe('RadioStationsView failures', () => {
  beforeEach(() => {
    Object.values(radioApi).forEach((m) => m.mockReset());
    radioApi.list.mockResolvedValue([{ ...jazz, homepageUrl: null }]);
  });

  it('reports failed saves and deletes with fallback messages', async () => {
    radioApi.update.mockRejectedValue('nope');
    radioApi.remove.mockRejectedValue('nope');
    render(<RadioStationsView isAdmin onPlayStation={vi.fn()} />);
    await screen.findByText('Jazz FM');
    expect(screen.queryByRole('link', { name: /Website/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Edit Jazz FM' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not save the station');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete Jazz FM' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(await screen.findByText('Could not delete the station')).toBeInTheDocument();
  });

  it('shows a generic message when the list fails without an Error', async () => {
    radioApi.list.mockRejectedValue('nope');
    render(<RadioStationsView isAdmin={false} onPlayStation={vi.fn()} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load radio stations');
  });
});
