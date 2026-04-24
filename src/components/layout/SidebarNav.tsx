import {
  ChevronLeft,
  ChevronRight,
  Clock,
  Download,
  FolderSearch,
  HardDrive,
  History,
  Home,
  Scan,
  Settings,
  Shield,
  Stethoscope,
  Terminal,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { useAppStore } from '../../stores/appStore';

const navItems = [
  { path: '/', icon: Home, labelKey: 'nav.home' },
  { path: '/devices', icon: HardDrive, labelKey: 'nav.devices' },
  { path: '/diagnostic', icon: Stethoscope, labelKey: 'nav.diagnostic' },
  { path: '/scan', icon: Scan, labelKey: 'nav.scan' },
  { path: '/results', icon: FolderSearch, labelKey: 'nav.results' },
  { path: '/export', icon: Download, labelKey: 'nav.export' },
  { path: '/history', icon: History, labelKey: 'nav.history' },
  { path: '/missing-files', icon: Clock, labelKey: 'nav.missing_files' },
  { path: '/expert', icon: Terminal, labelKey: 'nav.expert', expertOnly: true },
  { path: '/settings', icon: Settings, labelKey: 'nav.settings' },
];

export function SidebarNav() {
  const { t } = useTranslation();
  const { userMode, setUserMode, sidebarCollapsed, toggleSidebar } = useAppStore();
  const isExpert = userMode === 'expert';

  return (
    <aside className={`app-sidebar${sidebarCollapsed ? ' collapsed' : ''}`}>
      <div className="sidebar-brand">
        <div className="sidebar-brand-icon">
          <Shield size={18} />
        </div>
        {!sidebarCollapsed && <span className="sidebar-brand-text">{t('app.name')}</span>}
      </div>

      <nav className="sidebar-nav">
        {navItems
          .filter((item) => !item.expertOnly || isExpert)
          .map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              data-testid={`nav-${item.path === '/' ? 'home' : item.path.slice(1)}`}
              className={({ isActive }) => `sidebar-nav-item${isActive ? ' active' : ''}`}
              end={item.path === '/'}
            >
              <item.icon size={20} className="nav-icon" />
              {!sidebarCollapsed && <span className="sidebar-nav-label">{t(item.labelKey)}</span>}
            </NavLink>
          ))}
      </nav>

      <div className="sidebar-footer">
        <button
          type="button"
          className="mode-toggle"
          onClick={() => setUserMode(isExpert ? 'novice' : 'expert')}
          title={isExpert ? t('mode.novice') : t('mode.expert')}
          data-testid="sidebar-mode-toggle"
        >
          <div className={`mode-toggle-track${isExpert ? ' active' : ''}`}>
            <div className="mode-toggle-thumb" />
          </div>
          {!sidebarCollapsed && (
            <span className="mode-toggle-label">
              {isExpert ? t('mode.expert') : t('mode.novice')}
            </span>
          )}
        </button>

        <button
          type="button"
          className="sidebar-nav-item mt-2"
          onClick={toggleSidebar}
          style={{ justifyContent: sidebarCollapsed ? 'center' : undefined }}
          data-testid="sidebar-collapse-toggle"
        >
          {sidebarCollapsed ? (
            <ChevronRight size={20} className="nav-icon" />
          ) : (
            <ChevronLeft size={20} className="nav-icon" />
          )}
          {!sidebarCollapsed && <span className="sidebar-nav-label">{t('common.collapse')}</span>}
        </button>
      </div>
    </aside>
  );
}
