import { useTranslation } from 'react-i18next';
import type { ExportProgress } from '../../types';
import type { ScanLogEntry } from '../../types/scan';
import { ErrorState } from '../common/ErrorState';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';
import { ProgressTimeline } from '../scan/ProgressTimeline';
import { ScanLogPanel } from '../scan/ScanLogPanel';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '—';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

interface ExportStepProgressProps {
  exportProgress: ExportProgress | null;
  exportLogs: ScanLogEntry[];
  exportPercent: number;
  exportRunning: boolean;
  activeExportId: string | null;
  logsError: string | null;
}

export function ExportStepProgress(props: ExportStepProgressProps) {
  const { t } = useTranslation();
  const { exportProgress, exportRunning, exportPercent } = props;

  if (!exportProgress) {
    return (
      <SectionCard title={t('export.progress_title')}>
        <div className="text-secondary text-center py-8">{t('export.wizard.waiting_start')}</div>
      </SectionCard>
    );
  }

  return (
    <div>
      <SectionCard title={t('export.progress_title')}>
        <div className="flex flex-col gap-4">
          <div className="flex items-center justify-between">
            <span className="font-medium">{t(`export.status.${exportProgress.status}`)}</span>
            <span className="text-sm text-secondary">{exportPercent.toFixed(1)}%</span>
          </div>

          <ProgressTimeline percent={exportPercent} animated={exportRunning} />

          <div className="grid grid-4 gap-4">
            <div className="stat-card">
              <span className="stat-card-label">{t('export.files_done')}</span>
              <span className="stat-card-value">
                {exportProgress.exportedFiles}/{exportProgress.totalFiles}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.bytes_done')}</span>
              <span className="stat-card-value">{formatBytes(exportProgress.exportedBytes)}</span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.errors')}</span>
              <span className="stat-card-value text-warning">{exportProgress.errors.length}</span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.current_file')}</span>
              <span className="stat-card-value" style={{ fontSize: 'var(--font-size-sm)' }}>
                {exportProgress.currentFile || '—'}
              </span>
            </div>
          </div>

          {exportProgress.status === 'completed' && (
            <WarningBanner variant={exportProgress.errors.length > 0 ? 'warning' : 'success'}>
              {exportProgress.errors.length > 0
                ? t('export.completed_with_errors')
                : t('export.completed_ok')}
            </WarningBanner>
          )}

          {exportProgress.status === 'error' && (
            <WarningBanner variant="danger">{t('export.failed')}</WarningBanner>
          )}

          {exportProgress.errors.length > 0 && (
            <div className="flex flex-col gap-2">
              {exportProgress.errors.map((exportError, index) => (
                <div key={`${exportError.fileId}-${index}`} className="text-sm text-secondary">
                  <span className="font-medium">{exportError.fileName}</span>: {exportError.reason}
                </div>
              ))}
            </div>
          )}
        </div>
      </SectionCard>

      {props.activeExportId && (
        <SectionCard title={t('export.log_title')} className="mt-6">
          <div className="flex flex-col gap-3">
            <div className="text-sm text-secondary">
              <span className="font-medium">{t('history.selected_export')}:</span>{' '}
              {props.activeExportId}
            </div>
            {props.logsError ? (
              <ErrorState title={t('common.error')} description={props.logsError} />
            ) : (
              <ScanLogPanel logs={props.exportLogs} emptyMessage={t('export.log_waiting')} />
            )}
          </div>
        </SectionCard>
      )}
    </div>
  );
}
