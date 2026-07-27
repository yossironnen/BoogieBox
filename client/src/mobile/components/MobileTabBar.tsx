/**
 * Defines mobile Mobile Tab Bar behavior for the BoogieBox React client.
 */

import { hybridMobileShellStyles } from '../../hybridPreview';
import type { MobileTabId } from '../mobileShell';

const TABS: Array<{ id: MobileTabId; label: string; icon: string }> = [
  { id: 'home', label: 'Home', icon: '⌂' },
  { id: 'browse', label: 'Browse', icon: '♪' },
  { id: 'search', label: 'Search', icon: '⌕' },
  { id: 'playlists', label: 'Playlists', icon: '☰' },
  { id: 'now-playing', label: 'Now', icon: '▶' },
];

/** Mobile Tab Bar is part of this module's public API. */
export default function MobileTabBar({
  activeTab,
  onChange,
}: {
  activeTab: MobileTabId;
  onChange: (tab: MobileTabId) => void;
}) {
  return (
    <nav style={hybridMobileShellStyles.tabBar} aria-label="Mobile tabs">
      {TABS.map((tab) => {
        const active = tab.id === activeTab;
        return (
          <button
            key={tab.id}
            type="button"
            aria-current={active ? 'page' : undefined}
            style={{
              ...hybridMobileShellStyles.tab,
              ...(active ? hybridMobileShellStyles.tabActive : {}),
            }}
            onClick={() => onChange(tab.id)}
          >
            <span
              aria-hidden="true"
              style={{
                ...hybridMobileShellStyles.tabIcon,
                ...(active ? hybridMobileShellStyles.tabIconActive : {}),
              }}
            >
              {tab.icon}
            </span>
            <span>{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
