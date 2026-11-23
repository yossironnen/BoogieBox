import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ScrobblingPanel from './ScrobblingPanel';
import MobileScrobbling from '../mobile/components/MobileScrobbling';

const { scrobbleApi } = vi.hoisted(() => ({
  scrobbleApi: {
    accounts: vi.fn(),
    connectListenBrainz: vi.fn(),
    connectLastFm: vi.fn(),
    connectLibreFm: vi.fn(),
    connectServer: vi.fn(),
    disconnect: vi.fn(),
  },
}));
vi.mock('../api', () => ({ api: { scrobble: scrobbleApi } }));

const lbAccount = {
  service: 'listenbrainz' as const,
  accountName: 'lb_alice',
  needsReconnect: false,
  connectedAt: '2026-10-19 08:00:00',
  lastScrobbleAt: '2026-10-19 09:00:00',
  pending: 3,
};
const token = '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0';

describe('ScrobblingPanel', () => {
  beforeEach(() => {
    Object.values(scrobbleApi).forEach((m) => m.mockReset());
    scrobbleApi.disconnect.mockResolvedValue({ ok: true });
  });
  afterEach(() => { vi.useRealTimers(); });

  it('asks for an admin when Last.fm is not set up and leaves ListenBrainz to Integrations', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [lbAccount] });
    render(<ScrobblingPanel />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).getByText(/An admin needs to add the Last.fm API key/)).toBeInTheDocument();
    // Last.fm, Libre.fm and the own server.
    expect(within(panel).getAllByText('Not connected')).toHaveLength(3);
    expect(within(panel).getByText(/ListenBrainz is under Integrations/)).toBeInTheDocument();
    expect(within(panel).queryByLabelText('ListenBrainz user token')).not.toBeInTheDocument();
    expect(within(panel).queryByText('lb_alice')).not.toBeInTheDocument();
  });

  it('links admins to the Last.fm setup when it is missing', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [] });
    const onOpenLastFmSetup = vi.fn();
    render(<ScrobblingPanel onOpenLastFmSetup={onOpenLastFmSetup} />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).queryByText(/An admin needs to/)).not.toBeInTheDocument();
    fireEvent.click(within(panel).getByRole('button', { name: 'Open Last.fm setup' }));
    expect(onOpenLastFmSetup).toHaveBeenCalledTimes(1);
  });

  it('says when Last.fm refuses the server key', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      lastfmKeyRefused: true,
      accounts: [{ ...lbAccount, service: 'lastfm', accountName: 'fm_alice' }],
    });
    render(<ScrobblingPanel />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).getByText("Last.fm isn't accepting scrobbles from this server.")).toBeInTheDocument();
  });

  it('says nothing about the key without a Last.fm account', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: true, lastfmKeyRefused: true, accounts: [] });
    render(<ScrobblingPanel />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).queryByText(/isn't accepting scrobbles/)).not.toBeInTheDocument();
  });

  it('disconnects Last.fm after confirming', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      accounts: [{ ...lbAccount, service: 'lastfm', accountName: 'fm_alice' }],
    });
    render(<ScrobblingPanel />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).getByText('fm_alice')).toBeInTheDocument();
    expect(within(panel).getByText('3')).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole('button', { name: 'Disconnect Last.fm' }));
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(scrobbleApi.disconnect).toHaveBeenCalledWith('lastfm'));
  });

  it('opens the Last.fm sign-in and waits for the account', async () => {
    vi.useFakeTimers();
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    scrobbleApi.accounts
      .mockResolvedValueOnce({ lastfmAvailable: true, accounts: [] })
      .mockResolvedValueOnce({ lastfmAvailable: true, accounts: [] })
      .mockResolvedValue({
        lastfmAvailable: true,
        accounts: [{ ...lbAccount, service: 'lastfm', accountName: 'fm_alice', connectedAt: '2026-10-19 10:00:00' }],
      });
    scrobbleApi.connectLastFm.mockResolvedValue({ url: 'https://www.last.fm/api/auth/?api_key=k' });
    render(<ScrobblingPanel />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    fireEvent.click(screen.getByRole('button', { name: 'Connect Last.fm' }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(scrobbleApi.connectLastFm).toHaveBeenCalledWith(window.location.origin);
    expect(open).toHaveBeenCalledWith('https://www.last.fm/api/auth/?api_key=k', '_blank', 'noopener,noreferrer');
    expect(screen.getByText(/Finish signing in to Last.fm/)).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    expect(screen.getByText('Connected to Last.fm as fm_alice')).toBeInTheDocument();
    expect(screen.queryByText(/Finish signing in/)).not.toBeInTheDocument();
    open.mockRestore();
  });

  it('flags accounts that need reconnecting and reports sign-in errors', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      accounts: [{ ...lbAccount, service: 'lastfm', needsReconnect: true }, { ...lbAccount, needsReconnect: true }],
    });
    scrobbleApi.connectLastFm.mockRejectedValue(new Error('Last.fm is not set up on this server'));
    render(<ScrobblingPanel />);
    expect(await screen.findByText(/Last.fm stopped accepting this account/)).toBeInTheDocument();
    expect(screen.queryByText(/ListenBrainz stopped accepting this token/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect Last.fm' }));
    expect(await screen.findByText(/not set up on this server/)).toBeInTheDocument();
  });
});

