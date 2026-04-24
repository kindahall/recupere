import { Clock, Disc, Search, Shield } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { useAppStore } from '../stores/appStore';
import type { RuntimeCapabilityKey } from '../types';

type CapabilityStatus = 'available' | 'limited' | 'pending' | 'unavailable';

const statusStyles: Record<CapabilityStatus, { dot: string; badge: string }> = {
  available: { dot: 'var(--color-success)', badge: 'var(--color-success)' },
  limited: { dot: 'var(--color-warning)', badge: 'var(--color-warning)' },
  pending: { dot: 'var(--color-warning)', badge: 'var(--color-warning)' },
  unavailable: { dot: 'var(--color-danger)', badge: 'var(--color-danger)' },
};

function formatSessionDate(timestampMs: number): string {
  return new Date(timestampMs).toLocaleString();
}

function buildRecentSessionBadge(scanType: string, filesFound: number, t: (key: string) => string) {
  if (scanType === 'image') {
    return t('scan.image');
  }

  if (scanType === 'lost-volume') {
    return t('scan.lost_volume');
  }

  if (scanType === 'carving') {
    return t('scan.deleted');
  }

  if (scanType === 'signature-carving') {
    return t('scan.signature_carving');
  }

  return `${filesFound} ${t('history.files_found').toLowerCase()}`;
}

export function HomePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { scanHistory, runtimeCapabilities, runtimeCapabilitiesLoading } = useAppStore();
  const limitedCapabilities = new Set(runtimeCapabilities?.limitedCapabilities ?? []);
  const capabilityStatus = (
    enabled: boolean | undefined,
    key: RuntimeCapabilityKey,
    fallbackWhenDisabled: CapabilityStatus = 'unavailable',
  ): CapabilityStatus => {
    if (runtimeCapabilitiesLoading) {
      return 'pending';
    }
    if (!enabled) {
      return fallbackWhenDisabled;
    }
    return limitedCapabilities.has(key) ? 'limited' : 'available';
  };

  const capabilities = [
    {
      label: t('home.cap_device_detection'),
      note: null,
      status: capabilityStatus(runtimeCapabilities?.deviceDetection, 'deviceDetection'),
    },
    {
      label: t('home.cap_diagnostic'),
      note: null,
      status: capabilityStatus(runtimeCapabilities?.heuristicDiagnostic, 'heuristicDiagnostic'),
    },
    {
      label: t('home.cap_scan'),
      note: t('home.cap_scan_limited_note'),
      status: capabilityStatus(runtimeCapabilities?.scanEngine, 'scanEngine', 'pending'),
    },
    {
      label: t('home.cap_imaging'),
      note: t('home.cap_imaging_limited_note'),
      status: capabilityStatus(runtimeCapabilities?.imagingEngine, 'imagingEngine', 'pending'),
    },
    {
      label: t('home.cap_export_validation'),
      note: null,
      status: capabilityStatus(runtimeCapabilities?.exportValidation, 'exportValidation'),
    },
    {
      label: t('home.cap_export'),
      note: null,
      status: capabilityStatus(runtimeCapabilities?.exportEngine, 'exportEngine', 'pending'),
    },
    {
      label: t('home.cap_technical_logs'),
      note: t('home.cap_technical_logs_limited_note'),
      status: capabilityStatus(runtimeCapabilities?.technicalLogs, 'technicalLogs', 'pending'),
    },
  ] as const;
  const limitedCapabilityNotes = capabilities
    .filter((capability) => capability.status === 'limited' && capability.note)
    .map((capability) => `${capability.label}: ${capability.note}`);

  return (
    <div>
      <div className="home-hero">
        <div
          style={{
            width: 72,
            height: 72,
            borderRadius: 'var(--radius-2xl)',
            background: 'linear-gradient(135deg, #4A7CFF, #6B95FF)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            marginBottom: 'var(--space-5)',
            boxShadow: '0 8px 32px rgba(74, 124, 255, 0.3)',
          }}
        >
          <Shield size={36} color="white" />
        </div>
        <h1 className="home-hero-title">{t('home.title')}</h1>
        <p className="home-hero-subtitle">{t('home.subtitle')}</p>
        <div className="home-actions">
          <button
            type="button"
            className="btn btn-primary btn-lg pulse-glow"
            onClick={() => navigate('/devices')}
          >
            <Search size={20} />
            {t('home.cta_analyze')}
          </button>
          <button
            type="button"
            className="btn btn-secondary btn-lg"
            onClick={() => navigate('/devices')}
          >
            <Disc size={20} />
            {t('home.cta_image')}
          </button>
        </div>
      </div>

      <div className="mb-6">
        <WarningBanner variant="warning">{t('safety.banner_source')}</WarningBanner>
      </div>

      <div className="grid grid-2 gap-6">
        <SectionCard
          title={t('home.recent_scans')}
          action={
            scanHistory.length > 0 ? (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => navigate('/history')}
              >
                {t('home.view_all')}
              </button>
            ) : undefined
          }
        >
          {scanHistory.length === 0 ? (
            <div
              className="flex flex-col items-center gap-3"
              style={{ padding: 'var(--space-8) 0' }}
            >
              <Clock size={32} className="text-muted" style={{ opacity: 0.3 }} />
              <span className="text-secondary text-sm">{t('home.no_recent_scans')}</span>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              {scanHistory.slice(0, 5).map((session) => (
                <div
                  key={session.id}
                  className="flex items-center justify-between"
                  style={{
                    padding: 'var(--space-2) 0',
                    borderBottom: '1px solid var(--color-border-light)',
                  }}
                >
                  <div>
                    <span className="font-medium">{session.deviceName}</span>
                    <span className="text-sm text-secondary ml-2">
                      {formatSessionDate(session.startedAtMs)}
                    </span>
                  </div>
                  <span className="badge-device">
                    {buildRecentSessionBadge(session.scanType, session.filesFound, t)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </SectionCard>

        <SectionCard title={t('home.system_status')}>
          <div className="flex flex-col gap-4" style={{ padding: 'var(--space-4) 0' }}>
            {limitedCapabilityNotes.length > 0 && (
              <WarningBanner variant="info">
                <strong>{t('home.system_status_limited_title')}</strong>
                <div style={{ marginTop: 'var(--space-2)' }}>
                  {limitedCapabilityNotes.map((note) => (
                    <div key={note}>{note}</div>
                  ))}
                </div>
              </WarningBanner>
            )}
            {capabilities.map((capability) => {
              const style = statusStyles[capability.status];
              return (
                <div key={capability.label} className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div
                      style={{
                        width: 10,
                        height: 10,
                        borderRadius: '50%',
                        background: style.dot,
                        boxShadow: `0 0 8px ${style.dot}55`,
                      }}
                    />
                    <span className="text-sm">{capability.label}</span>
                  </div>
                  <span className="badge-device" style={{ color: style.badge }}>
                    {t(`common.${capability.status}`)}
                  </span>
                </div>
              );
            })}
          </div>
        </SectionCard>
      </div>
    </div>
  );
}
