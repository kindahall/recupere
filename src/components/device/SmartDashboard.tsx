import { useTranslation } from 'react-i18next';
import type { SmartReport } from '../../types';
import { SectionCard } from '../common/SectionCard';

interface SmartDashboardProps {
  report: SmartReport | null;
  loading: boolean;
}

function healthColor(health: string): string {
  switch (health) {
    case 'passed':
      return 'var(--color-success, #22c55e)';
    case 'warning':
      return 'var(--color-warning, #eab308)';
    case 'failed':
      return 'var(--color-danger, #ef4444)';
    default:
      return 'var(--color-muted, #9ca3af)';
  }
}

function statusColor(status: string): string {
  switch (status) {
    case 'ok':
      return 'var(--color-success, #22c55e)';
    case 'warning':
      return 'var(--color-warning, #eab308)';
    case 'failed':
      return 'var(--color-danger, #ef4444)';
    default:
      return 'var(--color-muted, #9ca3af)';
  }
}

export function SmartDashboard({ report, loading }: SmartDashboardProps) {
  const { t } = useTranslation();

  if (loading) {
    return (
      <SectionCard title={t('devices.smart_title')}>
        <p className="text-secondary text-sm">{t('devices.smart_loading')}</p>
      </SectionCard>
    );
  }

  if (!report) {
    return null;
  }

  if (report.overallHealth === 'unavailable') {
    return (
      <SectionCard title={t('devices.smart_title')}>
        <p className="text-secondary text-sm">{t('devices.smart_unavailable_detail')}</p>
      </SectionCard>
    );
  }

  return (
    <SectionCard title={t('devices.smart_title')}>
      {/* Health badge */}
      <div className="flex items-center gap-3 mb-4">
        <span className="text-sm font-medium">{t('devices.smart_health')}:</span>
        <span
          className="inline-flex items-center px-2 py-0.5 rounded text-xs font-semibold"
          style={{
            backgroundColor: healthColor(report.overallHealth),
            color: '#fff',
          }}
        >
          {report.overallHealth.toUpperCase()}
        </span>
      </div>

      {/* Key metrics */}
      <div className="grid grid-cols-2 gap-3 mb-4" style={{ maxWidth: '480px' }}>
        {report.temperatureCelsius !== undefined && (
          <div className="flex flex-col">
            <span className="text-xs text-secondary">{t('devices.smart_temperature')}</span>
            <span className="text-sm font-medium">{report.temperatureCelsius} °C</span>
          </div>
        )}
        {report.powerOnHours !== undefined && (
          <div className="flex flex-col">
            <span className="text-xs text-secondary">{t('devices.smart_power_hours')}</span>
            <span className="text-sm font-medium">{report.powerOnHours.toLocaleString()} h</span>
          </div>
        )}
        {report.reallocatedSectors !== undefined && (
          <div className="flex flex-col">
            <span className="text-xs text-secondary">{t('devices.smart_reallocated')}</span>
            <span className="text-sm font-medium">{report.reallocatedSectors}</span>
          </div>
        )}
        {report.pendingSectors !== undefined && (
          <div className="flex flex-col">
            <span className="text-xs text-secondary">{t('devices.smart_pending')}</span>
            <span className="text-sm font-medium">{report.pendingSectors}</span>
          </div>
        )}
      </div>

      {/* Attributes table */}
      {report.attributes.length > 0 && (
        <div>
          <h4 className="text-sm font-semibold mb-2">{t('devices.smart_attributes')}</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr
                  className="text-left text-secondary border-b"
                  style={{ borderColor: 'var(--color-border, #e5e7eb)' }}
                >
                  <th className="py-1 pr-3">{t('devices.smart_attr_id')}</th>
                  <th className="py-1 pr-3">{t('devices.smart_attr_name')}</th>
                  <th className="py-1 pr-3">{t('devices.smart_attr_value')}</th>
                  <th className="py-1 pr-3">{t('devices.smart_attr_worst')}</th>
                  <th className="py-1 pr-3">{t('devices.smart_attr_threshold')}</th>
                  <th className="py-1 pr-3">{t('devices.smart_attr_raw')}</th>
                  <th className="py-1">{t('devices.smart_attr_status')}</th>
                </tr>
              </thead>
              <tbody>
                {report.attributes.map((attr) => (
                  <tr
                    key={attr.id}
                    className="border-b"
                    style={{ borderColor: 'var(--color-border, #e5e7eb)' }}
                  >
                    <td className="py-1 pr-3">{attr.id}</td>
                    <td className="py-1 pr-3">{attr.name}</td>
                    <td className="py-1 pr-3">{attr.value}</td>
                    <td className="py-1 pr-3">{attr.worst}</td>
                    <td className="py-1 pr-3">{attr.threshold}</td>
                    <td className="py-1 pr-3 font-mono">{attr.rawValue}</td>
                    <td className="py-1">
                      <span
                        className="inline-block w-2 h-2 rounded-full mr-1"
                        style={{ backgroundColor: statusColor(attr.status) }}
                      />
                      {attr.status}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </SectionCard>
  );
}
