import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MobileConnectedApps from './MobileConnectedApps';

const { subsonicApi } = vi.hoisted(() => ({
  subsonicApi: {
    credentials: vi.fn(), setPassword: vi.fn(), clearPassword: vi.fn(), createApiKey: vi.fn(), revokeApiKey: vi.fn(),
  },
}));

vi.mock('../../api', () => ({ api: { subsonic: subsonicApi } }));

const creds = {
  enabled: true,
  hasPassword: true,
  updatedAt: '2026-10-18 10:00:00',
  lastUsedAt: '2026-10-18 11:00:00',
  lastClient: 'Amperfy',
  hasApiKey: false,
  apiKeyCreatedAt: null,
  serverUrls: ['http://192.168.1.20:3001'],
};

describe('MobileConnectedApps', () => {
  beforeEach(() => {
    Object.values(subsonicApi).forEach((mock) => mock.mockReset());
    subsonicApi.credentials.mockResolvedValue(creds);
    subsonicApi.setPassword.mockResolvedValue({ ok: true });
    subsonicApi.clearPassword.mockResolvedValue({ ok: true });
  });

  it('renders nothing while the API is off', async () => {
    subsonicApi.credentials.mockResolvedValue({ ...creds, enabled: false });
    const { container } = render(<MobileConnectedApps username="alice" />);
    await waitFor(() => expect(subsonicApi.credentials).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it('changes and removes the Subsonic password', async () => {
    render(<MobileConnectedApps username="alice" />);
    const section = await screen.findByRole('region', { name: 'Connected apps' });
    expect(within(section).getByText('http://192.168.1.20:3001')).toBeInTheDocument();
    expect(within(section).getByText(/· Amperfy/)).toBeInTheDocument();

    const change = within(section).getByRole('button', { name: 'Change password' });
    expect(change).toBeDisabled();
    fireEvent.change(within(section).getByLabelText('New Subsonic password'), { target: { value: 'Groove!2026' } });
    fireEvent.change(within(section).getByLabelText('Confirm password'), { target: { value: 'Groove!2026' } });
    fireEvent.click(change);
    await waitFor(() => expect(subsonicApi.setPassword).toHaveBeenCalledWith('Groove!2026'));
    expect(await within(section).findByText('Subsonic password saved.')).toBeInTheDocument();

    fireEvent.click(within(section).getByRole('button', { name: 'Remove password' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(subsonicApi.clearPassword).toHaveBeenCalled());
  });

  it('creates an API key, shows it once and revokes it', async () => {
    subsonicApi.credentials.mockResolvedValueOnce(creds).mockResolvedValue({
      ...creds, hasApiKey: true, apiKeyCreatedAt: '2026-10-22 09:00:00',
    });
    subsonicApi.createApiKey.mockResolvedValue({ apiKey: 'bbk_mobile' });
    subsonicApi.revokeApiKey.mockResolvedValue({ ok: true });
    render(<MobileConnectedApps username="alice" />);
    const section = await screen.findByRole('region', { name: 'Connected apps' });
    expect(within(section).getByText('Not created')).toBeInTheDocument();
    fireEvent.click(within(section).getByRole('button', { name: 'Create API key' }));
    const shown = await within(section).findByRole('status', { name: 'Your API key' });
    expect(within(shown).getByText('bbk_mobile')).toBeInTheDocument();
    fireEvent.click(within(shown).getByRole('button', { name: 'Done' }));
    expect(within(section).queryByText('bbk_mobile')).not.toBeInTheDocument();

    fireEvent.click(await within(section).findByRole('button', { name: 'Revoke API key' }));
    expect(await screen.findByText('Apps using it are signed out.')).toBeInTheDocument();
    const confirms = screen.getAllByRole('button', { name: 'Revoke API key' });
    fireEvent.click(confirms[confirms.length - 1]);
    await waitFor(() => expect(subsonicApi.revokeApiKey).toHaveBeenCalled());
  });

  it('shows a save failure', async () => {
    subsonicApi.setPassword.mockRejectedValue(new Error('Password must be at least 8 characters'));
    render(<MobileConnectedApps username="alice" />);
    const section = await screen.findByRole('region', { name: 'Connected apps' });
    fireEvent.change(within(section).getByLabelText('New Subsonic password'), { target: { value: 'Groove!2026' } });
    fireEvent.change(within(section).getByLabelText('Confirm password'), { target: { value: 'Groove!2026' } });
    fireEvent.click(within(section).getByRole('button', { name: 'Change password' }));
    expect(await within(section).findByRole('alert')).toHaveTextContent('Password must be at least 8 characters');
  });
});
