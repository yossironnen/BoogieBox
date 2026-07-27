import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { BUILTIN_PARAMETRIC_PRESETS, DEFAULT_PARAMETRIC_BANDS } from '../audio/eq';
import ParametricEqEditor from './ParametricEqEditor';

describe('ParametricEqEditor', () => {
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  });

  it('selects profiles and edits all band controls with clamping', () => {
    const onBandsChange = vi.fn();
    const onProfileChange = vi.fn();
    const custom = [{ name: 'My Curve', bands: DEFAULT_PARAMETRIC_BANDS }];
    render(
      <ParametricEqEditor
        bands={DEFAULT_PARAMETRIC_BANDS}
        profile="Manual"
        customProfiles={custom}
        autoEqEnabled={false}
        newProfileName=""
        accentColor="#fff"
        onBandsChange={onBandsChange}
        onProfileChange={onProfileChange}
        onNewProfileNameChange={vi.fn()}
        onSaveProfile={vi.fn().mockResolvedValue(null)}
        onDeleteProfile={vi.fn()}
      />,
    );
    const profile = screen.getByRole('combobox', { name: 'EQ profile' });
    fireEvent.change(profile, { target: { value: 'Warm' } });
    expect(onProfileChange).toHaveBeenCalledWith('Warm', BUILTIN_PARAMETRIC_PRESETS.Warm);
    fireEvent.change(profile, { target: { value: 'My Curve' } });
    expect(onProfileChange).toHaveBeenCalledWith('My Curve', DEFAULT_PARAMETRIC_BANDS);

    fireEvent.click(screen.getByRole('button', { name: /Band 7/ }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Band 7 type' }), { target: { value: 'lowpass' } });
    fireEvent.click(screen.getByLabelText('Enabled'));
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Band 7 frequency' }), { target: { value: '99999' } });
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Band 7 Q' }), { target: { value: '0' } });
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Band 7 gain value' }), { target: { value: '-99' } });
    expect(onBandsChange).toHaveBeenCalled();
    expect(screen.getByRole('slider', { name: 'Band 7 gain' })).toBeEnabled();
  });

  it('reports save errors and success, deletes custom profiles, and disables Auto EQ editing', async () => {
    const save = vi.fn()
      .mockResolvedValueOnce('Name already exists.')
      .mockResolvedValueOnce(null);
    const remove = vi.fn().mockResolvedValue(undefined);
    const props = {
      bands: DEFAULT_PARAMETRIC_BANDS,
      profile: 'My Curve',
      customProfiles: [{ name: 'My Curve', bands: DEFAULT_PARAMETRIC_BANDS }],
      autoEqEnabled: false,
      newProfileName: ' My Curve ',
      accentColor: '#fff',
      onBandsChange: vi.fn(),
      onProfileChange: vi.fn(),
      onNewProfileNameChange: vi.fn(),
      onSaveProfile: save,
      onDeleteProfile: remove,
    };
    const { rerender } = render(<ParametricEqEditor {...props} />);
    fireEvent.change(screen.getByPlaceholderText('Save as...'), { target: { value: 'next' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Name already exists.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Profile saved.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith('My Curve'));
    expect(screen.getByText('Profile deleted.')).toBeInTheDocument();

    rerender(<ParametricEqEditor {...props} profile="Manual" autoEqEnabled />);
    expect(screen.getByPlaceholderText('Save as...')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Delete' })).toBeDisabled();
    expect(screen.getByRole('slider', { name: 'Band 1 gain' })).toBeDisabled();
  });
});
