import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import UserManagement from './UserManagement';

const { usersApi } = vi.hoisted(() => ({
  usersApi: {
    list: vi.fn(),
    create: vi.fn(),
    remove: vi.fn(),
    setPermissions: vi.fn(),
    setPin: vi.fn(),
  },
}));

vi.mock('../api', () => ({
  api: { admin: { users: usersApi } },
}));

const currentUser = {
  id: 'admin-1',
  username: 'owner',
  role: 'admin',
} as any;

const admin = {
  id: 'admin-1',
  username: 'owner',
  role: 'admin',
  hasPin: true,
  canScan: true,
  canEditMetadata: true,
};

const listener = {
  id: 'user-2',
  username: 'listener',
  role: 'user',
  hasPin: false,
  canScan: false,
  canEditMetadata: true,
};

describe('UserManagement', () => {
  beforeEach(() => {
    Object.values(usersApi).forEach(mock => mock.mockReset());
    usersApi.list.mockResolvedValue([admin, listener]);
    usersApi.create.mockResolvedValue({});
    usersApi.remove.mockResolvedValue({});
    usersApi.setPermissions.mockResolvedValue({});
    usersApi.setPin.mockResolvedValue({});
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    vi.spyOn(window, 'alert').mockImplementation(() => {});
  });

  it('loads users, validates input, and creates a permission-scoped user', async () => {
    render(<UserManagement currentUser={currentUser} />);

    expect(screen.getByText('Loading...')).toBeInTheDocument();
    expect(await screen.findByText('listener')).toBeInTheDocument();
    expect(screen.getByText('(you)')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Delete' })[0]).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Add User' }));
    expect(screen.getByText('Username required')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Username'), { target: { value: '  guest  ' } });
    const pinInput = screen.getByPlaceholderText('PIN (optional, 4 digits)');
    fireEvent.change(pinInput, { target: { value: '12x' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add User' }));
    expect(screen.getByText('PIN must be exactly 4 digits')).toBeInTheDocument();

    fireEvent.change(pinInput, { target: { value: '1234' } });
    fireEvent.click(screen.getByLabelText('Allow library scan'));
    fireEvent.click(screen.getByLabelText('Allow metadata editing'));
    fireEvent.click(screen.getByRole('button', { name: 'Add User' }));

    await waitFor(() => expect(usersApi.create).toHaveBeenCalledWith({
      username: 'guest',
      role: 'user',
      pin: '1234',
      canScan: true,
      canEditMetadata: true,
    }));
    expect(usersApi.list).toHaveBeenCalledTimes(2);
  });

  it('supports admin creation and reports create failures', async () => {
    usersApi.create.mockRejectedValueOnce(new Error('duplicate user'));
    render(<UserManagement currentUser={currentUser} />);
    await screen.findByText('listener');

    fireEvent.change(screen.getByPlaceholderText('Username'), { target: { value: 'another' } });
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'admin' } });
    expect(screen.queryByLabelText('Allow library scan')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Add User' }));

    expect(await screen.findByText('duplicate user')).toBeInTheDocument();
    expect(usersApi.create).toHaveBeenCalledWith({
      username: 'another',
      role: 'admin',
      pin: undefined,
      canScan: undefined,
      canEditMetadata: undefined,
    });
  });

  it('toggles permissions and handles deletion confirmation and failures', async () => {
    render(<UserManagement currentUser={currentUser} />);
    const listenerName = await screen.findByText('listener');
    const row = listenerName.closest('div[style*="align-items: center"]') as HTMLElement;

    fireEvent.click(within(row).getByTitle('Toggle scan permission'));
    await waitFor(() => expect(usersApi.setPermissions).toHaveBeenCalledWith('user-2', {
      canScan: true,
      canEditMetadata: true,
    }));

    usersApi.setPermissions.mockRejectedValueOnce(new Error('permission failed'));
    fireEvent.click(within(row).getByTitle('Toggle metadata edit permission'));
    await waitFor(() => expect(window.alert).toHaveBeenCalledWith('permission failed'));

    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(within(row).getByRole('button', { name: 'Delete' }));
    expect(usersApi.remove).not.toHaveBeenCalled();

    usersApi.remove.mockRejectedValueOnce(new Error('delete failed'));
    fireEvent.click(within(row).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(window.alert).toHaveBeenCalledWith('delete failed'));
  });

  it('validates, saves, clears, and cancels PIN changes', async () => {
    render(<UserManagement currentUser={currentUser} />);
    await screen.findByText('listener');
    fireEvent.click(screen.getByRole('button', { name: 'Set PIN' }));

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('Enter a 4-digit PIN')).toBeInTheDocument();

    ['1', '2', '3', '4', '1', '2', '3', '5'].forEach((digit, index) => {
      const input = document.querySelectorAll<HTMLInputElement>('input[type="password"][maxlength="1"]')[index];
      fireEvent.change(input, { target: { value: digit } });
    });
    const currentInputs = document.querySelectorAll<HTMLInputElement>('input[type="password"][maxlength="1"]');
    fireEvent.keyDown(currentInputs[1], { key: 'Backspace' });
    fireEvent.focus(currentInputs[0]);
    fireEvent.blur(currentInputs[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('PINs do not match')).toBeInTheDocument();

    fireEvent.change(
      document.querySelectorAll<HTMLInputElement>('input[type="password"][maxlength="1"]')[7],
      { target: { value: '4' } },
    );
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(usersApi.setPin).toHaveBeenCalledWith('user-2', '1234'));

    fireEvent.click(screen.getByRole('button', { name: 'Set PIN' }));
    fireEvent.click(screen.getByRole('button', { name: 'Clear PIN' }));
    await waitFor(() => expect(usersApi.setPin).toHaveBeenCalledWith('user-2', null));

    fireEvent.click(screen.getByRole('button', { name: 'Set PIN' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByText('Set PIN for listener')).not.toBeInTheDocument();
  });

  it('shows PIN API errors without closing the modal', async () => {
    usersApi.setPin.mockRejectedValueOnce(new Error('PIN service unavailable'));
    render(<UserManagement currentUser={currentUser} />);
    await screen.findByText('listener');
    fireEvent.click(screen.getByRole('button', { name: 'Set PIN' }));
    fireEvent.click(screen.getByRole('button', { name: 'Clear PIN' }));

    expect(await screen.findByText('PIN service unavailable')).toBeInTheDocument();
    expect(screen.getByText('Set PIN for listener')).toBeInTheDocument();
  });
});
