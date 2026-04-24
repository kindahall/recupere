import {
  Cpu,
  CreditCard,
  Database,
  Disc,
  HardDrive,
  Lock,
  Shield,
  ShieldAlert,
  ShieldOff,
  Usb,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { DetectedDevice, DeviceType } from '../../types';
import { RiskBadge } from '../common/RiskBadge';

interface DeviceCardProps {
  device: DetectedDevice;
  onDiagnose?: (device: DetectedDevice) => void;
  onCreateImage?: (device: DetectedDevice) => void;
  onRemoveSource?: (device: DetectedDevice) => void;
  diagnoseDisabled?: boolean;
  diagnoseDisabledReason?: string;
}

const deviceIcons: Record<DeviceType, typeof HardDrive> = {
  hdd: HardDrive,
  ssd: Cpu,
  nvme: Cpu,
  usb: Usb,
  sd: CreditCard,
  external: Database,
  image: Disc,
};

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

const statusColors: Record<string, string> = {
  healthy: 'var(--color-success)',
  degraded: 'var(--color-warning)',
  failing: 'var(--color-danger)',
  unresponsive: 'var(--color-critical)',
  unknown: 'var(--color-text-secondary)',
};

export function DeviceCard({
  device,
  onDiagnose,
  onCreateImage,
  onRemoveSource,
  diagnoseDisabled = false,
  diagnoseDisabledReason,
}: DeviceCardProps) {
  const { t } = useTranslation();
  const Icon = deviceIcons[device.type] || HardDrive;
  const usedPercent =
    device.capacityBytes > 0 ? Math.min(100, (device.usedBytes / device.capacityBytes) * 100) : 0;
  const importedImageSource = device.type === 'image';

  const SmartIcon =
    device.smartAvailable === true
      ? Shield
      : device.smartAvailable === false
        ? ShieldOff
        : ShieldAlert;
  const smartColor =
    device.smartAvailable === true
      ? 'var(--color-success)'
      : device.smartAvailable === false
        ? 'var(--color-text-secondary)'
        : 'var(--color-warning)';

  return (
    <div
      className="device-card"
      role={diagnoseDisabled ? undefined : 'button'}
      tabIndex={diagnoseDisabled ? undefined : 0}
      aria-disabled={diagnoseDisabled}
      onClick={() => {
        if (!diagnoseDisabled) {
          onDiagnose?.(device);
        }
      }}
      onKeyDown={(event) => {
        if (diagnoseDisabled) {
          return;
        }
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onDiagnose?.(device);
        }
      }}
    >
      <div className={`device-card-icon ${device.type}`}>
        <Icon size={24} />
      </div>
      <div className="device-card-info">
        <div className="device-card-name">
          {device.name}
          {device.model && <span className="text-sm text-secondary ml-2">{device.model}</span>}
        </div>
        <div className="device-card-details">
          <span className="badge-device">{t(`device_type.${device.type}`)}</span>
          <span className="badge-device" style={{ background: 'var(--color-bg-secondary)' }}>
            {device.filesystem.toUpperCase()}
          </span>
          <RiskBadge level={device.riskLevel} />
          {device.isEncrypted && (
            <span
              title={t('devices.encrypted')}
              style={{ color: 'var(--color-warning)', display: 'inline-flex' }}
            >
              <Lock size={14} />
            </span>
          )}
          {device.isTrimEnabled && <span className="text-xs text-secondary">TRIM</span>}
        </div>

        {importedImageSource ? (
          <div className="mt-2">
            <div className="text-xs text-secondary">{formatBytes(device.capacityBytes)}</div>
            <div className="text-xs text-secondary" title={device.devicePath}>
              {device.devicePath}
            </div>
          </div>
        ) : (
          <div className="capacity-bar-container mt-2">
            <div className="capacity-bar">
              <div
                className="capacity-bar-fill"
                style={{
                  width: `${usedPercent}%`,
                  background:
                    usedPercent > 90
                      ? 'var(--color-danger)'
                      : usedPercent > 70
                        ? 'var(--color-warning)'
                        : 'var(--color-accent)',
                }}
              />
            </div>
            <div className="capacity-bar-labels">
              <span className="text-xs text-secondary">
                {formatBytes(device.usedBytes)} / {formatBytes(device.capacityBytes)}
              </span>
              <span className="text-xs text-secondary">{usedPercent.toFixed(0)}%</span>
            </div>
          </div>
        )}
      </div>

      <div className="device-card-status">
        <div
          className="health-badge"
          style={{ color: statusColors[device.status] || statusColors.unknown }}
        >
          <span
            className="health-badge-dot"
            style={{ background: statusColors[device.status] || statusColors.unknown }}
          />
          {t(`devices.status_${device.status}`)}
        </div>
        <span
          title={
            device.smartAvailable ? t('devices.smart_available') : t('devices.smart_unavailable')
          }
          style={{ color: smartColor }}
        >
          <SmartIcon size={16} />
        </span>
      </div>

      <div className="device-card-actions">
        <button
          type="button"
          className="btn btn-primary btn-sm"
          onClick={(e) => {
            e.stopPropagation();
            if (!diagnoseDisabled) {
              onDiagnose?.(device);
            }
          }}
          disabled={diagnoseDisabled}
          title={diagnoseDisabled ? diagnoseDisabledReason : undefined}
        >
          {t('devices.diagnose')}
        </button>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={(e) => {
            e.stopPropagation();
            if (importedImageSource) {
              onRemoveSource?.(device);
            } else {
              onCreateImage?.(device);
            }
          }}
        >
          {importedImageSource ? t('devices.remove_source') : t('devices.create_image')}
        </button>
      </div>
    </div>
  );
}
