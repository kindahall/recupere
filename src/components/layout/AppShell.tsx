import { Shield } from 'lucide-react';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet } from 'react-router-dom';
import { fetchRuntimeCapabilities, fetchScanHistory, getLicenseStatus } from '../../hooks/useIpc';
import { useAppStore } from '../../stores/appStore';
import { ToastContainer } from '../common/ToastContainer';
import { RecoveryStepIndicator } from './RecoveryStepIndicator';
import { SidebarNav } from './SidebarNav';

export function AppShell() {
  const { t } = useTranslation();
  const {
    userMode,
    setRuntimeCapabilities,
    setRuntimeCapabilitiesLoading,
    setScanHistory,
    setLicenseStatus,
  } = useAppStore();

  useEffect(() => {
    let cancelled = false;

    const loadCapabilities = async () => {
      setRuntimeCapabilitiesLoading(true);
      try {
        const [capabilities, history, license] = await Promise.all([
          fetchRuntimeCapabilities(),
          fetchScanHistory().catch(() => []),
          getLicenseStatus().catch(() => null),
        ]);
        if (!cancelled) {
          setRuntimeCapabilities(capabilities);
          setScanHistory(history);
          if (license) {
            setLicenseStatus(license.valid ? 'pro' : 'free');
          }
        }
      } catch {
        if (!cancelled) {
          setRuntimeCapabilities(null);
          setScanHistory([]);
        }
      } finally {
        if (!cancelled) {
          setRuntimeCapabilitiesLoading(false);
        }
      }
    };

    loadCapabilities();

    return () => {
      cancelled = true;
    };
  }, [setRuntimeCapabilities, setRuntimeCapabilitiesLoading, setScanHistory, setLicenseStatus]);

  return (
    <div className="app-shell">
      <SidebarNav />
      <div className="app-main">
        <header className="app-header">
          <div className="flex items-center gap-2" style={{ flex: 1 }}>
            <Shield size={16} className="text-accent" />
            <span className="text-sm font-medium text-secondary">
              {t('safety.banner_read_only')}
            </span>
          </div>
          <span className="badge-device">
            {userMode === 'expert' ? t('mode.expert') : t('mode.novice')}
          </span>
        </header>
        <main className="app-content">
          <RecoveryStepIndicator />
          <div className="page-enter">
            <Outlet />
          </div>
        </main>
      </div>
      <ToastContainer />
    </div>
  );
}
