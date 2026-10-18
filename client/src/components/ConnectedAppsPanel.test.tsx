import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ConnectedAppsPanel, { CopyValue, formatWhen } from './ConnectedAppsPanel';

const { subsonicApi } = vi.hoisted(() => ({
  subsonicApi: { credentials: vi.fn(), setPassword: vi.fn(), clearPassword: vi.fn() },
}));

vi.mock('../api', () => ({ api: { subsonic: subsonicApi } }));

const enabled = {
  enabled: true,
  hasPassword: false,
  updatedAt: null,
  lastUsedAt: null,
  lastClient: null,
  serverUrls: ['http://192.168.1.20:3001'],
};

function fillPassword(value: string, confirm = value) {
  const [password, confirmation] = Array.from(document.querySelectorAll('input[type="password"]')) as HTMLInputElement[];
  fireEvent.change(password, { target: { value } });
  fireEvent.change(confirmation, { target: { value: confirm } });
}

describe('ConnectedAppsPanel', () => {
  beforeEach(() => {
    Object.values(subsonicApi).forEach((mock) => mock.mockReset());
    subsonicApi.setPassword.mockResolvedValue({ ok: true });
    subsonicApi.clearPassword.mockResolvedValue({ ok: true });
  });

  it('stays hidden while the Subsonic API is off or unreachable', async () => {
    subsonicApi.credentials.mockResolvedValue({ ...enabled, enabled: false });
    const { container, unmount } = render(<ConnectedAppsPanel username="alice" />);
    await waitFor(() => expect(subsonicApi.credentials).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
    unmount();

    subsonicApi.credentials.mockRejectedValue(new Error('offline'));
    const second = render(<ConnectedAppsPanel username="alice" />);
    await waitFor(() => expect(subsonicApi.credentials).toHaveBeenCalledTimes(2));
    expect(second.container).toBeEmptyDOMElement();
  });

  it('shows connection details and sets a password once the rules pass', async () => {
    subsonicApi.credentials.mockResolvedValueOnce(enabled).mockResolvedValue({
      ...enabled,
      hasPassword: true,
      updatedAt: '2026-10-18 10:00:00',
      lastUsedAt: '2026-10-18 11:00:00',
      lastClient: 'Symfonium',
    });
    render(<ConnectedAppsPanel username="alice" />);
    const panel = await screen.findByRole('region', { name: 'Connected apps' });
    expect(within(panel).getByText('http://192.168.1.20:3001')).toBeInTheDocument();
    expect(within(panel).getByText('alice')).toBeInTheDocument();
    expect(within(panel).getByText('Not set')).toBeInTheDocument();
    expect(within(panel).getByText('Never')).toBeInTheDocument();

    const save = within(panel).getByRole('button', { name: 'Set password' });
    expect(save).toBeDisabled();
    fillPassword('Groove2026');
    expect(save).toBeDisabled();
    fillPassword('Groove!2026');
    expect(save).toBeEnabled();
    fireEvent.click(save);

    await waitFor(() => expect(subsonicApi.setPassword).toHaveBeenCalledWith('Groove!2026'));
    expect(await within(panel).findByText('Subsonic password saved')).toBeInTheDocument();
    expect(await within(panel).findByText('Symfonium')).toBeInTheDocument();
    expect(within(panel).getByText('Set')).toBeInTheDocument();
    expect(within(panel).getByRole('button', { name: 'Change password' })).toBeInTheDocument();
  });

  it('shows the server error and removes the password after confirmation', async () => {
    subsonicApi.credentials.mockResolvedValue({ ...enabled, hasPassword: true });
    subsonicApi.setPassword.mockRejectedValue(new Error('Password must be different from your PIN'));
    render(<ConnectedAppsPanel username="alice" />);
    const panel = await screen.findByRole('region', { name: 'Connected apps' });
    fillPassword('Groove!2026');
    fireEvent.click(within(panel).getByRole('button', { name: 'Change password' }));
    expect(await within(panel).findByText('Error: Password must be different from your PIN')).toBeInTheDocument();

    fireEvent.click(within(panel).getByRole('button', { name: 'Remove password' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(subsonicApi.clearPassword).toHaveBeenCalled());
    expect(await within(panel).findByText('Subsonic password removed')).toBeInTheDocument();
  });

  it('copies values and formats server timestamps', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    render(<CopyValue icon={null} label="Username" value="alice" />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy username' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('alice'));
    expect(await screen.findByTitle('Copied')).toBeInTheDocument();

    expect(formatWhen(null)).toBeNull();
    expect(formatWhen('not a date')).toBeNull();
    expect(formatWhen('2026-10-18 10:00:00')).toBe(new Date('2026-10-18T10:00:00Z').toLocaleString());
  });
});
