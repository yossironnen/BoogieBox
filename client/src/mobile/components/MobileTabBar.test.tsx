/**
 * Covers Hybrid mobile tab navigation semantics and touch geometry.
 */

import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import MobileTabBar from './MobileTabBar';

it('exposes one current tab, Hybrid emphasis, and touch-safe navigation', () => {
  const onChange = vi.fn();
  render(<MobileTabBar activeTab="home" onChange={onChange} />);

  const navigation = screen.getByRole('navigation', { name: 'Mobile tabs' });
  const home = screen.getByRole('button', { name: 'Home' });
  const search = screen.getByRole('button', { name: 'Search' });

  expect(navigation).toBeInTheDocument();
  expect(home).toHaveAttribute('aria-current', 'page');
  expect(home).toHaveStyle({
    minHeight: '56px',
    color: 'var(--accent)',
    fontSize: '13px',
    fontWeight: '700',
  });
  expect(home.firstElementChild).toHaveStyle({
    width: '38px',
    height: '30px',
    background: 'var(--accent-soft)',
    fontSize: '24px',
  });
  expect(search).not.toHaveAttribute('aria-current');

  fireEvent.click(search);
  expect(onChange).toHaveBeenCalledWith('search');
});
