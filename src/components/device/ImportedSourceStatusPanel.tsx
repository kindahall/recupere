import { HardDriveDownload, ShieldCheck, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { DetectedDevice, ImportedRecoverySourceStatus } from '../../types';
import { getImportedSourceKind } from '../../utils/importedSourceKind';
import { WarningBanner } from '../common/WarningBanner';

interface ImportedSourceStatusPanelProps {
  device?: DetectedDevice | null;
  status: ImportedRecoverySourceStatus | null;
  loading: boolean;
  preparing: boolean;
  error?: string | null;
  onPrepare?: () => void;
}

function formatBytes(bytes?: number): string {
  if (typeof bytes !== 'number' || Number.isNaN(bytes) || bytes <= 0) {
    return '—';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(value >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function bannerVariant(
  status: ImportedRecoverySourceStatus | null,
): 'info' | 'warning' | 'danger' | 'success' {
  if (!status) return 'info';
  if (status.supportTier === 'unsupported') return 'danger';
  if (status.supportTier === 'limited') return 'warning';
  if (!status.sourceAvailable) return 'danger';
  if (status.requiresPreparation && !status.prepared) return 'warning';
  return 'success';
}

export function ImportedSourceStatusPanel({
  device,
  status,
  loading,
  preparing,
  error,
  onPrepare,
}: ImportedSourceStatusPanelProps) {
  const { t } = useTranslation();
  const sourceKind = getImportedSourceKind(status, device);
  const sourceClassLabelKey =
    sourceKind === 'raid-analysis'
      ? 'devices.imported_source_class_raid'
      : sourceKind === 'forensic-image'
        ? 'devices.imported_source_class_forensic'
        : sourceKind === 'virtual-disk'
          ? 'devices.imported_source_class_virtual'
          : sourceKind === 'raw-image'
            ? 'devices.imported_source_class_raw'
            : 'devices.imported_source_class_generic';
  const preparationReasonKey =
    sourceKind === 'forensic-image'
      ? 'devices.imported_preparation_reason_forensic'
      : sourceKind === 'virtual-disk'
        ? 'devices.imported_preparation_reason_virtual'
        : null;

  if (loading) {
    return (
      <div className="imported-source-panel">
        <div className="imported-source-panel-header">
          <div className="imported-source-panel-title">
            <HardDriveDownload size={16} />
            <span>{t('devices.imported_status_title')}</span>
          </div>
        </div>
        <p className="text-sm text-secondary">{t('devices.imported_status_loading')}</p>
      </div>
    );
  }

  return (
    <div className="imported-source-panel">
      <div className="imported-source-panel-header">
        <div>
          <div className="imported-source-panel-title">
            <HardDriveDownload size={16} />
            <span>{t('devices.imported_status_title')}</span>
          </div>
          <p className="imported-source-panel-subtitle">{t('devices.imported_status_subtitle')}</p>
        </div>
        {status?.requiresPreparation && !status.prepared && onPrepare && (
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={onPrepare}
            disabled={preparing || !status.sourceAvailable}
          >
            {preparing
              ? t('devices.imported_prepare_working')
              : t('devices.imported_prepare_action')}
          </button>
        )}
      </div>

      {error ? (
        <WarningBanner variant="danger">{error}</WarningBanner>
      ) : (
        <WarningBanner variant={bannerVariant(status)}>
          {!status
            ? t('devices.imported_status_unavailable')
            : status.supportTier === 'unsupported'
              ? t('devices.imported_status_unsupported', {
                  format: status.sourceFormat,
                })
              : status.supportTier === 'limited'
                ? t('devices.imported_status_limited', {
                    format: status.sourceFormat,
                  })
                : !status.sourceAvailable
                  ? t('devices.imported_status_missing')
                  : status.requiresPreparation && !status.prepared
                    ? t('devices.imported_status_prepare_needed', {
                        format: status.sourceFormat,
                      })
                    : status.requiresPreparation
                      ? t('devices.imported_status_prepared', {
                          format: status.sourceFormat,
                        })
                      : t('devices.imported_status_direct', {
                          format: status.sourceFormat,
                        })}
        </WarningBanner>
      )}

      {status && (
        <div className="flex flex-col gap-4">
          <div className="imported-source-grid">
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_registered_name')}</span>
              <span className="font-medium">{status.displayName}</span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_format')}</span>
              <span className="font-medium">{status.sourceFormat}</span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_source_class')}</span>
              <span className="font-medium">{t(sourceClassLabelKey)}</span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_logical_size')}</span>
              <span className="font-medium">{formatBytes(status.logicalSizeBytes)}</span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_analysis_mode')}</span>
              <span className="font-medium">
                {status.supportTier === 'unsupported'
                  ? t('devices.imported_analysis_mode_blocked')
                  : status.requiresPreparation
                    ? t('devices.imported_analysis_mode_cache')
                    : t('devices.imported_analysis_mode_direct')}
              </span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_support_tier')}</span>
              <span className="font-medium">
                {status.supportTier === 'unsupported'
                  ? t('devices.imported_support_tier_unsupported')
                  : status.supportTier === 'limited'
                    ? t('devices.imported_support_tier_limited')
                    : t('devices.imported_support_tier_supported')}
              </span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_source_state')}</span>
              <span className="font-medium">
                {status.sourceAvailable
                  ? t('devices.imported_source_state_available')
                  : t('devices.imported_source_state_missing')}
              </span>
            </div>
            <div className="imported-source-row">
              <span className="text-secondary">{t('devices.imported_prepare_state')}</span>
              <span className="font-medium">
                {status.prepared
                  ? t('devices.imported_prepare_state_ready')
                  : status.requiresPreparation
                    ? t('devices.imported_prepare_state_required')
                    : t('devices.imported_prepare_state_not_needed')}
              </span>
            </div>
            {status.cachePath && (
              <div className="imported-source-row">
                <span className="text-secondary">{t('devices.imported_cache_size')}</span>
                <span className="font-medium">{formatBytes(status.cacheSizeBytes)}</span>
              </div>
            )}
            {preparationReasonKey && status.requiresPreparation && (
              <div className="imported-source-row imported-source-row-path">
                <span className="text-secondary">{t('devices.imported_preparation_reason')}</span>
                <span className="font-medium">{t(preparationReasonKey)}</span>
              </div>
            )}
          </div>

          {(status.supportNote || status.saferNextStep) && (
            <div
              style={{
                padding: 'var(--space-4)',
                borderRadius: 'var(--radius-lg)',
                border: '1px solid var(--color-border)',
                background: 'var(--color-bg-secondary)',
              }}
            >
              <div className="font-medium">{t('devices.imported_support_title')}</div>
              {status.supportNote && (
                <p className="text-sm text-secondary" style={{ marginTop: 'var(--space-2)' }}>
                  {status.supportNote}
                </p>
              )}
              {status.saferNextStep && (
                <p className="text-sm text-secondary" style={{ marginTop: 'var(--space-2)' }}>
                  {t('devices.imported_support_next_step', { step: status.saferNextStep })}
                </p>
              )}
            </div>
          )}

          <div
            style={{
              padding: 'var(--space-4)',
              borderRadius: 'var(--radius-lg)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
              display: 'flex',
              flexDirection: 'column',
              gap: 'var(--space-3)',
            }}
          >
            <div>
              <div className="font-medium">{t('devices.imported_traceability_title')}</div>
              <p className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                {status.requiresPreparation
                  ? t('devices.imported_traceability_subtitle_cache')
                  : t('devices.imported_traceability_subtitle_direct')}
              </p>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-3)' }}>
              <div
                style={{
                  padding: 'var(--space-3)',
                  borderRadius: 'var(--radius-md)',
                  border: '1px solid var(--color-border)',
                  background: 'var(--color-bg-primary)',
                }}
              >
                <div className="text-xs text-secondary">{t('devices.imported_trace_source')}</div>
                <div className="font-medium" title={status.sourcePath}>
                  {status.sourcePath}
                </div>
                <div className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                  {status.sourceAvailable
                    ? t('devices.imported_trace_source_available')
                    : t('devices.imported_trace_source_missing')}
                </div>
              </div>

              {status.requiresPreparation && (
                <div
                  style={{
                    padding: 'var(--space-3)',
                    borderRadius: 'var(--radius-md)',
                    border: '1px solid var(--color-border)',
                    background: 'var(--color-bg-primary)',
                  }}
                >
                  <div className="text-xs text-secondary">{t('devices.imported_trace_cache')}</div>
                  <div className="font-medium" title={status.cachePath}>
                    {status.cachePath ?? t('devices.imported_trace_pending_path')}
                  </div>
                  <div className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                    {status.prepared
                      ? t('devices.imported_trace_cache_ready')
                      : t('devices.imported_trace_cache_pending')}
                  </div>
                </div>
              )}

              <div
                style={{
                  padding: 'var(--space-3)',
                  borderRadius: 'var(--radius-md)',
                  border: '1px solid var(--color-border)',
                  background: 'var(--color-bg-primary)',
                }}
              >
                <div className="text-xs text-secondary">{t('devices.imported_trace_analysis')}</div>
                <div className="font-medium" title={status.analysisPath ?? status.sourcePath}>
                  {status.analysisPath ?? status.sourcePath}
                </div>
                <div className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                  {!status.requiresPreparation
                    ? t('devices.imported_trace_analysis_direct')
                    : status.prepared
                      ? t('devices.imported_trace_analysis_cache')
                      : t('devices.imported_trace_analysis_blocked')}
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {status?.requiresPreparation && !status.prepared && (
        <div className="imported-source-note">
          <TriangleAlert size={16} />
          <span>{t('devices.imported_prepare_note')}</span>
        </div>
      )}

      {status?.prepared && (
        <div className="imported-source-note">
          <ShieldCheck size={16} />
          <span>{t('devices.imported_ready_note')}</span>
        </div>
      )}
    </div>
  );
}
