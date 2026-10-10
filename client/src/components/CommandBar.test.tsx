/**
 * Tests the shared browse CommandBar and its compact library stats strip.
 */

import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import CommandBar, { LibraryStatsStrip, buildLibraryStatItems, formatLibraryStatsTitle, getStatsTier } from './CommandBar';
import type { Stats } from '../types';

const stats = {
  total_tracks: 4662,
  total_artists: 3360,
  total_albums: 274,
  total_hours: 369.3,
  total_gb: 170.05,
} as Stats;

describe('getStatsTier', () => {
  it('shows every stat when unmeasured or wide', () => {
    expect(getStatsTier(0)).toBe('full');
    expect(getStatsTier(1180)).toBe('full');
    expect(getStatsTier(1600)).toBe('full');
  });

  it('drops to tracks + albums, then hides, as the bar narrows', () => {
    expect(getStatsTier(1179)).toBe('short');
    expect(getStatsTier(960)).toBe('short');
    expect(getStatsTier(959)).toBe('hidden');
  });
});

describe('buildLibraryStatItems', () => {
  it('formats the five library totals', () => {
    expect(buildLibraryStatItems(stats).map((item) => item.display)).toEqual([
      (4662).toLocaleString(), (3360).toLocaleString(), '274', '369h', `${(170.05).toLocaleString(undefined, { maximumFractionDigits: 1 })} GB`,
    ]);
  });

  it('uses dash placeholders while stats are missing', () => {
    expect(buildLibraryStatItems(null).map((item) => item.display)).toEqual(['–', '–', '–', '–', '–']);
    expect(formatLibraryStatsTitle(null)).toBe('Library totals: – tracks, – artists, – albums, – hours, – GB');
  });
});

describe('LibraryStatsStrip', () => {
  it('renders all five stats in the full tier', () => {
    render(<LibraryStatsStrip stats={stats} tier="full" />);
    for (const label of [`${(4662).toLocaleString()} tracks`, `${(3360).toLocaleString()} artists`, '274 albums', '369 hours']) {
      expect(screen.getByLabelText(label)).toBeInTheDocument();
    }
  });

  it('renders only tracks and albums in the short tier', () => {
    render(<LibraryStatsStrip stats={stats} tier="short" />);
    expect(screen.getByLabelText('274 albums')).toBeInTheDocument();
    expect(screen.queryByLabelText(`${(3360).toLocaleString()} artists`)).not.toBeInTheDocument();
    expect(screen.queryByLabelText('369 hours')).not.toBeInTheDocument();
  });

  it('renders nothing in the hidden tier', () => {
    render(<LibraryStatsStrip stats={stats} tier="hidden" />);
    expect(screen.queryByTestId('library-stats-strip')).not.toBeInTheDocument();
  });
});

describe('CommandBar', () => {
  it('draws a decorative, non-interactive watermark behind the controls', () => {
    render(<CommandBar watermark="Browse" stats={stats}><button type="button">Tab</button></CommandBar>);
    const watermark = screen.getByTestId('command-bar-watermark');
    expect(watermark).toHaveTextContent('Browse');
    expect(watermark.parentElement).toHaveAttribute('aria-hidden', 'true');
    expect(watermark.parentElement).toHaveStyle({ pointerEvents: 'none', overflow: 'hidden' });
    expect(watermark).toHaveStyle({ opacity: '0.12', fontSize: '100px' });
    expect(screen.getByRole('button', { name: 'Tab' })).toBeInTheDocument();
    expect(screen.getByTestId('library-stats-strip')).toBeInTheDocument();
  });

  it('renders the actions zone only when actions are given', () => {
    const { container, rerender } = render(<CommandBar watermark="Search" stats={null}><span>left</span></CommandBar>);
    const bar = container.querySelector('[data-command-bar="search"]') as HTMLElement;
    expect(bar.children).toHaveLength(3);

    rerender(<CommandBar watermark="Search" stats={null} actions={<button type="button">Act</button>}><span>left</span></CommandBar>);
    expect(bar.children).toHaveLength(4);
    expect(screen.getByRole('button', { name: 'Act' })).toBeInTheDocument();
  });

  it('keeps the bar itself overflow-visible so anchored popovers are not clipped', () => {
    const { container } = render(<CommandBar watermark="Browse" stats={null}><span>left</span></CommandBar>);
    const bar = container.querySelector('[data-command-bar="browse"]') as HTMLElement;
    expect(bar.style.overflow).toBe('');
    expect(bar).toHaveStyle({ position: 'relative', flexWrap: 'nowrap' });
    // The bar is its own stacking context; it must sit above the grid/table so
    // the Refine, sort and genre popovers render over the content.
    expect(bar).toHaveStyle({ zIndex: '30' });
  });
});
