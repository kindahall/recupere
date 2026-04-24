import { useTranslation } from 'react-i18next';
import type { DetectedDevice } from '../../types';
import { SectionCard } from '../common/SectionCard';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

const rowStyle = {
  padding: 'var(--space-2) 0',
  borderBottom: '1px solid var(--color-border)',
} as const;

export function ExpertMetadataPanel({ device }: { device: DetectedDevice }) {
  const { t } = useTranslation();

  return (
    <SectionCard title={t('expert.metadata')}>
      <div className="flex flex-col gap-3" style={{ fontSize: 'var(--font-size-sm)' }}>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_device_path')}</span>
          <span className="font-medium" style={{ fontFamily: 'var(--font-mono)' }}>
            {device.devicePath}
          </span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_type')}</span>
          <span className="font-medium">{device.type.toUpperCase()}</span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_filesystem')}</span>
          <span className="font-medium" style={{ color: 'var(--color-accent)' }}>
            {device.filesystem.toUpperCase()}
          </span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_capacity')}</span>
          <span className="font-medium">{formatBytes(device.capacityBytes)}</span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_used')}</span>
          <span className="font-medium">{formatBytes(device.usedBytes)}</span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_status')}</span>
          <span className="font-medium">{t(`devices.status_${device.status}`)}</span>
        </div>
        {device.model && (
          <div className="flex justify-between" style={rowStyle}>
            <span className="text-secondary">{t('expert.meta_model')}</span>
            <span className="font-medium">{device.model}</span>
          </div>
        )}
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">TRIM</span>
          <span className="font-medium">
            {device.isTrimEnabled ? t('expert.meta_trim_enabled') : t('expert.meta_trim_disabled')}
          </span>
        </div>
        <div className="flex justify-between" style={rowStyle}>
          <span className="text-secondary">{t('expert.meta_encrypted')}</span>
          <span className="font-medium">
            {device.isEncrypted ? t('expert.meta_yes') : t('expert.meta_no')}
          </span>
        </div>
        <div className="flex justify-between" style={{ padding: 'var(--space-2) 0' }}>
          <span className="text-secondary">S.M.A.R.T.</span>
          <span className="font-medium">
            {device.smartAvailable ? t('expert.meta_smart_available') : t('expert.meta_smart_na')}
          </span>
        </div>
      </div>
    </SectionCard>
  );
}
