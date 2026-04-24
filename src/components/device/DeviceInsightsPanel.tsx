import { HardDrive, Layers3, Lock, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { EncryptionInfo, RaidMetadata } from '../../types';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';

interface DeviceInsightsPanelProps {
  raidMetadata: RaidMetadata | null;
  encryptionInfo: EncryptionInfo | null;
  loading: boolean;
  error?: string | null;
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

function encryptionVariant(info: EncryptionInfo | null): 'success' | 'warning' | 'info' {
  if (!info?.detected) {
    return 'success';
  }
  return info.workflowState === 'unsupported' ? 'info' : 'warning';
}

function encryptionRecoveryPostureKey(
  info: EncryptionInfo | null,
):
  | 'devices.encryption_posture_clear'
  | 'devices.encryption_posture_unlock_first'
  | 'devices.encryption_posture_lab_only' {
  if (!info?.detected) {
    return 'devices.encryption_posture_clear';
  }
  if (info.workflowState === 'pre_unlock_blocked') {
    return 'devices.encryption_posture_unlock_first';
  }
  return 'devices.encryption_posture_lab_only';
}

export function DeviceInsightsPanel({
  raidMetadata,
  encryptionInfo,
  loading,
  error,
}: DeviceInsightsPanelProps) {
  const { t } = useTranslation();

  if (loading) {
    return (
      <SectionCard title={t('devices.advanced_title')}>
        <p className="text-secondary text-sm">{t('devices.advanced_loading')}</p>
      </SectionCard>
    );
  }

  return (
    <SectionCard title={t('devices.advanced_title')}>
      <div className="flex flex-col gap-4">
        {error && (
          <WarningBanner variant="warning">
            {t('devices.advanced_partial_error', { error })}
          </WarningBanner>
        )}

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
            gap: 'var(--space-4)',
          }}
        >
          <div
            style={{
              padding: 'var(--space-4)',
              borderRadius: 'var(--radius-lg)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
            }}
          >
            <div className="flex items-center gap-2 mb-3">
              <Layers3 size={16} />
              <span className="font-semibold text-sm">{t('devices.raid_title')}</span>
            </div>

            {raidMetadata ? (
              <div className="flex flex-col gap-2 text-sm">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-secondary">{t('devices.raid_level')}</span>
                  <span className="font-medium">{raidMetadata.level}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span className="text-secondary">{t('devices.raid_members')}</span>
                  <span className="font-medium">{raidMetadata.memberCount}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span className="text-secondary">{t('devices.raid_stripe')}</span>
                  <span className="font-medium">{formatBytes(raidMetadata.stripeSizeBytes)}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span className="text-secondary">{t('devices.raid_offset')}</span>
                  <span className="font-medium">{formatBytes(raidMetadata.dataOffsetBytes)}</span>
                </div>
                {raidMetadata.superblockVersion && (
                  <div className="flex items-center justify-between gap-3">
                    <span className="text-secondary">{t('devices.raid_superblock')}</span>
                    <span className="font-medium">{raidMetadata.superblockVersion}</span>
                  </div>
                )}
              </div>
            ) : (
              <p className="text-secondary text-sm" style={{ margin: 0 }}>
                {t('devices.raid_none')}
              </p>
            )}
          </div>

          <div
            style={{
              padding: 'var(--space-4)',
              borderRadius: 'var(--radius-lg)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
            }}
          >
            <div className="flex items-center gap-2 mb-3">
              <Lock size={16} />
              <span className="font-semibold text-sm">{t('devices.encryption_title')}</span>
            </div>

            <WarningBanner variant={encryptionVariant(encryptionInfo)}>
              {encryptionInfo?.detected
                ? t('devices.encryption_detected', {
                    type: encryptionInfo.encryptionType.toUpperCase(),
                  })
                : t('devices.encryption_none')}
            </WarningBanner>

            <div className="flex flex-col gap-2 text-sm" style={{ marginTop: 'var(--space-3)' }}>
              <div className="flex items-center justify-between gap-3">
                <span className="text-secondary">{t('devices.encryption_status')}</span>
                <span className="font-medium">
                  {encryptionInfo?.detected ? t('common.yes') : t('common.no')}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <span className="text-secondary">{t('devices.encryption_unlock_status')}</span>
                <span className="font-medium">
                  {encryptionInfo?.canUnlock
                    ? t('devices.encryption_unlock_lab_only')
                    : t('devices.encryption_unlock_unavailable')}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <span className="text-secondary">{t('devices.encryption_workflow_state')}</span>
                <span className="font-medium">
                  {!encryptionInfo?.detected
                    ? t('devices.encryption_workflow_clear')
                    : encryptionInfo.workflowState === 'pre_unlock_blocked'
                      ? t('devices.encryption_workflow_pre_unlock_blocked')
                      : t('devices.encryption_workflow_unsupported')}
                </span>
              </div>
              <div className="flex items-center justify-between gap-3">
                <span className="text-secondary">{t('devices.encryption_recovery_posture')}</span>
                <span className="font-medium">
                  {t(encryptionRecoveryPostureKey(encryptionInfo))}
                </span>
              </div>
            </div>

            <div
              className="text-sm text-secondary"
              style={{ marginTop: 'var(--space-3)', display: 'flex', gap: 'var(--space-2)' }}
            >
              <ShieldAlert size={16} style={{ flexShrink: 0, marginTop: 2 }} />
              <div className="flex flex-col gap-1">
                <span>{encryptionInfo?.message ?? t('devices.encryption_none')}</span>
                {encryptionInfo?.detected && (
                  <span>
                    {t('devices.encryption_next_step', { step: encryptionInfo.saferNextStep })}
                  </span>
                )}
              </div>
            </div>
          </div>
        </div>

        <div
          className="text-sm text-secondary"
          style={{ display: 'flex', gap: 'var(--space-2)', alignItems: 'flex-start' }}
        >
          <HardDrive size={16} style={{ flexShrink: 0, marginTop: 2 }} />
          <span>{t('devices.advanced_safety_note')}</span>
        </div>
      </div>
    </SectionCard>
  );
}
