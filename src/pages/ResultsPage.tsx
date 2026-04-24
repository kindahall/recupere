import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { openPath } from '@tauri-apps/plugin-opener';
import { Download, FolderSearch } from 'lucide-react';
import { LayoutGrid, Table as TableIcon } from 'lucide-react';
import { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { CustomSelect } from '../components/common/CustomSelect';
import { EmptyState } from '../components/common/EmptyState';
import { ErrorState } from '../components/common/ErrorState';
import { LoadingState } from '../components/common/LoadingState';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { PageHeader } from '../components/layout/PageHeader';
import { AiAnalysisPanel } from '../components/results/AiAnalysisPanel';
import { AiChatPanel } from '../components/results/AiChatPanel';
import { FileGalleryPanel } from '../components/results/FileGalleryPanel';
import { FilePreviewPanel } from '../components/results/FilePreviewPanel';
import { FileTreePanel } from '../components/results/FileTreePanel';
import { ResultsTable } from '../components/results/ResultsTable';
import { ResultsToolbar } from '../components/results/ResultsToolbar';
import {
  classifyRecoveryImportance,
  filterInputStyle,
  getScoreColor,
  parseOptionalMegabytes,
  parseOptionalNumber,
} from '../components/results/resultsFormat';
import { TrackedScansBar } from '../components/scan/TrackedScansBar';
import {
  type RecoveredFileData,
  classifyScanFilesFor,
  exportResultsCsvFor,
  fetchFilePreviewFor,
  fetchRecoveryContextHints,
  fetchResultsFor,
  fetchScanAiBriefFor,
  generateRecoveryReportFor,
  remoteGetFileMediaAsset,
  remotePullRecoveredFile,
  remoteRestore,
} from '../hooks/useIpc';
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts';
import { useSelectedImportedSourceStatus } from '../hooks/useSelectedImportedSourceStatus';
import { useAppStore } from '../stores/appStore';
import type {
  AiRecoveryBrief,
  FileIntegrity,
  FilePreview,
  RecoveredFile,
  RecoveryContextHint,
  RecoveryResult,
} from '../types';
import type { FileTreeFilterMode } from '../utils/fileTree';
import { buildFileTree, filterFilesForTree } from '../utils/fileTree';
import {
  applyRecoveryContextHints,
  buildRecoveryContextLookupInputs,
  collectFilesystemContextRoots,
  countFilesystemMemoryReprioritizedFiles,
  pathsAppearRelated,
} from '../utils/filesystemMemoryContext';
import { isRaidImportedSource } from '../utils/importedSourceKind';
import {
  type ResultsComplexityFilter,
  type ResultsCompressionFilter,
  type ResultsDateField,
  type ResultsSortDirection,
  type ResultsSortKey,
  type ResultsSourceViewFilter,
  type ResultsTypeFilter,
  collectRecoveryFileExtensions,
  filterRecoveryFiles,
  isApfsCatalogPreviewFirstCandidate,
  isApfsCatalogReassembledCandidate,
  sortRecoveryFiles,
} from '../utils/resultFilters';

function normalizeFileIntegrity(integrity: string): FileIntegrity {
  if (
    integrity === 'intact' ||
    integrity === 'partial' ||
    integrity === 'fragmented' ||
    integrity === 'corrupt'
  ) {
    return integrity;
  }

  return 'uncertain';
}

function buildRecoveryResult(scanId: string, files: RecoveredFileData[]): RecoveryResult {
  const totalSizeBytes = files.reduce((sum, file) => sum + file.sizeBytes, 0);
  const mappedFiles: RecoveredFile[] = files.map((file) => ({
    ...file,
    recoveryMethod: file.recoveryMethod as RecoveredFile['recoveryMethod'],
    integrity: normalizeFileIntegrity(file.integrity),
    compressionKind: file.compressionKind as RecoveredFile['compressionKind'],
    sourceView: file.sourceView as RecoveredFile['sourceView'],
    nativeAuxiliaryKind: file.nativeAuxiliaryKind as RecoveredFile['nativeAuxiliaryKind'],
    recoveryComplexity: file.recoveryComplexity as RecoveredFile['recoveryComplexity'],
    validatorStatus: file.validatorStatus as RecoveredFile['validatorStatus'],
  }));

  return {
    scanId,
    totalFiles: files.length,
    intactFiles: files.filter((file) => file.integrity === 'intact').length,
    partialFiles: files.filter((file) => file.integrity === 'partial').length,
    fragmentedFiles: files.filter((file) => file.integrity === 'fragmented').length,
    uncertainFiles: mappedFiles.filter((file) => file.integrity === 'uncertain').length,
    corruptFiles: files.filter((file) => file.integrity === 'corrupt').length,
    totalSizeBytes,
    recoverableSizeBytes: totalSizeBytes,
    files: mappedFiles,
    treeRoot: buildFileTree(mappedFiles),
  };
}

function isBrowserPreviewWithoutDesktop(): boolean {
  return __ALLOW_BROWSER_PREVIEW__ && !isTauri();
}

function joinDestinationPath(root: string, leaf: string): string {
  const trimmedRoot = root.replace(/[\\/]+$/, '');
  if (trimmedRoot.length === 0) {
    return leaf;
  }

  const separator = trimmedRoot.includes('\\') || /^[A-Za-z]:/.test(trimmedRoot) ? '\\' : '/';
  return `${trimmedRoot}${separator}${leaf}`;
}

export function ResultsPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    activeScanId,
    activeRemoteAgentId,
    runtimeCapabilities,
    devices,
    selectedDeviceId,
    scanConfig,
    language,
    recoveryResult,
    setRecoveryResult,
    filesystemMemoryComparison,
    selectedFileIds,
    toggleFileSelection,
    selectAllFiles,
    deselectAllFiles,
  } = useAppStore();

  useKeyboardShortcuts(
    useMemo(
      () => ({
        'mod+a': selectAllFiles,
        escape: deselectAllFiles,
      }),
      [selectAllFiles, deselectAllFiles],
    ),
  );
  const selectedDevice = devices.find((device) => device.id === selectedDeviceId);
  const { status: importedSourceStatus } = useSelectedImportedSourceStatus(selectedDevice);
  const raidImportedSource = isRaidImportedSource(importedSourceStatus, selectedDevice);

  const [viewMode, setViewMode] = useState<'table' | 'gallery'>('table');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filterIntegrity, setFilterIntegrity] = useState<FileIntegrity | 'all'>('all');
  const [resultsQuery, setResultsQuery] = useState('');
  const [typeFilter, setTypeFilter] = useState<ResultsTypeFilter>('all');
  const [sourceViewFilter, setSourceViewFilter] = useState<ResultsSourceViewFilter>('all');
  const [compressionFilter, setCompressionFilter] = useState<ResultsCompressionFilter>('all');
  const [complexityFilter, setComplexityFilter] = useState<ResultsComplexityFilter>('all');
  const [extensionFilter, setExtensionFilter] = useState('');
  const [minSizeMb, setMinSizeMb] = useState('');
  const [maxSizeMb, setMaxSizeMb] = useState('');
  const [minScore, setMinScore] = useState('');
  const [dateField, setDateField] = useState<ResultsDateField>('modifiedAt');
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');
  const [sortKey, setSortKey] = useState<ResultsSortKey>('score');
  const [sortDirection, setSortDirection] = useState<ResultsSortDirection>('desc');
  const [treeQuery, setTreeQuery] = useState('');
  const [treeFilterMode, setTreeFilterMode] = useState<FileTreeFilterMode>('all');
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [activePreviewFileId, setActivePreviewFileId] = useState<string | null>(null);
  const [filePreview, setFilePreview] = useState<FilePreview | null>(null);
  const [aiBrief, setAiBrief] = useState<AiRecoveryBrief | null>(null);
  const [fileImportanceMap, setFileImportanceMap] = useState<Record<string, string>>({});
  const [reportLoading, setReportLoading] = useState<'pdf' | 'csv' | null>(null);
  const [remoteActionLoading, setRemoteActionLoading] = useState<'pull' | 'restore' | null>(null);
  const [remoteRestoreDestination, setRemoteRestoreDestination] = useState('');
  const [reportNotice, setReportNotice] = useState<{
    variant: 'info' | 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [filesystemMemoryHints, setFilesystemMemoryHints] = useState<RecoveryContextHint[]>([]);
  const [filesystemMemoryLoading, setFilesystemMemoryLoading] = useState(false);
  const [filesystemMemoryError, setFilesystemMemoryError] = useState<string | null>(null);
  const browserPreviewWithoutDesktop = isBrowserPreviewWithoutDesktop();

  const openGeneratedArtifact = async (path: string) => {
    if (isTauri()) {
      await openPath(path);
      return;
    }
    window.open(`file://${path}`, '_blank', 'noopener,noreferrer');
  };

  const handleGenerateReport = async () => {
    if (!activeScanId) return;
    try {
      let localDestination = '';
      if (activeRemoteAgentId) {
        if (browserPreviewWithoutDesktop) {
          setReportNotice({
            variant: 'warning',
            message: t('results.remote_report_unavailable'),
          });
          return;
        }

        const destinationPath = await save({
          defaultPath: `recupere-remote-${activeScanId}-report.pdf`,
          filters: [{ name: 'PDF report', extensions: ['pdf'] }],
        });

        if (!destinationPath) {
          setReportNotice({
            variant: 'warning',
            message: t('results.remote_report_cancelled'),
          });
          return;
        }

        localDestination = destinationPath;
      }

      setReportNotice({ variant: 'info', message: t('results.report_generating') });
      setReportLoading('pdf');
      const path = await generateRecoveryReportFor(
        activeScanId,
        language,
        true,
        activeRemoteAgentId,
        localDestination,
      );
      setReportNotice({ variant: 'success', message: t('results.report_success', { path }) });
      await openGeneratedArtifact(path);
    } catch (err) {
      setReportNotice({
        variant: 'danger',
        message: t('results.report_failed', {
          error: err instanceof Error ? err.message : String(err),
        }),
      });
    } finally {
      setReportLoading(null);
    }
  };

  const handleExportCsv = async () => {
    if (!activeScanId) return;
    try {
      let localDestination = '';
      if (activeRemoteAgentId) {
        if (browserPreviewWithoutDesktop) {
          setReportNotice({
            variant: 'warning',
            message: t('results.remote_csv_unavailable'),
          });
          return;
        }

        const destinationPath = await save({
          defaultPath: `recupere-remote-${activeScanId}-results.csv`,
          filters: [{ name: 'CSV file', extensions: ['csv'] }],
        });

        if (!destinationPath) {
          setReportNotice({
            variant: 'warning',
            message: t('results.remote_csv_cancelled'),
          });
          return;
        }

        localDestination = destinationPath;
      }

      setReportNotice({ variant: 'info', message: t('results.csv_generating') });
      setReportLoading('csv');
      const path = await exportResultsCsvFor(activeScanId, activeRemoteAgentId, localDestination);
      setReportNotice({ variant: 'success', message: t('results.csv_success', { path }) });
      await openGeneratedArtifact(path);
    } catch (err) {
      setReportNotice({
        variant: 'danger',
        message: t('results.csv_failed', {
          error: err instanceof Error ? err.message : String(err),
        }),
      });
    } finally {
      setReportLoading(null);
    }
  };

  const deferredResultsQuery = useDeferredValue(resultsQuery);
  const deferredTreeQuery = useDeferredValue(treeQuery);

  useEffect(() => {
    if (!activeScanId) return;
    if (recoveryResult?.scanId === activeScanId) return;

    let cancelled = false;

    const loadResults = async () => {
      setLoading(true);
      setError(null);
      try {
        const files = await fetchResultsFor(activeScanId, activeRemoteAgentId);
        if (!cancelled) {
          setRecoveryResult(buildRecoveryResult(activeScanId, files));
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : t('results.load_failed'));
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    loadResults();

    return () => {
      cancelled = true;
    };
  }, [activeScanId, activeRemoteAgentId, recoveryResult?.scanId, setRecoveryResult, t]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: this reset is intentionally keyed to scan changes.
  useEffect(() => {
    setActivePreviewFileId(null);
    setFilePreview(null);
    setPreviewError(null);
    setPreviewLoading(false);
    setAiBrief(null);
    setFilesystemMemoryHints([]);
    setFilesystemMemoryError(null);
    setFilesystemMemoryLoading(false);
  }, [activeScanId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: this reset is intentionally keyed to remote-agent changes.
  useEffect(() => {
    setRemoteRestoreDestination('');
  }, [activeRemoteAgentId]);

  useEffect(() => {
    if (!activeScanId || !runtimeCapabilities?.aiAdvisory) return;
    if (!recoveryResult || recoveryResult.scanId !== activeScanId) return;
    let cancelled = false;
    fetchScanAiBriefFor(activeScanId, activeRemoteAgentId)
      .then((brief) => {
        if (!cancelled) setAiBrief(brief);
      })
      .catch(() => {
        if (!cancelled) setAiBrief(null);
      });
    return () => {
      cancelled = true;
    };
  }, [activeScanId, activeRemoteAgentId, recoveryResult, runtimeCapabilities?.aiAdvisory]);

  // Auto-classify files for importance display
  useEffect(() => {
    if (!activeScanId || !recoveryResult || recoveryResult.files.length === 0) return;
    let cancelled = false;
    classifyScanFilesFor(activeScanId, activeRemoteAgentId)
      .then((classifications) => {
        if (cancelled) return;
        const map: Record<string, string> = {};
        for (const c of classifications) {
          map[c.fileId] = c.importance;
        }
        setFileImportanceMap(map);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [activeScanId, activeRemoteAgentId, recoveryResult]);

  const filesystemContextRoots = useMemo(
    () => collectFilesystemContextRoots(selectedDevice, importedSourceStatus),
    [importedSourceStatus, selectedDevice],
  );
  const filesystemMemoryApplicableRoot = useMemo(() => {
    if (!filesystemMemoryComparison || activeRemoteAgentId) {
      return null;
    }

    return (
      filesystemContextRoots.find((root) =>
        pathsAppearRelated(root, filesystemMemoryComparison.targetPath),
      ) ?? null
    );
  }, [activeRemoteAgentId, filesystemContextRoots, filesystemMemoryComparison]);
  const filesystemMemoryApplicable = Boolean(filesystemMemoryApplicableRoot);

  useEffect(() => {
    if (
      !activeScanId ||
      !recoveryResult ||
      recoveryResult.scanId !== activeScanId ||
      !filesystemMemoryComparison ||
      !filesystemMemoryApplicable ||
      activeRemoteAgentId
    ) {
      setFilesystemMemoryHints([]);
      setFilesystemMemoryError(null);
      setFilesystemMemoryLoading(false);
      return;
    }

    const recoveredFiles = buildRecoveryContextLookupInputs(recoveryResult.files);
    if (recoveredFiles.length === 0) {
      setFilesystemMemoryHints([]);
      setFilesystemMemoryError(null);
      setFilesystemMemoryLoading(false);
      return;
    }

    let cancelled = false;
    setFilesystemMemoryLoading(true);
    setFilesystemMemoryError(null);

    fetchRecoveryContextHints(
      filesystemMemoryComparison.baselineId,
      filesystemMemoryComparison.headId,
      recoveredFiles,
    )
      .then((hints) => {
        if (!cancelled) {
          setFilesystemMemoryHints(hints);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setFilesystemMemoryHints([]);
          setFilesystemMemoryError(
            err instanceof Error ? err.message : t('results.filesystem_memory_context_error'),
          );
        }
      })
      .finally(() => {
        if (!cancelled) {
          setFilesystemMemoryLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [
    activeRemoteAgentId,
    activeScanId,
    filesystemMemoryApplicable,
    filesystemMemoryComparison,
    recoveryResult,
    t,
  ]);

  const files = useMemo(
    () => applyRecoveryContextHints(recoveryResult?.files ?? [], filesystemMemoryHints),
    [filesystemMemoryHints, recoveryResult?.files],
  );
  const displayFileImportanceMap = useMemo(() => {
    if (files.length === 0) {
      return fileImportanceMap;
    }

    const nextImportanceMap = { ...fileImportanceMap };
    const importanceWeight: Record<string, number> = {
      low: 0,
      medium: 1,
      high: 2,
      critical: 3,
    };

    for (const file of files) {
      if ((file.recoveryScoreAdjustment ?? 0) > 0) {
        nextImportanceMap[file.id] = classifyRecoveryImportance(file.recoveryScore);
      }
      if (isApfsCatalogPreviewFirstCandidate(file)) {
        const currentImportance = nextImportanceMap[file.id];
        if (
          !currentImportance ||
          (importanceWeight[currentImportance] ?? -1) < importanceWeight.medium
        ) {
          nextImportanceMap[file.id] = 'medium';
        }
      }
    }

    return nextImportanceMap;
  }, [fileImportanceMap, files]);
  const filesystemMemoryAdjustedCount = useMemo(
    () => countFilesystemMemoryReprioritizedFiles(files),
    [files],
  );
  const extensionOptions = useMemo(() => collectRecoveryFileExtensions(files), [files]);
  const filteredFiles = useMemo(
    () =>
      filterRecoveryFiles(files, {
        query: deferredResultsQuery,
        integrity: filterIntegrity,
        type: typeFilter,
        sourceView: sourceViewFilter,
        compressionKind: compressionFilter,
        recoveryComplexity: complexityFilter,
        extension: extensionFilter,
        minSizeBytes: parseOptionalMegabytes(minSizeMb),
        maxSizeBytes: parseOptionalMegabytes(maxSizeMb),
        minRecoveryScore: parseOptionalNumber(minScore),
        dateField,
        dateFrom,
        dateTo,
      }),
    [
      dateField,
      dateFrom,
      dateTo,
      deferredResultsQuery,
      extensionFilter,
      files,
      sourceViewFilter,
      compressionFilter,
      complexityFilter,
      filterIntegrity,
      maxSizeMb,
      minScore,
      minSizeMb,
      typeFilter,
    ],
  );
  const sortedFilteredFiles = useMemo(
    () => sortRecoveryFiles(filteredFiles, sortKey, sortDirection),
    [filteredFiles, sortDirection, sortKey],
  );
  const filteredApfsPreviewFirstFiles = useMemo(
    () => sortedFilteredFiles.filter((file) => isApfsCatalogPreviewFirstCandidate(file)),
    [sortedFilteredFiles],
  );
  const filteredApfsReassembledFiles = useMemo(
    () => sortedFilteredFiles.filter((file) => isApfsCatalogReassembledCandidate(file)),
    [sortedFilteredFiles],
  );
  const filteredSaferFiles = useMemo(
    () => sortedFilteredFiles.filter((file) => !isApfsCatalogPreviewFirstCandidate(file)),
    [sortedFilteredFiles],
  );
  const treeFiles = useMemo(
    () =>
      filterFilesForTree(sortedFilteredFiles, {
        query: deferredTreeQuery,
        mode: treeFilterMode,
        selectedFileIds,
      }),
    [deferredTreeQuery, selectedFileIds, sortedFilteredFiles, treeFilterMode],
  );
  const filteredTreeRoot = useMemo(() => buildFileTree(treeFiles), [treeFiles]);
  const counts = useMemo(
    () => ({
      total: files.length,
      intact: files.filter((file) => file.integrity === 'intact').length,
      partial: files.filter((file) => file.integrity === 'partial').length,
      fragmented: files.filter((file) => file.integrity === 'fragmented').length,
      uncertain: files.filter((file) => file.integrity === 'uncertain').length,
      corrupt: files.filter((file) => file.integrity === 'corrupt').length,
      averageScore:
        files.length > 0
          ? Math.round(files.reduce((sum, file) => sum + file.recoveryScore, 0) / files.length)
          : 0,
    }),
    [files],
  );
  const apfsCurrentCatalogCount = useMemo(
    () =>
      files.filter(
        (file) => file.sourceView === 'live-catalog' || file.sourceView === 'mounted-volume',
      ).length,
    [files],
  );
  const apfsSnapshotCount = useMemo(
    () => files.filter((file) => file.sourceView === 'snapshot').length,
    [files],
  );
  const apfsJournalCount = useMemo(
    () =>
      files.filter((file) => file.sourceView === 'journal' || Boolean(file.journalDerived)).length,
    [files],
  );
  const allFilteredSelected =
    sortedFilteredFiles.length > 0 &&
    sortedFilteredFiles.every((file) => selectedFileIds.has(file.id));
  const hasDeletedResults = files.some((file) => file.isDeleted);
  const hasCarvedResults = files.some((file) => file.recoveryMethod === 'carving');
  const recoveredFilesystem = scanConfig?.potentialVolumeFilesystem ?? selectedDevice?.filesystem;
  const apfsOperatorSummaryVisible =
    recoveredFilesystem === 'apfs' || apfsSnapshotCount > 0 || apfsJournalCount > 0;
  const lostVolumeMode =
    scanConfig?.deviceId === selectedDeviceId && scanConfig?.scanType === 'lost-volume';
  const deletedSubtitle =
    recoveredFilesystem === 'ntfs'
      ? t('results.deleted_ntfs_subtitle')
      : recoveredFilesystem === 'exfat'
        ? t('results.deleted_exfat_subtitle')
        : recoveredFilesystem === 'apfs'
          ? t('results.deleted_apfs_subtitle')
          : recoveredFilesystem === 'hfs+'
            ? t('results.deleted_hfsplus_subtitle')
            : recoveredFilesystem === 'ext4'
              ? t('results.deleted_ext4_subtitle')
              : t('results.deleted_fat32_subtitle');
  const deletedNotice =
    recoveredFilesystem === 'ntfs'
      ? t('results.deleted_ntfs_notice')
      : recoveredFilesystem === 'exfat'
        ? t('results.deleted_exfat_notice')
        : recoveredFilesystem === 'apfs'
          ? t('results.deleted_apfs_notice')
          : recoveredFilesystem === 'hfs+'
            ? t('results.deleted_hfsplus_notice')
            : recoveredFilesystem === 'ext4'
              ? t('results.deleted_ext4_notice')
              : t('results.deleted_fat32_notice');
  const pageSubtitle = hasCarvedResults
    ? t('results.carved_subtitle')
    : lostVolumeMode
      ? t('results.lost_volume_subtitle', {
          filesystem: (recoveredFilesystem ?? 'unknown').toUpperCase(),
          label: scanConfig?.potentialVolumeLabel ?? t('scan.lost_volume'),
        })
      : hasDeletedResults
        ? deletedSubtitle
        : raidImportedSource
          ? t('results.raid_source_subtitle')
          : t('results.subtitle');
  const pageNotice = hasCarvedResults
    ? t('results.carved_notice')
    : lostVolumeMode
      ? t('results.lost_volume_notice', {
          filesystem: (recoveredFilesystem ?? 'unknown').toUpperCase(),
          label: scanConfig?.potentialVolumeLabel ?? t('scan.lost_volume'),
        })
      : hasDeletedResults
        ? deletedNotice
        : raidImportedSource
          ? t('results.raid_source_notice')
          : t('results.catalog_notice');
  const raidAnalysisPath = importedSourceStatus?.analysisPath ?? importedSourceStatus?.sourcePath;
  const activePreviewFile = activePreviewFileId
    ? (files.find((file) => file.id === activePreviewFileId) ?? null)
    : null;
  const aiFocusFile =
    activePreviewFile ?? files.find((file) => selectedFileIds.has(file.id)) ?? files[0] ?? null;
  const previewAssetUrl = filePreview?.assetPath
    ? isTauri()
      ? convertFileSrc(filePreview.assetPath)
      : filePreview.assetPath
    : undefined;
  const hasCustomView =
    filterIntegrity !== 'all' ||
    resultsQuery.trim().length > 0 ||
    typeFilter !== 'all' ||
    sourceViewFilter !== 'all' ||
    compressionFilter !== 'all' ||
    complexityFilter !== 'all' ||
    extensionFilter.length > 0 ||
    minSizeMb.trim().length > 0 ||
    maxSizeMb.trim().length > 0 ||
    minScore.trim().length > 0 ||
    dateField !== 'modifiedAt' ||
    dateFrom.length > 0 ||
    dateTo.length > 0 ||
    sortKey !== 'score' ||
    sortDirection !== 'desc' ||
    treeQuery.trim().length > 0 ||
    treeFilterMode !== 'all';
  const exportActionCount =
    selectedFileIds.size > 0 ? selectedFileIds.size : sortedFilteredFiles.length;
  const explicitSelectionActive = selectedFileIds.size > 0;
  const filesystemMemoryWindow =
    filesystemMemoryHints.length > 0
      ? {
          lastObservedAtMs: filesystemMemoryHints[0].lastObservedAtMs,
          firstMissingObservedAtMs: filesystemMemoryHints[0].firstMissingObservedAtMs,
        }
      : null;

  const resetViewFilters = () => {
    setFilterIntegrity('all');
    setResultsQuery('');
    setTypeFilter('all');
    setSourceViewFilter('all');
    setCompressionFilter('all');
    setComplexityFilter('all');
    setExtensionFilter('');
    setMinSizeMb('');
    setMaxSizeMb('');
    setMinScore('');
    setDateField('modifiedAt');
    setDateFrom('');
    setDateTo('');
    setSortKey('score');
    setSortDirection('desc');
    setTreeQuery('');
    setTreeFilterMode('all');
  };

  const toggleAll = () => {
    sortedFilteredFiles.forEach((file) => {
      const isSelected = selectedFileIds.has(file.id);
      if (allFilteredSelected && isSelected) {
        toggleFileSelection(file.id);
      } else if (!allFilteredSelected && !isSelected) {
        toggleFileSelection(file.id);
      }
    });
  };

  const replaceSelection = (targetFiles: RecoveredFile[]) => {
    deselectAllFiles();
    targetFiles.forEach((file) => {
      toggleFileSelection(file.id);
    });
  };

  const handleSelectPrudentBatch = () => {
    replaceSelection(filteredSaferFiles);
  };

  const handleSelectAllFilteredIncludingPreviewFirst = () => {
    replaceSelection(sortedFilteredFiles);
  };

  const handleRemoteDownloadSelected = async () => {
    if (!activeScanId || !activeRemoteAgentId) return;
    const targetIds =
      selectedFileIds.size > 0
        ? Array.from(selectedFileIds)
        : sortedFilteredFiles.map((file) => file.id);
    if (targetIds.length === 0) return;
    if (browserPreviewWithoutDesktop) {
      setReportNotice({
        variant: 'warning',
        message: t('results.remote_download_unavailable'),
      });
      return;
    }

    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        setReportNotice({
          variant: 'warning',
          message: t('results.remote_download_cancelled'),
        });
        return;
      }

      setRemoteActionLoading('pull');
      setReportNotice({
        variant: 'info',
        message: t('results.remote_download_started', {
          count: targetIds.length,
          path: selectedPath,
        }),
      });

      const filesById = new Map((recoveryResult?.files ?? []).map((file) => [file.id, file]));
      let pulled = 0;
      let failed = 0;
      for (const fileId of targetIds) {
        const file = filesById.get(fileId);
        if (!file) {
          failed += 1;
          continue;
        }

        const safeName = file.name.replace(/[^A-Za-z0-9._-]+/g, '_');
        const localPath = joinDestinationPath(selectedPath, `${fileId.slice(0, 8)}-${safeName}`);
        try {
          await remotePullRecoveredFile(activeRemoteAgentId, activeScanId, fileId, localPath);
          pulled += 1;
          setReportNotice({
            variant: 'info',
            message: t('results.remote_download_progress', {
              pulled,
              total: targetIds.length,
              path: selectedPath,
            }),
          });
        } catch (err) {
          failed += 1;
          console.error('remote pull failed for', fileId, err);
        }
      }

      setReportNotice({
        variant: failed > 0 ? 'warning' : 'success',
        message:
          failed > 0
            ? t('results.remote_download_partial', {
                pulled,
                failed,
                path: selectedPath,
              })
            : t('results.remote_download_success', {
                count: pulled,
                path: selectedPath,
              }),
      });
    } catch (err) {
      setReportNotice({
        variant: 'danger',
        message: t('results.remote_download_failed', {
          error: err instanceof Error ? err.message : String(err),
        }),
      });
    } finally {
      setRemoteActionLoading(null);
    }
  };

  const handleRemoteRestore = async () => {
    if (!activeScanId || !activeRemoteAgentId) return;
    if (browserPreviewWithoutDesktop) {
      setReportNotice({
        variant: 'warning',
        message: t('results.remote_restore_unavailable'),
      });
      return;
    }
    const destination = remoteRestoreDestination.trim();
    if (!destination) {
      setReportNotice({
        variant: 'warning',
        message: t('results.remote_restore_destination_required'),
      });
      return;
    }
    const targetIds =
      selectedFileIds.size > 0
        ? Array.from(selectedFileIds)
        : sortedFilteredFiles.map((file) => file.id);
    if (targetIds.length === 0) return;
    setRemoteActionLoading('restore');
    setReportNotice({
      variant: 'info',
      message: t('results.remote_restore_starting', {
        count: targetIds.length,
        destination,
      }),
    });
    try {
      const exportId = await remoteRestore(
        activeRemoteAgentId,
        activeScanId,
        destination,
        targetIds,
      );
      setReportNotice({
        variant: 'success',
        message: t('results.remote_restore_success', {
          exportId,
          destination,
        }),
      });
    } catch (err) {
      setReportNotice({
        variant: 'danger',
        message: t('results.remote_restore_failed', {
          error: err instanceof Error ? err.message : String(err),
        }),
      });
    } finally {
      setRemoteActionLoading(null);
    }
  };

  const handleNavigateToExport = () => {
    if (
      selectedFileIds.size === 0 &&
      sortedFilteredFiles.length > 0 &&
      sortedFilteredFiles.length < files.length
    ) {
      sortedFilteredFiles.forEach((file) => {
        if (!selectedFileIds.has(file.id)) {
          toggleFileSelection(file.id);
        }
      });
    }

    navigate('/export');
  };

  const openPreview = async (file: RecoveredFile) => {
    if (!activeScanId || !file.previewAvailable) return;

    setActivePreviewFileId(file.id);
    setPreviewError(null);
    setPreviewLoading(true);
    setFilePreview(null);

    try {
      const preview = await fetchFilePreviewFor(activeScanId, file.id, activeRemoteAgentId);
      // For remote previews that ship a media asset path (e.g. extracted
      // image/audio/video frame), the path lives on the server. Materialise
      // it locally so the desktop `<video>`/`<img>` element can read it via
      // `convertFileSrc`.
      if (activeRemoteAgentId && preview.assetPath) {
        try {
          const localAsset = await remoteGetFileMediaAsset(
            activeRemoteAgentId,
            activeScanId,
            file.id,
          );
          preview.assetPath = localAsset;
        } catch {
          // If the media materialisation fails, leave the textual preview
          // (if any) and let FilePreviewPanel render what it can.
          preview.assetPath = undefined;
        }
      }
      setFilePreview(preview);
    } catch (err) {
      setFilePreview(null);
      setPreviewError(err instanceof Error ? err.message : t('results.preview_failed'));
    } finally {
      setPreviewLoading(false);
    }
  };

  const handlePreview = async (file: RecoveredFile) => {
    if (!activeScanId || !file.previewAvailable) return;

    if (activePreviewFileId === file.id) {
      setActivePreviewFileId(null);
      setFilePreview(null);
      setPreviewError(null);
      setPreviewLoading(false);
      return;
    }

    await openPreview(file);
  };

  const handleTreeOpenFile = async (file: RecoveredFile) => {
    if (!selectedFileIds.has(file.id)) {
      toggleFileSelection(file.id);
    }

    if (file.previewAvailable) {
      await openPreview(file);
    }
  };

  if (!activeScanId) {
    return (
      <EmptyState
        icon={<FolderSearch className="empty-state-icon" />}
        title={t('results.empty')}
        description={t('results.empty_desc')}
      />
    );
  }

  if (loading) return <LoadingState message={t('common.loading')} />;
  if (error) return <ErrorState title={t('common.error')} description={error} />;
  if (!recoveryResult) return <LoadingState message={t('common.loading')} />;

  return (
    <div>
      <TrackedScansBar />
      <PageHeader
        title={t('results.title')}
        subtitle={pageSubtitle}
        actions={
          <div className="flex gap-2">
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={handleGenerateReport}
              disabled={!activeScanId || reportLoading !== null}
            >
              {reportLoading === 'pdf' ? t('common.loading') : t('results.generate_report')}
            </button>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={handleExportCsv}
              disabled={!activeScanId || reportLoading !== null}
            >
              {reportLoading === 'csv' ? t('common.loading') : t('results.generate_csv')}
            </button>
            {activeRemoteAgentId ? (
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={handleRemoteDownloadSelected}
                disabled={
                  remoteActionLoading !== null ||
                  (selectedFileIds.size === 0 && sortedFilteredFiles.length === 0)
                }
                title={t('results.remote_download_title')}
              >
                <Download size={16} />
                {remoteActionLoading === 'pull'
                  ? t('results.remote_download_loading')
                  : `${t('results.remote_download_button')} (${exportActionCount})`}
              </button>
            ) : (
              runtimeCapabilities?.exportEngine && (
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={handleNavigateToExport}
                  disabled={selectedFileIds.size === 0 && sortedFilteredFiles.length === 0}
                >
                  <Download size={16} />
                  {t('results.export_selected')} ({exportActionCount})
                </button>
              )
            )}
          </div>
        }
      />

      <div className="mb-6">
        <WarningBanner variant="info">{pageNotice}</WarningBanner>
      </div>

      {raidImportedSource && (
        <div className="mb-6">
          <WarningBanner variant="success">
            {t('results.raid_source_ready_notice', {
              path: raidAnalysisPath ?? '—',
            })}
          </WarningBanner>
        </div>
      )}

      {filesystemMemoryLoading && filesystemMemoryApplicable && (
        <div className="mb-6">
          <WarningBanner variant="info">
            {t('results.filesystem_memory_context_loading')}
          </WarningBanner>
        </div>
      )}

      {filesystemMemoryError && filesystemMemoryApplicable && (
        <div className="mb-6">
          <WarningBanner variant="warning">{filesystemMemoryError}</WarningBanner>
        </div>
      )}

      {!filesystemMemoryLoading &&
        !filesystemMemoryError &&
        filesystemMemoryApplicable &&
        filesystemMemoryHints.length > 0 &&
        filesystemMemoryWindow && (
          <div className="mb-6">
            <WarningBanner variant="info">
              {t('results.filesystem_memory_context_summary', {
                count: filesystemMemoryHints.length,
                targetPath:
                  filesystemMemoryComparison?.targetPath ?? filesystemMemoryApplicableRoot,
                lastObserved: new Date(filesystemMemoryWindow.lastObservedAtMs).toLocaleString(),
                firstMissing: filesystemMemoryWindow.firstMissingObservedAtMs
                  ? new Date(filesystemMemoryWindow.firstMissingObservedAtMs).toLocaleString()
                  : '—',
              })}
              {filesystemMemoryAdjustedCount > 0
                ? ` ${t('results.filesystem_memory_context_reprioritized', {
                    count: filesystemMemoryAdjustedCount,
                  })}`
                : ''}
            </WarningBanner>
          </div>
        )}

      {reportNotice && (
        <div className="mb-6">
          <WarningBanner variant={reportNotice.variant}>{reportNotice.message}</WarningBanner>
        </div>
      )}

      {counts.uncertain > 0 && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            {t('results.uncertain_integrity_notice', { count: counts.uncertain })}
          </WarningBanner>
        </div>
      )}

      {apfsOperatorSummaryVisible && (
        <SectionCard
          className="mb-6"
          title={t('results.apfs_operator_title')}
          subtitle={t('results.apfs_operator_subtitle')}
        >
          <div className="grid grid-4 gap-4" style={{ padding: 'var(--space-4) 0' }}>
            <button
              type="button"
              className="stat-card"
              onClick={() => setSourceViewFilter('all')}
              style={{
                cursor: 'pointer',
                borderColor: sourceViewFilter === 'all' ? 'var(--color-accent)' : undefined,
              }}
            >
              <span className="stat-card-label">{t('results.apfs_operator_all')}</span>
              <span className="stat-card-value">{files.length}</span>
            </button>
            <button
              type="button"
              className="stat-card"
              onClick={() => setSourceViewFilter('live-catalog')}
              style={{
                cursor: 'pointer',
                borderColor:
                  sourceViewFilter === 'live-catalog' ? 'var(--color-success)' : undefined,
              }}
            >
              <span className="stat-card-label">{t('results.apfs_operator_current_catalog')}</span>
              <span className="stat-card-value text-success">{apfsCurrentCatalogCount}</span>
            </button>
            <button
              type="button"
              className="stat-card"
              onClick={() => setSourceViewFilter('snapshot')}
              style={{
                cursor: 'pointer',
                borderColor: sourceViewFilter === 'snapshot' ? 'var(--color-warning)' : undefined,
              }}
            >
              <span className="stat-card-label">{t('results.apfs_operator_snapshot')}</span>
              <span className="stat-card-value text-warning">{apfsSnapshotCount}</span>
            </button>
            <button
              type="button"
              className="stat-card"
              onClick={() => setSourceViewFilter('journal')}
              style={{
                cursor: 'pointer',
                borderColor: sourceViewFilter === 'journal' ? 'var(--color-danger)' : undefined,
              }}
            >
              <span className="stat-card-label">{t('results.apfs_operator_journal')}</span>
              <span className="stat-card-value" style={{ color: 'var(--color-danger)' }}>
                {apfsJournalCount}
              </span>
            </button>
          </div>
          <div className="text-sm text-secondary">{t('results.apfs_operator_notice')}</div>
          {filteredApfsReassembledFiles.length > 0 && (
            <div style={{ marginTop: 'var(--space-4)' }}>
              <WarningBanner variant="success">
                {t('results.apfs_reassembled_notice', {
                  count: filteredApfsReassembledFiles.length,
                })}
              </WarningBanner>
            </div>
          )}
        </SectionCard>
      )}

      {activeRemoteAgentId && (
        <div className="mb-6">
          <SectionCard
            title={t('results.remote_restore_panel_title')}
            subtitle={t('results.remote_restore_panel_subtitle', { count: exportActionCount })}
            action={
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={handleRemoteRestore}
                disabled={
                  remoteActionLoading !== null ||
                  remoteRestoreDestination.trim().length === 0 ||
                  (selectedFileIds.size === 0 && sortedFilteredFiles.length === 0)
                }
              >
                <Download size={16} />
                {remoteActionLoading === 'restore'
                  ? t('results.remote_restore_loading')
                  : `${t('results.remote_restore_button')} (${exportActionCount})`}
              </button>
            }
          >
            <label style={{ display: 'grid', gap: 'var(--space-2)' }}>
              <span className="text-sm font-medium">
                {t('results.remote_restore_destination_label')}
              </span>
              <input
                type="text"
                value={remoteRestoreDestination}
                onChange={(event) => setRemoteRestoreDestination(event.target.value)}
                placeholder={t('results.remote_restore_destination_placeholder')}
                style={filterInputStyle}
              />
            </label>
            <p className="text-xs text-secondary" style={{ marginTop: 'var(--space-3)' }}>
              {t('results.remote_restore_destination_help')}
            </p>
          </SectionCard>
        </div>
      )}

      {runtimeCapabilities?.aiAdvisory && aiBrief && (
        <div
          className="mb-6"
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 'var(--space-3)',
            padding: 'var(--space-3) var(--space-4)',
            borderRadius: 'var(--radius-md)',
            background: 'var(--color-bg-secondary)',
            border: '1px solid var(--color-border)',
          }}
        >
          <span className="text-sm font-medium">
            {t('results.summary_line', {
              found: recoveryResult.totalFiles,
              exportable: recoveryResult.totalFiles,
            })}
          </span>
          <span
            style={{
              display: 'inline-block',
              padding: '2px 10px',
              borderRadius: 'var(--radius-sm)',
              fontSize: 'var(--font-size-xs)',
              fontWeight: 600,
              color: 'white',
              background:
                (aiBrief.confidenceScore ?? 0) >= 75
                  ? 'var(--color-success)'
                  : (aiBrief.confidenceScore ?? 0) >= 50
                    ? 'var(--color-warning)'
                    : 'var(--color-danger)',
            }}
          >
            {t('results.ai_confidence', { score: aiBrief.confidenceScore ?? 0 })}
          </span>
        </div>
      )}

      {activeScanId && (
        <div className="mb-6">
          <AiChatPanel scanId={activeScanId} />
        </div>
      )}

      <div
        style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 'var(--space-4)' }}
        className="mb-6"
      >
        <button
          type="button"
          className="stat-card"
          onClick={() => setFilterIntegrity('all')}
          style={{
            cursor: 'pointer',
            borderColor: filterIntegrity === 'all' ? 'var(--color-accent)' : undefined,
          }}
        >
          <span className="stat-card-label">{t('results.total_files')}</span>
          <span className="stat-card-value">{counts.total}</span>
        </button>
        <button
          type="button"
          className="stat-card"
          onClick={() => setFilterIntegrity('intact')}
          style={{
            cursor: 'pointer',
            borderColor: filterIntegrity === 'intact' ? 'var(--color-success)' : undefined,
          }}
        >
          <span className="stat-card-label">{t('results.recoverable')}</span>
          <span className="stat-card-value text-success">{counts.intact}</span>
        </button>
        <button
          type="button"
          className="stat-card"
          onClick={() => setFilterIntegrity('uncertain')}
          style={{
            cursor: 'pointer',
            borderColor: filterIntegrity === 'uncertain' ? 'var(--color-warning)' : undefined,
          }}
        >
          <span className="stat-card-label">{t('results.uncertain')}</span>
          <span className="stat-card-value text-warning">{counts.uncertain}</span>
        </button>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.average_score')}</span>
          <span className="stat-card-value" style={{ color: getScoreColor(counts.averageScore) }}>
            {counts.averageScore}%
          </span>
        </div>
      </div>

      <ResultsToolbar
        resultsQuery={resultsQuery}
        setResultsQuery={setResultsQuery}
        filteredCount={sortedFilteredFiles.length}
        totalCount={files.length}
        hasCustomView={hasCustomView}
        resetViewFilters={resetViewFilters}
        filterIntegrity={filterIntegrity}
        setFilterIntegrity={setFilterIntegrity}
        typeFilter={typeFilter}
        setTypeFilter={setTypeFilter}
        sourceViewFilter={sourceViewFilter}
        setSourceViewFilter={setSourceViewFilter}
        compressionFilter={compressionFilter}
        setCompressionFilter={setCompressionFilter}
        complexityFilter={complexityFilter}
        setComplexityFilter={setComplexityFilter}
        extensionFilter={extensionFilter}
        setExtensionFilter={setExtensionFilter}
        extensionOptions={extensionOptions}
        minSizeMb={minSizeMb}
        setMinSizeMb={setMinSizeMb}
        maxSizeMb={maxSizeMb}
        setMaxSizeMb={setMaxSizeMb}
        minScore={minScore}
        setMinScore={setMinScore}
        dateField={dateField}
        setDateField={setDateField}
        dateFrom={dateFrom}
        setDateFrom={setDateFrom}
        dateTo={dateTo}
        setDateTo={setDateTo}
        sortKey={sortKey}
        setSortKey={setSortKey}
        sortDirection={sortDirection}
        setSortDirection={setSortDirection}
      />

      {filteredApfsPreviewFirstFiles.length > 0 && (
        <SectionCard
          className="mb-6"
          title={t('results.selection_guidance_title')}
          subtitle={t('results.selection_guidance_subtitle', {
            heldBack: filteredApfsPreviewFirstFiles.length,
          })}
        >
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              gap: 'var(--space-3)',
              alignItems: 'center',
              justifyContent: 'space-between',
            }}
          >
            <div className="text-sm text-secondary" style={{ maxWidth: '720px' }}>
              {explicitSelectionActive
                ? t('results.selection_guidance_explicit', {
                    selected: selectedFileIds.size,
                    heldBack: filteredApfsPreviewFirstFiles.length,
                  })
                : t('results.selection_guidance_default', {
                    prudent: filteredSaferFiles.length,
                    heldBack: filteredApfsPreviewFirstFiles.length,
                  })}
            </div>
            <div style={{ display: 'flex', gap: 'var(--space-2)', flexWrap: 'wrap' }}>
              <button
                type="button"
                className="btn btn-secondary"
                onClick={handleSelectPrudentBatch}
                disabled={filteredSaferFiles.length === 0}
              >
                {t('results.selection_guidance_prudent_action', {
                  count: filteredSaferFiles.length,
                })}
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={handleSelectAllFilteredIncludingPreviewFirst}
              >
                {t('results.selection_guidance_include_action', {
                  count: sortedFilteredFiles.length,
                })}
              </button>
            </div>
          </div>
        </SectionCard>
      )}

      <div className="grid grid-2 gap-6 mb-6">
        <SectionCard
          title={t('results.tree_title')}
          subtitle={t('results.tree_subtitle')}
          action={
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'flex-end',
                gap: 'var(--space-2)',
                flexWrap: 'wrap',
              }}
            >
              <span className="text-xs text-secondary">
                {t('results.tree_results_count', {
                  visible: treeFiles.length,
                  total: sortedFilteredFiles.length,
                })}
              </span>
              <input
                type="search"
                value={treeQuery}
                onChange={(event) => setTreeQuery(event.target.value)}
                placeholder={t('results.tree_search_placeholder')}
                style={{
                  ...filterInputStyle,
                  minWidth: 220,
                }}
              />
              <CustomSelect
                value={treeFilterMode}
                onChange={(val) => setTreeFilterMode(val as FileTreeFilterMode)}
                style={{
                  minWidth: 170,
                }}
                options={[
                  { value: 'all', label: t('results.tree_filter_all') },
                  { value: 'deleted', label: t('results.tree_filter_deleted') },
                  { value: 'previewable', label: t('results.tree_filter_previewable') },
                  { value: 'selected', label: t('results.tree_filter_selected') },
                ]}
              />
            </div>
          }
        >
          <FileTreePanel
            root={filteredTreeRoot}
            selectedFileIds={selectedFileIds}
            activePreviewFileId={activePreviewFileId}
            onToggleFileSelection={toggleFileSelection}
            onOpenFile={handleTreeOpenFile}
          />
        </SectionCard>

        <SectionCard title={t('results.preview')} subtitle={t('results.preview_subtitle')}>
          <FilePreviewPanel
            file={activePreviewFile}
            scanId={activeScanId}
            preview={filePreview}
            loading={previewLoading}
            error={previewError}
            assetUrl={previewAssetUrl}
            sourceDevicePath={selectedDevice?.devicePath}
          />
        </SectionCard>
      </div>

      <div className="flex items-center gap-2" style={{ marginBottom: 'var(--space-3)' }}>
        <button
          type="button"
          className={`btn btn-sm ${viewMode === 'table' ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setViewMode('table')}
        >
          <TableIcon size={14} /> {t('results.view_table', 'Tableau')}
        </button>
        <button
          type="button"
          className={`btn btn-sm ${viewMode === 'gallery' ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setViewMode('gallery')}
        >
          <LayoutGrid size={14} /> {t('results.view_gallery', 'Galerie')}
        </button>
      </div>

      {viewMode === 'gallery' && activeScanId ? (
        <SectionCard>
          <FileGalleryPanel
            scanId={activeScanId}
            files={sortedFilteredFiles}
            selectedFileIds={selectedFileIds}
            toggleFileSelection={toggleFileSelection}
            onPreview={handlePreview}
          />
        </SectionCard>
      ) : (
        <ResultsTable
          files={sortedFilteredFiles}
          selectedFileIds={selectedFileIds}
          toggleFileSelection={toggleFileSelection}
          toggleAll={toggleAll}
          allFilteredSelected={allFilteredSelected}
          fileImportanceMap={displayFileImportanceMap}
          activePreviewFileId={activePreviewFileId}
          onPreview={handlePreview}
        />
      )}

      {activeScanId && (
        <details className="mb-6 mt-6">
          <summary
            style={{
              cursor: 'pointer',
              padding: 'var(--space-3) var(--space-4)',
              borderRadius: 'var(--radius-md)',
              background: 'var(--color-bg-secondary)',
              border: '1px solid var(--color-border)',
              fontWeight: 600,
              fontSize: 'var(--font-size-sm)',
            }}
          >
            {t('results.ai_tools_title')}
            <span
              className="text-secondary"
              style={{ fontWeight: 400, marginLeft: 'var(--space-2)' }}
            >
              — {t('results.ai_tools_subtitle')}
            </span>
          </summary>
          <div style={{ padding: 'var(--space-4) 0' }}>
            <AiAnalysisPanel
              scanId={activeScanId}
              focusFile={aiFocusFile}
              onSelectFiles={(ids) => {
                deselectAllFiles();
                ids.forEach((id) => toggleFileSelection(id));
              }}
            />
          </div>
        </details>
      )}
    </div>
  );
}