describe('MobileScrobbling', () => {
  beforeEach(() => {
    Object.values(scrobbleApi).forEach((m) => m.mockReset());
  });

  it('connects ListenBrainz and disconnects Last.fm', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      accounts: [{ ...lbAccount, service: 'lastfm', accountName: 'fm_alice' }],
    });
    scrobbleApi.connectListenBrainz.mockRejectedValue(new Error('The service did not accept this account'));
    scrobbleApi.disconnect.mockResolvedValue({ ok: true });
    render(<MobileScrobbling />);
    expect(await screen.findByText(/fm_alice/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('ListenBrainz user token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect ListenBrainz' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('did not accept');
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect Last.fm' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(scrobbleApi.disconnect).toHaveBeenCalledWith('lastfm'));
  });

  it('explains when Last.fm is not set up', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [lbAccount] });
    render(<MobileScrobbling />);
    expect(await screen.findByText(/An admin needs to add the Last.fm API key/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disconnect ListenBrainz' })).toBeInTheDocument();
  });

  it('shows reconnect states, waits for Last.fm and reports a success', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      accounts: [{ ...lbAccount, service: 'lastfm', needsReconnect: true }, { ...lbAccount, needsReconnect: true }],
    });
    scrobbleApi.connectLastFm.mockResolvedValue({ url: 'https://www.last.fm/api/auth/' });
    scrobbleApi.connectListenBrainz.mockResolvedValue({ ok: true, accountName: '' });
    render(<MobileScrobbling />);
    expect(await screen.findByText(/Last.fm stopped accepting this account/)).toBeInTheDocument();
    expect(screen.getByText(/ListenBrainz stopped accepting this token/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect Last.fm' }));
    expect(await screen.findByText(/Finish signing in to Last.fm/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('ListenBrainz user token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect ListenBrainz' }));
    expect(await screen.findByText('Connected to ListenBrainz')).toBeInTheDocument();
    open.mockRestore();
  });

  it('renders nothing until loaded and survives a failed load', async () => {
    scrobbleApi.accounts.mockRejectedValue(new Error('down'));
    render(<MobileScrobbling />);
    expect(await screen.findByText('Scrobbling')).toBeInTheDocument();
    expect(screen.getAllByText('Not connected')).toHaveLength(4);
  });
});

