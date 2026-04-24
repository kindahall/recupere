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

export function ExpertPartitionsPanel({ device }: { device: DetectedDevice }) {
  const { t } = useTranslation();

  return (
    <details>
      <summary className="text-sm font-semibold mb-2" style={{ cursor: 'pointer' }}>
        {t('expert.partitions')}
      </summary>
      <SectionCard title={t('expert.partitions')}>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-xs)' }}>
          {device.partitions.length === 0 ? (
            <span className="text-muted">{t('expert.no_partitions')}</span>
          ) : (
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ borderBottom: '1px solid var(--color-border)' }}>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_index')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_label')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_fs')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_offset')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_size')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'center',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_boot')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {device.partitions.map((p, i) => (
                  <tr key={p.id} style={{ borderBottom: '1px solid var(--color-border-light)' }}>
                    <td style={{ padding: '4px 8px' }}>{i + 1}</td>
                    <td style={{ padding: '4px 8px' }} className="font-medium">
                      {p.label}
                    </td>
                    <td style={{ padding: '4px 8px' }}>
                      <span className="text-accent">{p.filesystem.toUpperCase()}</span>
                    </td>
                    <td style={{ padding: '4px 8px', textAlign: 'right' }}>
                      0x{p.startOffset.toString(16).toUpperCase()}
                    </td>
                    <td style={{ padding: '4px 8px', textAlign: 'right' }}>
                      {formatBytes(p.sizeBytes)}
                    </td>
                    <td style={{ padding: '4px 8px', textAlign: 'center' }}>
                      {p.isBootable ? <span className="text-success">✓</span> : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </SectionCard>
    </details>
  );
}
