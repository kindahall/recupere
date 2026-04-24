import { CheckSquare, Eye, Square } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { FileIntegrity, RecoveredFile } from '../../types';
import { IntegrityBadge } from '../common/IntegrityBadge';
import { SectionCard } from '../common/SectionCard';
import {
  formatBytes,
  formatTimestamp,
  getScoreColor,
  translationKeyForRecoveryComplexity,
  translationKeyForSourceView,
  translationKeyForValidatorStatus,
} from './resultsFormat';

export interface ResultsTableProps {
  files: RecoveredFile[];
  selectedFileIds: Set<string>;
  toggleFileSelection: (id: string) => void;
  toggleAll: () => void;
  allFilteredSelected: boolean;
  fileImportanceMap: Record<string, string>;
  activePreviewFileId: string | null;
  onPreview: (file: RecoveredFile) => void;
}

export function ResultsTable({
  files,
  selectedFileIds,
  toggleFileSelection,
  toggleAll,
  allFilteredSelected,
  fileImportanceMap,
  activePreviewFileId,
  onPreview,
}: ResultsTableProps) {
  const { t } = useTranslation();

  const formatObservedAt = (timestampMs: number | null | undefined): string =>
    typeof timestampMs === 'number' ? new Date(timestampMs).toLocaleString() : '—';

  const buildMetadataSummary = (file: RecoveredFile): string | null => {
    const details: string[] = [];

    if (file.modifiedAt) {
      details.push(`${t('results.modified_label')} ${formatTimestamp(file.modifiedAt)}`);
    }
    if (file.createdAt) {
      details.push(`${t('results.created_label')} ${formatTimestamp(file.createdAt)}`);
    }
    if (file.resourceFork) {
      details.push(
        t('results.resource_fork_hint', { size: formatBytes(file.resourceFork.sizeBytes) }),
      );
    }
    if (file.alternateDataStreams && file.alternateDataStreams.length > 0) {
      const totalBytes = file.alternateDataStreams.reduce(
        (sum, stream) => sum + stream.sizeBytes,
        0,
      );
      details.push(
        t('results.ads_hint', {
          count: file.alternateDataStreams.length,
          bytes: formatBytes(totalBytes),
        }),
      );
    }
    if (file.sourceView) {
      if (file.sourceView === 'snapshot' && typeof file.snapshotXid === 'number') {
        details.push(t('results.source_view_snapshot_xid', { xid: file.snapshotXid }));
      } else {
        details.push(t(translationKeyForSourceView(file.sourceView)));
      }
    }
    if (file.journalDerived && file.sourceView !== 'journal') {
      details.push(t('results.journal_derived_hint'));
    }
    if (file.compressionKind) {
      details.push(t('results.compression_hint', { kind: file.compressionKind.toUpperCase() }));
    }
    if (file.validatorStatus) {
      details.push(t(translationKeyForValidatorStatus(file.validatorStatus)));
    }
    if (file.recoveryComplexity) {
      details.push(
        t('results.complexity_hint', {
          level: t(translationKeyForRecoveryComplexity(file.recoveryComplexity)),
        }),
      );
    }
    if ((file.assemblySegmentCount ?? 1) > 1) {
      details.push(
        t('results.assembly_hint', {
          segments: file.assemblySegmentCount,
          gaps: file.gapCount ?? 0,
        }),
      );
    }
    if (file.filesystemMemoryContext) {
      details.push(
        t('results.filesystem_memory_last_known', {
          path: file.filesystemMemoryContext.lastKnownPath,
        }),
      );
      details.push(
        t('results.filesystem_memory_missing_window', {
          lastObserved: formatObservedAt(file.filesystemMemoryContext.lastObservedAtMs),
          firstMissing: formatObservedAt(file.filesystemMemoryContext.firstMissingObservedAtMs),
        }),
      );
      if (file.filesystemMemoryContext.fileModifiedAtMs !== null) {
        details.push(
          t('results.filesystem_memory_modified', {
            modifiedAt: formatObservedAt(file.filesystemMemoryContext.fileModifiedAtMs),
          }),
        );
      }
      if ((file.recoveryScoreAdjustment ?? 0) > 0 && typeof file.baseRecoveryScore === 'number') {
        details.push(
          t('results.filesystem_memory_score_hint', {
            base: file.baseRecoveryScore,
            adjusted: file.recoveryScore,
            delta: file.recoveryScoreAdjustment,
          }),
        );
      }
    }

    return details.length > 0 ? details.join(' | ') : null;
  };

  return (
    <SectionCard>
      {files.length === 0 ? (
        <div className="text-sm text-secondary">{t('results.filtered_empty')}</div>
      ) : (
        <table
          className="results-data-grid"
          style={{ width: '100%', borderCollapse: 'separate', borderSpacing: 0 }}
        >
          <thead>
            <tr>
              <th
                style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'left', width: 40 }}
              >
                <button type="button" className="btn btn-ghost btn-icon btn-sm" onClick={toggleAll}>
                  {allFilteredSelected ? <CheckSquare size={16} /> : <Square size={16} />}
                </button>
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'left',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.sort_name')}
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'left',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.sort_path')}
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'right',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.sort_size')}
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'center',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.filter_integrity')}
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'center',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.filter_score')}
              </th>
              <th
                style={{
                  padding: 'var(--space-2) var(--space-3)',
                  textAlign: 'center',
                  fontWeight: 600,
                  fontSize: 'var(--font-size-sm)',
                  color: 'var(--color-text-secondary)',
                }}
              >
                {t('results.importance')}
              </th>
              <th
                style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center', width: 50 }}
              />
            </tr>
          </thead>
          <tbody>
            {files.map((file) => {
              const metadataSummary = buildMetadataSummary(file);
              return (
                <tr
                  key={file.id}
                  style={{
                    borderBottom: '1px solid var(--color-border-light)',
                    background: selectedFileIds.has(file.id)
                      ? 'var(--color-accent-subtle)'
                      : undefined,
                    transition: 'background 120ms ease',
                  }}
                >
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                    <button
                      type="button"
                      className="btn btn-ghost btn-icon btn-sm"
                      onClick={() => toggleFileSelection(file.id)}
                    >
                      {selectedFileIds.has(file.id) ? (
                        <CheckSquare size={16} className="text-accent" />
                      ) : (
                        <Square size={16} />
                      )}
                    </button>
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{file.name}</span>
                      {file.isDeleted && (
                        <span className="badge-device">{t('results.deleted_badge')}</span>
                      )}
                      {file.sourceView === 'snapshot' && (
                        <span className="badge-device">{t('results.snapshot_badge')}</span>
                      )}
                      {(file.sourceView === 'journal' || file.journalDerived) && (
                        <span className="badge-device">{t('results.journal_badge')}</span>
                      )}
                      {file.compressionKind && (
                        <span className="badge-device">
                          {t('results.compressed_badge', {
                            kind: file.compressionKind.toUpperCase(),
                          })}
                        </span>
                      )}
                      {file.recoveryComplexity === 'high' && (
                        <span className="badge-device">{t('results.high_complexity_badge')}</span>
                      )}
                      {file.validatorStatus && file.validatorStatus !== 'validated' && (
                        <span className="badge-device">
                          {t(translationKeyForValidatorStatus(file.validatorStatus))}
                        </span>
                      )}
                      {file.resourceFork && (
                        <span className="badge-device">{t('results.resource_fork_badge')}</span>
                      )}
                      {file.alternateDataStreams && file.alternateDataStreams.length > 0 && (
                        <span className="badge-device">{t('results.ads_badge')}</span>
                      )}
                      {file.filesystemMemoryContext && (
                        <span className="badge-device">{t('results.filesystem_memory_badge')}</span>
                      )}
                    </div>
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                    <div className="flex flex-col gap-1">
                      <span className="text-sm text-secondary">{file.path}</span>
                      {metadataSummary && (
                        <span className="text-xs text-secondary">{metadataSummary}</span>
                      )}
                    </div>
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'right' }}>
                    <div className="flex flex-col items-end gap-1">
                      <span className="text-sm">{formatBytes(file.sizeBytes)}</span>
                      {typeof file.expectedSizeBytes === 'number' &&
                        file.expectedSizeBytes > file.sizeBytes && (
                          <span className="text-xs text-secondary">
                            {t('results.recoverable_size_hint', {
                              recoverable: formatBytes(file.sizeBytes),
                              expected: formatBytes(file.expectedSizeBytes),
                            })}
                          </span>
                        )}
                    </div>
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                    <IntegrityBadge integrity={file.integrity as FileIntegrity} />
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                    <div className="flex flex-col items-center gap-1">
                      <span
                        className="font-semibold"
                        style={{ color: getScoreColor(file.recoveryScore) }}
                      >
                        {file.recoveryScore}%
                      </span>
                      {(file.recoveryScoreAdjustment ?? 0) > 0 &&
                        typeof file.baseRecoveryScore === 'number' && (
                          <span className="text-xs text-secondary">
                            {t('results.filesystem_memory_score_delta', {
                              base: file.baseRecoveryScore,
                              delta: file.recoveryScoreAdjustment,
                            })}
                          </span>
                        )}
                    </div>
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                    {fileImportanceMap[file.id] && (
                      <span
                        className="badge-device"
                        style={{
                          fontSize: 'var(--font-size-xs)',
                          background:
                            fileImportanceMap[file.id] === 'critical'
                              ? 'var(--color-danger)'
                              : fileImportanceMap[file.id] === 'high'
                                ? 'var(--color-warning)'
                                : fileImportanceMap[file.id] === 'medium'
                                  ? 'var(--color-info)'
                                  : 'var(--color-bg-secondary)',
                          color:
                            fileImportanceMap[file.id] === 'low'
                              ? 'var(--color-text-secondary)'
                              : 'white',
                        }}
                      >
                        {fileImportanceMap[file.id]}
                      </span>
                    )}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                    {file.previewAvailable && (
                      <button
                        type="button"
                        className="btn btn-ghost btn-icon btn-sm"
                        title={t('results.preview')}
                        onClick={() => onPreview(file)}
                        style={{
                          color:
                            activePreviewFileId === file.id ? 'var(--color-accent)' : undefined,
                        }}
                      >
                        <Eye size={14} />
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </SectionCard>
  );
}