describe('ScrobblingPanel edge cases', () => {
  beforeEach(() => {
    Object.values(scrobbleApi).forEach((m) => m.mockReset());
  });
  afterEach(() => { vi.useRealTimers(); });

  it('stops waiting for Last.fm after the time limit and can be cancelled', async () => {
    vi.useFakeTimers();
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: true, accounts: [] });
    scrobbleApi.connectLastFm.mockResolvedValue({ url: 'https://www.last.fm/api/auth/' });
    render(<ScrobblingPanel />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    fireEvent.click(screen.getByRole('button', { name: 'Connect Last.fm' }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText(/Finish signing in/)).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60 * 1000 + 4000); });
    expect(screen.queryByText(/Finish signing in/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Connect Last.fm' }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByText(/Finish signing in/)).not.toBeInTheDocument();
    open.mockRestore();
  });

  it('reports non-Error failures with fallback messages', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: true, accounts: [{ ...lbAccount, service: 'lastfm', needsReconnect: true }] });
    scrobbleApi.connectLastFm.mockRejectedValue('nope');
    scrobbleApi.disconnect.mockRejectedValue('nope');
    render(<ScrobblingPanel />);
    fireEvent.click(await screen.findByRole('button', { name: 'Reconnect Last.fm' }));
    expect(await screen.findByText(/Could not start the Last.fm sign-in/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect Last.fm' }));
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect Last.fm' }));
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Disconnect' }));
    expect(await screen.findByText(/Could not disconnect/)).toBeInTheDocument();
  });

  it('reports a ListenBrainz rejection', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [] });
    scrobbleApi.connectListenBrainz.mockRejectedValue('nope');
    render(<MobileScrobbling />);
    fireEvent.change(await screen.findByLabelText('ListenBrainz user token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect ListenBrainz' }));
    expect(await screen.findByText(/Could not connect/)).toBeInTheDocument();
  });
});

