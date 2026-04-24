import { useTranslation } from 'react-i18next';
import type { RecoveredFile } from '../../types';
import { ErrorState } from '../common/ErrorState';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '—';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

interface ExportStepConfirmProps {
  files: RecoveredFile[];
  implicitPreviewFirstExcludedCount: number;
  destination: string;
  conflictStrategy: 'rename' | 'skip' | 'overwrite';
  preserveStructure: boolean;
  verifyIntegrity: boolean;
  requiredBytes: number;
  apfsExportOperatorVisible: boolean;
  apfsCurrentCatalogFilesCount: number;
  apfsReassembledFilesCount: number;
  snapshotFilesCount: number;
  journalFilesCount: number;
  filesystemMemoryContextFilesCount: number;
  filesystemMemoryAdjustedFilesCount: number;
  raidImportedSource: boolean;
  raidAnalysisPath: string | undefined;
  onStartExport: () => void;
  exportEngineAvailable: boolean;
  error: string | null;
}

export function ExportStepConfirm(props: ExportStepConfirmProps) {
  const { t } = useTranslation();

  return (
    <div>
      <SectionCard title={t('export.wizard.confirm_title')}>
        <div className="flex flex-col gap-3">
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.file_count')}</span>
            <span className="font-medium">{props.files.length}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.space_required')}</span>
            <span className="font-medium">{formatBytes(props.requiredBytes)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.destination')}</span>
            <span
              className="font-medium"
              style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-sm)' }}
            >
              {props.destination}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.conflict_strategy')}</span>
            <span className="font-medium">{t(`export.conflict_${props.conflictStrategy}`)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.preserve_structure')}</span>
            <span className="font-medium">
              {props.preserveStructure ? t('common.yes') : t('common.no')}
            </span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.verify_integrity')}</span>
            <span className="font-medium">
              {props.verifyIntegrity ? t('common.yes') : t('common.no')}
            </span>
          </div>
          {props.apfsExportOperatorVisible && (
            <>
              <div className="flex justify-between">
                <span className="text-sm text-secondary">
                  {t('export.apfs_operator_current_catalog')}
                </span>
                <span className="font-medium">{props.apfsCurrentCatalogFilesCount}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-secondary">
                  {t('export.apfs_operator_reassembled')}
                </span>
                <span className="font-medium">{props.apfsReassembledFilesCount}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-secondary">{t('export.apfs_operator_snapshot')}</span>
                <span className="font-medium">{props.snapshotFilesCount}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-sm text-secondary">{t('export.apfs_operator_journal')}</span>
                <span className="font-medium">{props.journalFilesCount}</span>
              </div>
            </>
          )}
          {props.filesystemMemoryContextFilesCount > 0 && (
            <div className="flex justify-between">
              <span className="text-sm text-secondary">
                {t('export.filesystem_memory_context_label')}
              </span>
              <span className="font-medium">{props.filesystemMemoryContextFilesCount}</span>
            </div>
          )}
          {props.filesystemMemoryAdjustedFilesCount > 0 && (
            <div className="flex justify-between">
              <span className="text-sm text-secondary">
                {t('export.filesystem_memory_score_label')}
              </span>
              <span className="font-medium">{props.filesystemMemoryAdjustedFilesCount}</span>
            </div>
          )}
          {props.raidImportedSource && (
            <div className="flex justify-between">
              <span className="text-sm text-secondary">{t('export.raid_source_label')}</span>
              <span
                className="font-medium"
                style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-sm)' }}
              >
                {props.raidAnalysisPath ?? '—'}
              </span>
            </div>
          )}
        </div>
      </SectionCard>

      {props.conflictStrategy === 'overwrite' && (
        <div className="mt-4">
          <WarningBanner variant="danger">{t('export.wizard.overwrite_warning')}</WarningBanner>
        </div>
      )}

      <div className="mt-4">
        <WarningBanner variant="info">{t('export.destination_warning')}</WarningBanner>
      </div>

      {props.apfsExportOperatorVisible && (
        <div className="mt-4">
          <WarningBanner variant="info">{t('export.apfs_operator_notice')}</WarningBanner>
        </div>
      )}

      {props.apfsReassembledFilesCount > 0 && (
        <div className="mt-4">
          <WarningBanner variant="success">
            {t('export.apfs_reassembled_notice', {
              count: props.apfsReassembledFilesCount,
            })}
          </WarningBanner>
        </div>
      )}

      {props.implicitPreviewFirstExcludedCount > 0 && (
        <div className="mt-4">
          <WarningBanner variant="warning">
            {t('export.apfs_preview_first_default_notice', {
              count: props.implicitPreviewFirstExcludedCount,
            })}
          </WarningBanner>
        </div>
      )}

      {props.filesystemMemoryContextFilesCount > 0 && (
        <div className="mt-4">
          <WarningBanner variant="info">
            {t('export.filesystem_memory_context_confirm_notice', {
              count: props.filesystemMemoryContextFilesCount,
            })}
          </WarningBanner>
        </div>
      )}

      {props.filesystemMemoryAdjustedFilesCount > 0 && (
        <div className="mt-4">
          <WarningBanner variant="info">
            {t('export.filesystem_memory_score_confirm_notice', {
              count: props.filesystemMemoryAdjustedFilesCount,
            })}
          </WarningBanner>
        </div>
      )}

      {props.raidImportedSource && (
        <div className="mt-4">
          <WarningBanner variant="success">
            {t('export.wizard.raid_source_confirm_notice')}
          </WarningBanner>
        </div>
      )}

      {props.error && (
        <div className="mt-4">
          <ErrorState title={t('common.error')} description={props.error} />
        </div>
      )}
    </div>
  );
}
