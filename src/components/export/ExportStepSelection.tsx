import { useTranslation } from 'react-i18next';
import type { RecoveredFile } from '../../types';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '—';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

interface ExportStepSelectionProps {
  files: RecoveredFile[];
  implicitPreviewFirstExcludedCount: number;
  requiredBytes: number;
  resourceForkFilesCount: number;
  resourceForkBytes: number;
  alternateDataStreamsCount: number;
  alternateDataStreamsBytes: number;
  apfsExportOperatorVisible: boolean;
  apfsCurrentCatalogFilesCount: number;
  apfsReassembledFilesCount: number;
  snapshotFilesCount: number;
  journalFilesCount: number;
  compressedFilesCount: number;
  highComplexityFilesCount: number;
  filesystemMemoryContextFilesCount: number;
  filesystemMemoryAdjustedFilesCount: number;
  deletedRecoveryExport: boolean;
  carvedExport: boolean;
  raidImportedSource: boolean;
  raidAnalysisPath: string | undefined;
}

export function ExportStepSelection(props: ExportStepSelectionProps) {
  const { t } = useTranslation();
  const { files } = props;

  const intactCount = files.filter((f) => f.integrity === 'intact').length;
  const partialCount = files.filter(
    (f) => f.integrity === 'partial' || f.integrity === 'fragmented',
  ).length;
  const uncertainCount = files.filter((f) => f.integrity === 'uncertain').length;
  const corruptCount = files.filter((f) => f.integrity === 'corrupt').length;

  return (
    <div>
      <SectionCard title={t('export.wizard.selection_title')}>
        <div className="grid grid-3 gap-4">
          <div className="stat-card">
            <span className="stat-card-label">{t('export.file_count')}</span>
            <span className="stat-card-value">{files.length}</span>
          </div>
          <div className="stat-card">
            <span className="stat-card-label">{t('export.space_required')}</span>
            <span className="stat-card-value">{formatBytes(props.requiredBytes)}</span>
          </div>
          <div className="stat-card">
            <span className="stat-card-label">{t('export.wizard.intact_files')}</span>
            <span className="stat-card-value text-success">{intactCount}</span>
          </div>
        </div>

        {(partialCount > 0 || uncertainCount > 0 || corruptCount > 0) && (
          <div className="grid grid-3 gap-4 mt-4">
            {partialCount > 0 && (
              <div className="stat-card">
                <span className="stat-card-label">{t('export.wizard.partial_files')}</span>
                <span className="stat-card-value text-warning">{partialCount}</span>
              </div>
            )}
            {uncertainCount > 0 && (
              <div className="stat-card">
                <span className="stat-card-label">{t('export.wizard.uncertain_files')}</span>
                <span className="stat-card-value text-warning">{uncertainCount}</span>
              </div>
            )}
            {corruptCount > 0 && (
              <div className="stat-card">
                <span className="stat-card-label">{t('export.wizard.corrupt_files')}</span>
                <span className="stat-card-value text-danger">{corruptCount}</span>
              </div>
            )}
          </div>
        )}

        {props.apfsExportOperatorVisible && (
          <div className="grid grid-4 gap-4 mt-4">
            <div className="stat-card">
              <span className="stat-card-label">{t('export.apfs_operator_current_catalog')}</span>
              <span className="stat-card-value text-success">
                {props.apfsCurrentCatalogFilesCount}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.apfs_operator_reassembled')}</span>
              <span className="stat-card-value text-success">
                {props.apfsReassembledFilesCount}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.apfs_operator_snapshot')}</span>
              <span className="stat-card-value text-warning">{props.snapshotFilesCount}</span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('export.apfs_operator_journal')}</span>
              <span className="stat-card-value" style={{ color: 'var(--color-danger)' }}>
                {props.journalFilesCount}
              </span>
            </div>
          </div>
        )}
      </SectionCard>

      <div className="flex flex-col gap-3 mt-4">
        {props.deletedRecoveryExport && (
          <WarningBanner variant="info">{t('export.wizard.deleted_recovery_notice')}</WarningBanner>
        )}
        {props.carvedExport && (
          <WarningBanner variant="info">{t('export.wizard.carved_notice')}</WarningBanner>
        )}
        {uncertainCount > 0 && (
          <WarningBanner variant="warning">
            {t('export.wizard.uncertain_notice', { count: uncertainCount })}
          </WarningBanner>
        )}
        {props.raidImportedSource && (
          <WarningBanner variant="success">
            {t('export.wizard.raid_source_notice', {
              path: props.raidAnalysisPath ?? '—',
            })}
          </WarningBanner>
        )}
        {props.apfsExportOperatorVisible && (
          <WarningBanner variant="info">{t('export.apfs_operator_notice')}</WarningBanner>
        )}
        {props.apfsReassembledFilesCount > 0 && (
          <WarningBanner variant="success">
            {t('export.apfs_reassembled_notice', {
              count: props.apfsReassembledFilesCount,
            })}
          </WarningBanner>
        )}
        {props.implicitPreviewFirstExcludedCount > 0 && (
          <WarningBanner variant="warning">
            {t('export.apfs_preview_first_default_notice', {
              count: props.implicitPreviewFirstExcludedCount,
            })}
          </WarningBanner>
        )}
        {props.resourceForkFilesCount > 0 && (
          <WarningBanner variant="warning">
            {t('export.resource_fork_notice', {
              count: props.resourceForkFilesCount,
              bytes: formatBytes(props.resourceForkBytes),
            })}
          </WarningBanner>
        )}
        {props.alternateDataStreamsCount > 0 && (
          <WarningBanner variant="warning">
            {t('export.ads_notice', {
              count: props.alternateDataStreamsCount,
              bytes: formatBytes(props.alternateDataStreamsBytes),
            })}
          </WarningBanner>
        )}
        {props.highComplexityFilesCount > 0 && (
          <WarningBanner variant="warning">
            {t('export.high_complexity_notice', { count: props.highComplexityFilesCount })}
          </WarningBanner>
        )}
        {props.filesystemMemoryContextFilesCount > 0 && (
          <WarningBanner variant="info">
            {t('export.filesystem_memory_context_notice', {
              count: props.filesystemMemoryContextFilesCount,
            })}
          </WarningBanner>
        )}
        {props.filesystemMemoryAdjustedFilesCount > 0 && (
          <WarningBanner variant="info">
            {t('export.filesystem_memory_score_notice', {
              count: props.filesystemMemoryAdjustedFilesCount,
            })}
          </WarningBanner>
        )}
      </div>
    </div>
  );
}