describe('Libre.fm and own server', () => {
  const serverAddress = 'http://maloja.lan:42010/apis/listenbrainz';
  beforeEach(() => {
    Object.values(scrobbleApi).forEach((m) => m.mockReset());
    scrobbleApi.disconnect.mockResolvedValue({ ok: true });
  });
  afterEach(() => { vi.useRealTimers(); });

  it('signs in to Libre.fm with no admin setup and waits for the account', async () => {
    vi.useFakeTimers();
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    scrobbleApi.accounts
      .mockResolvedValueOnce({ lastfmAvailable: false, accounts: [] })
      .mockResolvedValueOnce({ lastfmAvailable: false, accounts: [] })
      .mockResolvedValue({
        lastfmAvailable: false,
        accounts: [{ ...lbAccount, service: 'librefm', accountName: 'lf_alice', connectedAt: '2026-11-22 10:00:00' }],
      });
    scrobbleApi.connectLibreFm.mockResolvedValue({ url: 'https://libre.fm/api/auth/?api_key=k' });
    render(<ScrobblingPanel />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText(/An admin needs to add the Last.fm API key/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Connect Libre.fm' }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(scrobbleApi.connectLibreFm).toHaveBeenCalledWith(window.location.origin);
    expect(scrobbleApi.connectLastFm).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledWith('https://libre.fm/api/auth/?api_key=k', '_blank', 'noopener,noreferrer');
    expect(screen.getByText(/Finish signing in to Libre.fm in the window that opened/)).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    expect(screen.getByText('Connected to Libre.fm as lf_alice')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disconnect Libre.fm' })).toBeInTheDocument();
    open.mockRestore();
  });

  it('warns when Libre.fm stops accepting the account', async () => {
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [{ ...lbAccount, service: 'librefm', needsReconnect: true }] });
    render(<ScrobblingPanel />);
    expect(await screen.findByText(/Libre.fm stopped accepting this account. Reconnect to keep scrobbling./)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Reconnect Libre.fm' })).toBeInTheDocument();
  });

  it('connects an own server, shows its address and disconnects it', async () => {
    scrobbleApi.accounts
      .mockResolvedValueOnce({ lastfmAvailable: true, accounts: [] })
      .mockResolvedValue({
        lastfmAvailable: true,
        accounts: [{ ...lbAccount, service: 'server', accountName: null, address: serverAddress }],
      });
    scrobbleApi.connectServer.mockResolvedValue({ ok: true, accountName: '', address: serverAddress });
    render(<ScrobblingPanel />);
    const panel = await screen.findByRole('region', { name: 'Scrobbling' });
    expect(within(panel).getByText(/Any server that takes ListenBrainz scrobbles, such as Maloja or Koito/)).toBeInTheDocument();
    const connect = within(panel).getByRole('button', { name: 'Connect server' });
    expect(connect).toBeDisabled();
    fireEvent.change(within(panel).getByLabelText('Server address'), { target: { value: 'maloja.lan:42010' } });
    fireEvent.change(within(panel).getByLabelText('Token'), { target: { value: 'maloja-key' } });
    fireEvent.click(connect);
    expect(await within(panel).findByText(/Enter the server address, starting with http:\/\/ or https:\/\//)).toBeInTheDocument();
    expect(scrobbleApi.connectServer).not.toHaveBeenCalled();

    fireEvent.change(within(panel).getByLabelText('Server address'), { target: { value: ` ${serverAddress}/1 ` } });
    fireEvent.click(connect);
    await waitFor(() => expect(scrobbleApi.connectServer).toHaveBeenCalledWith(`${serverAddress}/1`, 'maloja-key'));
    expect(await within(panel).findByText(`Connected to ${serverAddress}`)).toBeInTheDocument();
    expect(within(panel).getByText(serverAddress)).toBeInTheDocument();
    expect(within(panel).queryByLabelText('Server address')).not.toBeInTheDocument();

    fireEvent.click(within(panel).getByRole('button', { name: 'Disconnect your server' }));
    const dialog = screen.getByRole('alertdialog');
    expect(within(dialog).getByText('Disconnect your server?')).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(scrobbleApi.disconnect).toHaveBeenCalledWith('server'));
    expect(await within(panel).findByText('Disconnected from your server')).toBeInTheDocument();
  });

  it('names the server account and asks for a new token when it is refused', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: true,
      accounts: [{ ...lbAccount, service: 'server', accountName: 'mal', address: serverAddress, needsReconnect: true }],
    });
    scrobbleApi.connectServer.mockResolvedValue({ ok: true, accountName: 'mal', address: serverAddress });
    render(<ScrobblingPanel />);
    expect(await screen.findByText('Your server stopped accepting this token. Enter a new one.')).toBeInTheDocument();
    expect(screen.getByText('mal')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Server address'), { target: { value: serverAddress } });
    fireEvent.change(screen.getByLabelText('Token'), { target: { value: 'new-key' } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect server' }));
    expect(await screen.findByText(`Connected to ${serverAddress} as mal`)).toBeInTheDocument();
  });

  it('connects Libre.fm and an own server on mobile', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    scrobbleApi.accounts.mockResolvedValue({ lastfmAvailable: false, accounts: [] });
    scrobbleApi.connectLibreFm.mockResolvedValue({ url: 'https://libre.fm/api/auth/' });
    scrobbleApi.connectServer.mockRejectedValue(new Error('The service could not be reached. Try again later'));
    render(<MobileScrobbling />);
    fireEvent.click(await screen.findByRole('button', { name: 'Connect Libre.fm' }));
    expect(await screen.findByText(/Finish signing in to Libre.fm/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Server address'), { target: { value: serverAddress } });
    fireEvent.change(screen.getByLabelText('Token'), { target: { value: 'maloja-key' } });
    fireEvent.click(screen.getByRole('button', { name: 'Connect server' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be reached');
    open.mockRestore();
  });

  it('disconnects the own server on mobile', async () => {
    scrobbleApi.accounts.mockResolvedValue({
      lastfmAvailable: false,
      accounts: [{ ...lbAccount, service: 'server', accountName: null, address: serverAddress }],
    });
    render(<MobileScrobbling />);
    expect(await screen.findByText(new RegExp(serverAddress))).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect your server' }));
    expect(await screen.findByText('Disconnect your server?')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }));
    await waitFor(() => expect(scrobbleApi.disconnect).toHaveBeenCalledWith('server'));
  });
});
