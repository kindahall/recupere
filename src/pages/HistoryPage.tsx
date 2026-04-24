import { isTauri } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { RefreshCw, Trash2 } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ConfirmDialog } from '../components/common/ConfirmDialog';
import { EmptyState } from '../components/common/EmptyState';
import { ErrorState } from '../components/common/ErrorState';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { PageHeader } from '../components/layout/PageHeader';
import { ScanLogPanel } from '../components/scan/ScanLogPanel';
import {
  clearLocalHistory,
  fetchAuditTrail,
  fetchExportHistory,
  fetchExportLogs,
  fetchScanHistory,
  fetchScanLogs,
  generateImagingRescueMap,
  generateImagingSessionReport,
  saveSupportBundle,
  saveTechnicalTimelineReport,
} from '../hooks/useIpc';
import { useAppStore } from '../stores/appStore';
import type {
  AuditRecord,
  ExportHistoryEntry,
  ImagingMapRange,
  LocalHistoryPurgeResult,
  ScanLogEntry,
  ScanSession,
} from '../types';
import { buildTechnicalTimeline, formatTechnicalTimelineExport } from '../utils/technicalTimeline';

function formatSessionDate(timestampMs: number): string {
  return new Date(timestampMs).toLocaleString();
}

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${Number.parseFloat((bytes / 1024 ** index).toFixed(1))} ${units[index]}`;
}

function formatOffset(offset: number): string {
  return `0x${offset.toString(16).toUpperCase().padStart(8, '0')}`;
}

function formatUnreadableRange(range: ImagingMapRange): string {
  const endOffset = range.startOffset + Math.max(range.length - 1, 0);
  return `${formatOffset(range.startOffset)} - ${formatOffset(endOffset)}`;
}

function largestUnreadableRanges(ranges: ImagingMapRange[]): ImagingMapRange[] {
  return [...ranges]
    .sort((left, right) => right.length - left.length || left.startOffset - right.startOffset)
    .slice(0, 4);
}

function buildSessionTypeLabel(scanType: string, t: (key: string) => string) {
  if (scanType === 'lost-volume') return t('scan.lost_volume');
  if (scanType === 'carving') return t('scan.deleted');
  if (scanType === 'signature-carving') return t('scan.signature_carving');
  if (scanType === 'image') return t('scan.image');
  if (scanType === 'quick' || scanType === 'deep') return t(`scan.${scanType}`);
  return scanType;
}

function sessionUsesImaging(session: ScanSession): boolean {
  return (
    session.scanType === 'image' ||
    (session.resumeFromBytes ?? 0) > 0 ||
    (session.unreadableBytes ?? 0) > 0
  );
}

type ImagingIncidentSummary = {
  severity: 'info' | 'success' | 'warning' | 'danger';
  statusKey:
    | 'history.imaging_operator_status_clean'
    | 'history.imaging_operator_status_resumed'
    | 'history.imaging_operator_status_rescued'
    | 'history.imaging_operator_status_degraded';
  summaryKey:
    | 'history.imaging_operator_summary_clean'
    | 'history.imaging_operator_summary_resumed'
    | 'history.imaging_operator_summary_rescued'
    | 'history.imaging_operator_summary_degraded';
  nextStepKey:
    | 'history.imaging_operator_next_clean'
    | 'history.imaging_operator_next_resumed'
    | 'history.imaging_operator_next_rescued'
    | 'history.imaging_operator_next_degraded';
};

function buildImagingIncidentSummary(session: ScanSession): ImagingIncidentSummary {
  const unreadableBytes = session.unreadableBytes ?? 0;
  const unreadableRanges = session.unreadableRangesCount ?? 0;
  const rescuedAfterRetryBytes = session.rescuedAfterRetryBytes ?? 0;
  const retryPassesCompleted = session.retryPassesCompleted ?? 0;
  const resumedFromBytes = session.resumeFromBytes ?? 0;

  if (unreadableBytes > 0 || unreadableRanges > 0) {
    return {
      severity: 'danger',
      statusKey: 'history.imaging_operator_status_degraded',
      summaryKey: 'history.imaging_operator_summary_degraded',
      nextStepKey: 'history.imaging_operator_next_degraded',
    };
  }

  if (rescuedAfterRetryBytes > 0 || retryPassesCompleted > 0) {
    return {
      severity: 'warning',
      statusKey: 'history.imaging_operator_status_rescued',
      summaryKey: 'history.imaging_operator_summary_rescued',
      nextStepKey: 'history.imaging_operator_next_rescued',
    };
  }

  if (resumedFromBytes > 0) {
    return {
      severity: 'info',
      statusKey: 'history.imaging_operator_status_resumed',
      summaryKey: 'history.imaging_operator_summary_resumed',
      nextStepKey: 'history.imaging_operator_next_resumed',
    };
  }

  return {
    severity: 'success',
    statusKey: 'history.imaging_operator_status_clean',
    summaryKey: 'history.imaging_operator_summary_clean',
    nextStepKey: 'history.imaging_operator_next_clean',
  };
}

function getAuditDetailString(details: unknown, key: string): string | null {
  if (!details || typeof details !== 'object') {
    return null;
  }
  const value = (details as Record<string, unknown>)[key];
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function getAuditDetailNumber(details: unknown, key: string): number | null {
  if (!details || typeof details !== 'object') {
    return null;
  }
  const value = (details as Record<string, unknown>)[key];
  return typeof value === 'number' ? value : null;
}

function isFilesystemMemoryAuditEvent(event: AuditRecord['event']): boolean {
  return (
    event === 'filesystem_snapshot_captured' ||
    event === 'filesystem_snapshot_failed' ||
    event === 'filesystem_diff_computed' ||
    event === 'filesystem_diff_failed'
  );
}

function buildFilesystemMemoryAuditSummary(
  record: AuditRecord,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const targetPath = getAuditDetailString(record.details, 'target_path') ?? '—';
  const baselineId = getAuditDetailString(record.details, 'baseline_id') ?? '—';
  const headId = getAuditDetailString(record.details, 'head_id') ?? '—';
  const filesIndexed = getAuditDetailNumber(record.details, 'files_indexed');
  const changesCount = getAuditDetailNumber(record.details, 'changes_count');
  const error = getAuditDetailString(record.details, 'error');

  switch (record.event) {
    case 'filesystem_snapshot_captured':
      return t('history.filesystem_memory_snapshot_captured', {
        targetPath,
        filesIndexed: filesIndexed ?? 0,
      });
    case 'filesystem_snapshot_failed':
      return t('history.filesystem_memory_snapshot_failed', {
        targetPath,
        error: error ?? 'unknown error',
      });
    case 'filesystem_diff_computed':
      return t('history.filesystem_memory_diff_computed', {
        baselineId,
        headId,
        changesCount: changesCount ?? 0,
      });
    case 'filesystem_diff_failed':
      return t('history.filesystem_memory_diff_failed', {
        baselineId,
        headId,
        error: error ?? 'unknown error',
      });
    default:
      return record.event;
  }
}

function buildExportSelectionModeLabel(
  entry: ExportHistoryEntry,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (entry.explicitSelection) {
    return t('history.export_selection_mode_explicit');
  }

  if (entry.implicitPreviewFirstExcludedCount > 0) {
    return t('history.export_selection_mode_prudent_with_holdout', {
      count: entry.implicitPreviewFirstExcludedCount,
    });
  }

  return t('history.export_selection_mode_prudent');
}

function buildSourceKindLabel(
  sourceKind: ScanSession['sourceKind'] | ExportHistoryEntry['sourceKind'] | undefined,
  t: (key: string) => string,
): string | null {
  if (!sourceKind) {
    return null;
  }

  switch (sourceKind) {
    case 'raid-analysis':
      return t('history.source_kind_raid_analysis');
    case 'forensic-image':
      return t('history.source_kind_forensic_image');
    case 'virtual-disk':
      return t('history.source_kind_virtual_disk');
    case 'raw-image':
      return t('history.source_kind_raw_image');
    case 'generic-image':
      return t('history.source_kind_generic_image');
    default:
      return sourceKind;
  }
}

export function HistoryPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { devices, scanHistory, selectDevice, setScanConfig, setScanHistory, userMode } =
    useAppStore();
  const isExpert = userMode === 'expert';
  const [error, setError] = useState<string | null>(null);
  const [exportHistory, setExportHistory] = useState<ExportHistoryEntry[]>([]);
  const [filesystemMemoryAudit, setFilesystemMemoryAudit] = useState<AuditRecord[]>([]);
  const [purging, setPurging] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [logsLoading, setLogsLoading] = useState(false);
  const [exportLogsLoading, setExportLogsLoading] = useState(false);
  const [imagingReportExporting, setImagingReportExporting] = useState(false);
  const [imagingMapExporting, setImagingMapExporting] = useState(false);
  const [supportBundleExporting, setSupportBundleExporting] = useState(false);
  const [caseTimelineExporting, setCaseTimelineExporting] = useState(false);
  const [caseSummaryExporting, setCaseSummaryExporting] = useState(false);
  const [purgeNotice, setPurgeNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [imagingReportNotice, setImagingReportNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [imagingMapNotice, setImagingMapNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [supportBundleNotice, setSupportBundleNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [caseTimelineNotice, setCaseTimelineNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [caseSummaryNotice, setCaseSummaryNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [selectedLogs, setSelectedLogs] = useState<ScanLogEntry[]>([]);
  const [logsError, setLogsError] = useState<string | null>(null);
  const [selectedExportId, setSelectedExportId] = useState<string | null>(null);
  const [selectedExportLogs, setSelectedExportLogs] = useState<ScanLogEntry[]>([]);
  const [exportLogsError, setExportLogsError] = useState<string | null>(null);
  const selectedSessionIdRef = useRef<string | null>(null);
  const selectedExportIdRef = useRef<string | null>(null);
  const selectedSession = scanHistory.find((session) => session.id === selectedSessionId) ?? null;
  const selectedSessionDevice = selectedSession
    ? (devices.find((device) => device.id === selectedSession.deviceId) ?? null)
    : null;
  const selectedSessionNeedsPreparation = Boolean(
    selectedSession?.sourceRequiresPreparation && selectedSession.sourcePrepared === false,
  );
  const selectedSessionRequiresDevicesReview =
    !selectedSessionDevice || selectedSessionNeedsPreparation;
  const selectedExport = exportHistory.find((entry) => entry.id === selectedExportId) ?? null;
  const selectedExportScan = selectedExport
    ? (scanHistory.find((session) => session.id === selectedExport.scanId) ?? null)
    : null;
  const selectedExportDevice = selectedExportScan
    ? (devices.find((device) => device.id === selectedExportScan.deviceId) ?? null)
    : null;
  const selectedExportNeedsPreparation = Boolean(
    selectedExport?.sourceRequiresPreparation && selectedExport.sourcePrepared === false,
  );
  const imagingIncidentSummary =
    selectedSession && sessionUsesImaging(selectedSession)
      ? buildImagingIncidentSummary(selectedSession)
      : null;
  const selectedSessionUnreadableRangeHighlights = selectedSession
    ? largestUnreadableRanges(selectedSession.unreadableRanges ?? [])
    : [];

  useEffect(() => {
    selectedSessionIdRef.current = selectedSessionId;
  }, [selectedSessionId]);

  useEffect(() => {
    selectedExportIdRef.current = selectedExportId;
  }, [selectedExportId]);

  const loadLogs = useCallback(
    async (session: ScanSession) => {
      selectedSessionIdRef.current = session.id;
      setSelectedSessionId(session.id);
      setLogsLoading(true);
      try {
        const logs = await fetchScanLogs(session.id);
        setSelectedLogs(logs);
        setLogsError(null);
      } catch (err) {
        setSelectedLogs([]);
        setLogsError(err instanceof Error ? err.message : t('history.load_logs_failed'));
      } finally {
        setLogsLoading(false);
      }
    },
    [t],
  );

  const loadExportLogs = useCallback(
    async (entry: ExportHistoryEntry) => {
      selectedExportIdRef.current = entry.id;
      setSelectedExportId(entry.id);
      setExportLogsLoading(true);
      try {
        const logs = await fetchExportLogs(entry.id);
        setSelectedExportLogs(logs);
        setExportLogsError(null);
      } catch (err) {
        setSelectedExportLogs([]);
        setExportLogsError(
          err instanceof Error ? err.message : t('history.load_export_logs_failed'),
        );
      } finally {
        setExportLogsLoading(false);
      }
    },
    [t],
  );

  const loadHistory = useCallback(async () => {
    setRefreshing(true);
    try {
      const [history, exports, auditTrail] = await Promise.all([
        fetchScanHistory(),
        fetchExportHistory(),
        fetchAuditTrail(),
      ]);
      setScanHistory(history);
      setExportHistory(exports);
      setFilesystemMemoryAudit(
        auditTrail
          .filter((record) => isFilesystemMemoryAuditEvent(record.event))
          .sort((a, b) => b.timestampMs - a.timestampMs)
          .slice(0, 12),
      );
      setError(null);
      const nextSession =
        history.find((session) => session.id === selectedSessionIdRef.current) ?? null;
      const nextExport = exports.find((entry) => entry.id === selectedExportIdRef.current) ?? null;

      if (nextSession) {
        void loadLogs(nextSession);
      } else {
        selectedSessionIdRef.current = null;
        setSelectedSessionId(null);
        setSelectedLogs([]);
        setLogsError(null);
      }

      if (nextExport) {
        void loadExportLogs(nextExport);
      } else {
        selectedExportIdRef.current = null;
        setSelectedExportId(null);
        setSelectedExportLogs([]);
        setExportLogsError(null);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : t('history.empty_desc'));
    } finally {
      setRefreshing(false);
    }
  }, [loadExportLogs, loadLogs, setScanHistory, t]);

  useEffect(() => {
    void loadHistory();
  }, [loadHistory]);

  const buildPurgeMessage = useCallback(
    (result: LocalHistoryPurgeResult) => {
      const summary = t('history.purge_success', {
        scans: result.removedScanRecords,
        exports: result.removedExportRecords,
      });

      if (result.liveScanSessions > 0 || result.liveExportSessions > 0) {
        return `${summary} ${t('history.purge_success_live_notice')}`;
      }

      return summary;
    },
    [t],
  );

  const handleExportImagingReport = useCallback(async () => {
    if (!selectedSession || !sessionUsesImaging(selectedSession)) {
      return;
    }

    setImagingReportNotice(null);

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setImagingReportNotice({
        variant: 'warning',
        message: t('history.imaging_report_unavailable'),
      });
      return;
    }

    setImagingReportExporting(true);
    try {
      const content = await generateImagingSessionReport(selectedSession.id);
      const destinationPath = await save({
        defaultPath: `recupere-imaging-session-${selectedSession.id}.txt`,
        filters: [{ name: 'Text report', extensions: ['txt', 'log'] }],
      });

      if (!destinationPath) {
        setImagingReportNotice({
          variant: 'warning',
          message: t('history.imaging_report_cancelled'),
        });
        return;
      }

      const savedPath = await saveTechnicalTimelineReport(destinationPath, content);
      setImagingReportNotice({
        variant: 'success',
        message: t('history.imaging_report_success', { path: savedPath }),
      });
    } catch (err) {
      setImagingReportNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('history.imaging_report_failed'),
      });
    } finally {
      setImagingReportExporting(false);
    }
  }, [selectedSession, t]);

  const handleExportImagingMap = useCallback(async () => {
    if (!selectedSession || !sessionUsesImaging(selectedSession)) {
      return;
    }

    setImagingMapNotice(null);

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setImagingMapNotice({
        variant: 'warning',
        message: t('history.imaging_map_unavailable'),
      });
      return;
    }

    setImagingMapExporting(true);
    try {
      const content = await generateImagingRescueMap(selectedSession.id);
      const destinationPath = await save({
        defaultPath: `recupere-imaging-session-${selectedSession.id}.map`,
        filters: [{ name: 'Rescue map', extensions: ['map', 'log'] }],
      });

      if (!destinationPath) {
        setImagingMapNotice({
          variant: 'warning',
          message: t('history.imaging_map_cancelled'),
        });
        return;
      }

      const savedPath = await saveTechnicalTimelineReport(destinationPath, content);
      setImagingMapNotice({
        variant: 'success',
        message: t('history.imaging_map_success', { path: savedPath }),
      });
    } catch (err) {
      setImagingMapNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('history.imaging_map_failed'),
      });
    } finally {
      setImagingMapExporting(false);
    }
  }, [selectedSession, t]);

  const handleExportSupportBundle = useCallback(
    async (contextId: string, sourceDevicePath?: string) => {
      setSupportBundleNotice(null);

      if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
        setSupportBundleNotice({
          variant: 'warning',
          message: t('history.support_bundle_unavailable'),
        });
        return;
      }

      setSupportBundleExporting(true);
      try {
        const destinationPath = await save({
          defaultPath: `recupere-support-bundle-${contextId}.zip`,
          filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
        });

        if (!destinationPath) {
          setSupportBundleNotice({
            variant: 'warning',
            message: t('history.support_bundle_cancelled'),
          });
          return;
        }

        await saveSupportBundle(destinationPath, sourceDevicePath);
        setSupportBundleNotice({
          variant: 'success',
          message: t('history.support_bundle_success', { path: destinationPath }),
        });
      } catch (error) {
        setSupportBundleNotice({
          variant: 'warning',
          message: error instanceof Error ? error.message : t('history.support_bundle_failed'),
        });
      } finally {
        setSupportBundleExporting(false);
      }
    },
    [t],
  );

  const handleExportCaseTimelineForSession = useCallback(
    async (session: ScanSession, sourceDevicePath?: string) => {
      setCaseTimelineNotice(null);

      if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
        setCaseTimelineNotice({
          variant: 'warning',
          message: t('history.case_timeline_unavailable'),
        });
        return;
      }

      setCaseTimelineExporting(true);
      try {
        const scanLogs = await fetchScanLogs(session.id);
        const entries = buildTechnicalTimeline({
          activeScanId: session.id,
          scanLogs,
          activeExportId: null,
          exportLogs: [],
        });

        if (entries.length === 0) {
          setCaseTimelineNotice({
            variant: 'warning',
            message: t('history.case_timeline_empty'),
          });
          return;
        }

        const destinationPath = await save({
          defaultPath: `recupere-case-timeline-${session.id}.log`,
          filters: [{ name: 'Technical Logs', extensions: ['log', 'txt'] }],
        });

        if (!destinationPath) {
          setCaseTimelineNotice({
            variant: 'warning',
            message: t('history.case_timeline_cancelled'),
          });
          return;
        }

        const savedPath = await saveTechnicalTimelineReport(
          destinationPath,
          formatTechnicalTimelineExport(entries),
          sourceDevicePath,
        );
        setCaseTimelineNotice({
          variant: 'success',
          message: t('history.case_timeline_success', { path: savedPath }),
        });
      } catch (error) {
        setCaseTimelineNotice({
          variant: 'danger',
          message: error instanceof Error ? error.message : t('history.case_timeline_failed'),
        });
      } finally {
        setCaseTimelineExporting(false);
      }
    },
    [t],
  );

  const handleExportCaseTimelineForExport = useCallback(
    async (entry: ExportHistoryEntry, sourceDevicePath?: string) => {
      setCaseTimelineNotice(null);

      if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
        setCaseTimelineNotice({
          variant: 'warning',
          message: t('history.case_timeline_unavailable'),
        });
        return;
      }

      setCaseTimelineExporting(true);
      try {
        const [scanLogs, exportLogs] = await Promise.all([
          fetchScanLogs(entry.scanId),
          fetchExportLogs(entry.id),
        ]);
        const entries = buildTechnicalTimeline({
          activeScanId: entry.scanId,
          scanLogs,
          activeExportId: entry.id,
          exportLogs,
        });

        if (entries.length === 0) {
          setCaseTimelineNotice({
            variant: 'warning',
            message: t('history.case_timeline_empty'),
          });
          return;
        }

        const destinationPath = await save({
          defaultPath: `recupere-case-timeline-${entry.id}.log`,
          filters: [{ name: 'Technical Logs', extensions: ['log', 'txt'] }],
        });

        if (!destinationPath) {
          setCaseTimelineNotice({
            variant: 'warning',
            message: t('history.case_timeline_cancelled'),
          });
          return;
        }

        const savedPath = await saveTechnicalTimelineReport(
          destinationPath,
          formatTechnicalTimelineExport(entries),
          sourceDevicePath,
        );
        setCaseTimelineNotice({
          variant: 'success',
          message: t('history.case_timeline_success', { path: savedPath }),
        });
      } catch (error) {
        setCaseTimelineNotice({
          variant: 'danger',
          message: error instanceof Error ? error.message : t('history.case_timeline_failed'),
        });
      } finally {
        setCaseTimelineExporting(false);
      }
    },
    [t],
  );

  const handleExportCaseSummaryForSession = useCallback(
    async (session: ScanSession, sourceDevicePath?: string) => {
      setCaseSummaryNotice(null);

      if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
        setCaseSummaryNotice({
          variant: 'warning',
          message: t('history.case_summary_unavailable'),
        });
        return;
      }

      const sourceKindLabel = buildSourceKindLabel(session.sourceKind, t);
      const lines = [
        'Recupere Case Summary',
        '',
        'Case type: scan session',
        `Session ID: ${session.id}`,
        `Started: ${formatSessionDate(session.startedAtMs)}`,
        `Status: ${session.status}`,
        `Scan type: ${buildSessionTypeLabel(session.scanType, t)}`,
        `Device: ${session.deviceName}`,
        `Duration: ${formatDuration(session.durationSeconds)}`,
        `Files found: ${session.scanType === 'image' ? '—' : session.filesFound}`,
        `Files recovered: ${session.filesRecovered}`,
      ];

      if (session.sourceDisplayName) {
        lines.push(`Registered source: ${session.sourceDisplayName}`);
      }
      if (sourceKindLabel) {
        lines.push(`Source kind: ${sourceKindLabel}`);
      }
      if (session.sourceFormat) {
        lines.push(`Source format: ${session.sourceFormat}`);
      }
      if (session.sourceAnalysisPath) {
        lines.push(`Analysis path: ${session.sourceAnalysisPath}`);
      }
      if (session.reconstructedRaidSource) {
        lines.push('Reconstructed RAID analysis source: yes');
      }
      if (session.sourceRequiresPreparation) {
        lines.push(
          `Preparation state: ${session.sourcePrepared ? 'prepared' : 'preparation pending'}`,
        );
      }

      if (sessionUsesImaging(session)) {
        const incidentSummary = buildImagingIncidentSummary(session);
        lines.push('');
        lines.push('Imaging operator summary');
        lines.push(`Operator status: ${t(incidentSummary.statusKey)}`);
        lines.push(
          `What happened: ${t(incidentSummary.summaryKey, {
            bytesCopied: formatBytes(session.bytesCopied ?? 0),
            totalBytes: formatBytes(session.totalBytes ?? 0),
            resumeBytes: formatBytes(session.resumeFromBytes ?? 0),
            unreadableRanges: session.unreadableRangesCount ?? 0,
            unreadableBytes: formatBytes(session.unreadableBytes ?? 0),
            retryPasses: session.retryPassesCompleted ?? 0,
            rescuedBytes: formatBytes(session.rescuedAfterRetryBytes ?? 0),
          })}`,
        );
        lines.push(`Safer next step: ${t(incidentSummary.nextStepKey)}`);
      }

      try {
        setCaseSummaryExporting(true);
        const destinationPath = await save({
          defaultPath: `recupere-case-summary-${session.id}.txt`,
          filters: [{ name: 'Case Summary', extensions: ['txt', 'log'] }],
        });

        if (!destinationPath) {
          setCaseSummaryNotice({
            variant: 'warning',
            message: t('history.case_summary_cancelled'),
          });
          return;
        }

        const savedPath = await saveTechnicalTimelineReport(
          destinationPath,
          lines.join('\n'),
          sourceDevicePath,
        );
        setCaseSummaryNotice({
          variant: 'success',
          message: t('history.case_summary_success', { path: savedPath }),
        });
      } catch (error) {
        setCaseSummaryNotice({
          variant: 'danger',
          message: error instanceof Error ? error.message : t('history.case_summary_failed'),
        });
      } finally {
        setCaseSummaryExporting(false);
      }
    },
    [t],
  );

  const handleExportCaseSummaryForExport = useCallback(
    async (entry: ExportHistoryEntry, sourceDevicePath?: string) => {
      setCaseSummaryNotice(null);

      if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
        setCaseSummaryNotice({
          variant: 'warning',
          message: t('history.case_summary_unavailable'),
        });
        return;
      }

      const sourceKindLabel = buildSourceKindLabel(entry.sourceKind, t);
      const lines = [
        'Recupere Case Summary',
        '',
        'Case type: export session',
        `Export ID: ${entry.id}`,
        `Linked scan ID: ${entry.scanId}`,
        `Started: ${formatSessionDate(entry.startedAtMs)}`,
        `Status: ${entry.status}`,
        `Destination: ${entry.destinationPath}`,
        `Selection mode: ${buildExportSelectionModeLabel(entry, t)}`,
        `Files exported: ${entry.exportedFiles}/${entry.totalFiles}`,
        `Bytes copied: ${formatBytes(entry.exportedBytes)} / ${formatBytes(entry.totalBytes)}`,
      ];

      if (entry.sourceDeviceName) {
        lines.push(`Device: ${entry.sourceDeviceName}`);
      }
      if (entry.sourceDisplayName) {
        lines.push(`Registered source: ${entry.sourceDisplayName}`);
      }
      if (sourceKindLabel) {
        lines.push(`Source kind: ${sourceKindLabel}`);
      }
      if (entry.sourceFormat) {
        lines.push(`Source format: ${entry.sourceFormat}`);
      }
      if (entry.sourceAnalysisPath) {
        lines.push(`Analysis path: ${entry.sourceAnalysisPath}`);
      }
      if (entry.reconstructedRaidSource) {
        lines.push('Reconstructed RAID analysis source: yes');
      }
      if (entry.sourceRequiresPreparation) {
        lines.push(
          `Preparation state: ${entry.sourcePrepared ? 'prepared' : 'preparation pending'}`,
        );
      }
      if (!entry.explicitSelection && entry.implicitPreviewFirstExcludedCount > 0) {
        lines.push(
          `APFS preview-first candidates held out of implicit batch: ${entry.implicitPreviewFirstExcludedCount}`,
        );
      }
      if (entry.errors.length > 0) {
        lines.push('');
        lines.push('Recorded export errors');
        for (const error of entry.errors) {
          lines.push(`- ${error.fileName}: ${error.reason}`);
        }
      }

      try {
        setCaseSummaryExporting(true);
        const destinationPath = await save({
          defaultPath: `recupere-case-summary-${entry.id}.txt`,
          filters: [{ name: 'Case Summary', extensions: ['txt', 'log'] }],
        });

        if (!destinationPath) {
          setCaseSummaryNotice({
            variant: 'warning',
            message: t('history.case_summary_cancelled'),
          });
          return;
        }

        const savedPath = await saveTechnicalTimelineReport(
          destinationPath,
          lines.join('\n'),
          sourceDevicePath,
        );
        setCaseSummaryNotice({
          variant: 'success',
          message: t('history.case_summary_success', { path: savedPath }),
        });
      } catch (error) {
        setCaseSummaryNotice({
          variant: 'danger',
          message: error instanceof Error ? error.message : t('history.case_summary_failed'),
        });
      } finally {
        setCaseSummaryExporting(false);
      }
    },
    [t],
  );

  const handleResumeWorkflow = useCallback(
    (deviceId: string | null, target: 'devices' | 'diagnostic' | 'scan') => {
      if (!deviceId) {
        return;
      }

      selectDevice(deviceId);
      setScanConfig(null);
      navigate(`/${target}`);
    },
    [navigate, selectDevice, setScanConfig],
  );

  const [confirmPurgeOpen, setConfirmPurgeOpen] = useState(false);

  const handleClearLocalHistory = useCallback(async () => {
    setConfirmPurgeOpen(false);
    setPurging(true);
    setPurgeNotice(null);

    try {
      const result = await clearLocalHistory('all');
      await loadHistory();
      setPurgeNotice({
        variant:
          result.liveScanSessions > 0 || result.liveExportSessions > 0 ? 'warning' : 'success',
        message: buildPurgeMessage(result),
      });
    } catch (err) {
      setPurgeNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('history.purge_failed'),
      });
    } finally {
      setPurging(false);
    }
  }, [buildPurgeMessage, loadHistory, t]);

  return (
    <div>
      <PageHeader
        title={t('history.title')}
        subtitle={t('history.subtitle')}
        actions={
          <>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={loadHistory}
              disabled={purging || refreshing}
            >
              <RefreshCw size={14} className={refreshing ? 'animate-spin' : undefined} />
              {refreshing ? t('results.history_refreshing') : t('results.history_refresh')}
            </button>
            <button
              type="button"
              className="btn btn-danger btn-sm"
              onClick={() => setConfirmPurgeOpen(true)}
              disabled={purging}
            >
              <Trash2 size={14} />
              {purging ? t('history.purging') : t('history.purge_button')}
            </button>
            <ConfirmDialog
              open={confirmPurgeOpen}
              title={t('history.purge_confirm_title')}
              description={t('history.purge_confirm')}
              variant="danger"
              confirmLabel={t('history.purge_button')}
              onConfirm={handleClearLocalHistory}
              onCancel={() => setConfirmPurgeOpen(false)}
            />
          </>
        }
      />

      {error && (
        <div className="mb-6">
          <ErrorState title={t('common.error')} description={error} onRetry={loadHistory} />
        </div>
      )}

      <div className="mb-6">
        <WarningBanner variant="info">{t('history.persistence_notice')}</WarningBanner>
      </div>

      <SectionCard
        title={t('history.filesystem_memory_activity_title')}
        subtitle={t('history.filesystem_memory_activity_subtitle')}
        className="mb-6"
      >
        {filesystemMemoryAudit.length === 0 ? (
          <div className="text-sm text-secondary">
            {t('history.filesystem_memory_activity_empty')}
          </div>
        ) : (
          <ul
            style={{
              listStyle: 'none',
              padding: 0,
              margin: 0,
              display: 'flex',
              flexDirection: 'column',
              gap: 'var(--space-3)',
            }}
          >
            {filesystemMemoryAudit.map((record) => (
              <li
                key={record.id}
                style={{
                  padding: 'var(--space-4)',
                  borderRadius: 'var(--radius-lg)',
                  background: 'var(--color-bg-secondary)',
                  border: '1px solid var(--color-border)',
                }}
              >
                <div className="font-medium">{buildFilesystemMemoryAuditSummary(record, t)}</div>
                <div className="text-xs text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                  {formatSessionDate(record.timestampMs)}
                </div>
                {isExpert && (
                  <pre
                    style={{
                      margin: 'var(--space-2) 0 0',
                      fontSize: '0.75rem',
                      whiteSpace: 'pre-wrap',
                      color: 'var(--color-text-secondary)',
                    }}
                  >
                    {JSON.stringify(record.details, null, 2)}
                  </pre>
                )}
              </li>
            ))}
          </ul>
        )}
      </SectionCard>

      {purgeNotice && (
        <div className="mb-6">
          <WarningBanner variant={purgeNotice.variant}>{purgeNotice.message}</WarningBanner>
        </div>
      )}

      {imagingReportNotice && (
        <div className="mb-6">
          <WarningBanner variant={imagingReportNotice.variant}>
            {imagingReportNotice.message}
          </WarningBanner>
        </div>
      )}

      {imagingMapNotice && (
        <div className="mb-6">
          <WarningBanner variant={imagingMapNotice.variant}>
            {imagingMapNotice.message}
          </WarningBanner>
        </div>
      )}

      {supportBundleNotice && (
        <div className="mb-6">
          <WarningBanner variant={supportBundleNotice.variant}>
            {supportBundleNotice.message}
          </WarningBanner>
        </div>
      )}

      {caseTimelineNotice && (
        <div className="mb-6">
          <WarningBanner variant={caseTimelineNotice.variant}>
            {caseTimelineNotice.message}
          </WarningBanner>
        </div>
      )}

      {caseSummaryNotice && (
        <div className="mb-6">
          <WarningBanner variant={caseSummaryNotice.variant}>
            {caseSummaryNotice.message}
          </WarningBanner>
        </div>
      )}

      {scanHistory.length === 0 ? (
        <EmptyState title={t('history.empty')} description={t('history.empty_desc')} />
      ) : (
        <SectionCard>
          <table style={{ width: '100%', borderCollapse: 'collapse' }}>
            <thead>
              <tr style={{ borderBottom: '2px solid var(--color-border)' }}>
                <th
                  style={{
                    padding: 'var(--space-2) var(--space-3)',
                    textAlign: 'left',
                    fontWeight: 600,
                    fontSize: 'var(--font-size-sm)',
                    color: 'var(--color-text-secondary)',
                  }}
                >
                  {t('history.date')}
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
                  {t('history.device')}
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
                  {t('history.type')}
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
                  {t('history.duration')}
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
                  {t('history.files_found')}
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
                  {t('history.status')}
                </th>
              </tr>
            </thead>
            <tbody>
              {scanHistory.map((session) => (
                <tr
                  key={session.id}
                  style={{
                    borderBottom: '1px solid var(--color-border-light)',
                    background:
                      selectedSessionId === session.id ? 'var(--color-accent-subtle)' : undefined,
                    cursor: 'pointer',
                  }}
                  onClick={() => loadLogs(session)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      loadLogs(session);
                    }
                  }}
                  tabIndex={0}
                >
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                    {formatSessionDate(session.startedAtMs)}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }} className="font-medium">
                    {session.deviceName}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                    <span className="badge-device">
                      {buildSessionTypeLabel(session.scanType, t)}
                    </span>
                    {session.reconstructedRaidSource && (
                      <span
                        className="badge badge-risk-low"
                        style={{ marginLeft: 'var(--space-2)' }}
                      >
                        {t('history.raid_analysis_badge')}
                      </span>
                    )}
                    {session.sourceKind && !session.reconstructedRaidSource && (
                      <span
                        className="badge badge-risk-low"
                        style={{ marginLeft: 'var(--space-2)' }}
                      >
                        {buildSourceKindLabel(session.sourceKind, t)}
                      </span>
                    )}
                    {session.sourceRequiresPreparation && session.sourcePrepared === false && (
                      <span
                        className="badge badge-risk-medium"
                        style={{ marginLeft: 'var(--space-2)' }}
                      >
                        {t('history.source_preparation_pending_badge')}
                      </span>
                    )}
                    {(session.resumeFromBytes ?? 0) > 0 && (
                      <span
                        className="badge badge-risk-low"
                        style={{ marginLeft: 'var(--space-2)' }}
                      >
                        {t('history.imaging_resumed_badge')}
                      </span>
                    )}
                    {(session.unreadableBytes ?? 0) > 0 && (
                      <span
                        className="badge badge-risk-medium"
                        style={{ marginLeft: 'var(--space-2)' }}
                      >
                        {t('history.imaging_degraded_badge')}
                      </span>
                    )}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'right' }}>
                    {formatDuration(session.durationSeconds)}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'right' }}>
                    {session.scanType === 'image' ? '—' : session.filesFound}
                  </td>
                  <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                    <span
                      className={`badge badge-risk-${session.status === 'completed' ? 'low' : 'medium'}`}
                    >
                      {session.status}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </SectionCard>
      )}

      {selectedSession && (
        <SectionCard
          title={t('history.session_details')}
          className="mt-6"
          action={
            <div className="flex items-center gap-2">
              {sessionUsesImaging(selectedSession) && (
                <>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => {
                      void handleExportImagingReport();
                    }}
                    disabled={imagingReportExporting || imagingMapExporting}
                  >
                    {imagingReportExporting
                      ? t('history.imaging_report_exporting')
                      : t('history.imaging_report_action')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => {
                      void handleExportImagingMap();
                    }}
                    disabled={imagingReportExporting || imagingMapExporting}
                  >
                    {imagingMapExporting
                      ? t('history.imaging_map_exporting')
                      : t('history.imaging_map_action')}
                  </button>
                </>
              )}
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void handleExportSupportBundle(
                    selectedSession.id,
                    selectedSessionDevice?.devicePath,
                  )
                }
                disabled={supportBundleExporting}
              >
                {supportBundleExporting
                  ? t('history.support_bundle_exporting')
                  : t('history.support_bundle_action')}
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void handleExportCaseTimelineForSession(
                    selectedSession,
                    selectedSessionDevice?.devicePath,
                  )
                }
                disabled={caseTimelineExporting}
              >
                {caseTimelineExporting
                  ? t('history.case_timeline_exporting')
                  : t('history.case_timeline_action')}
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void handleExportCaseSummaryForSession(
                    selectedSession,
                    selectedSessionDevice?.devicePath,
                  )
                }
                disabled={caseSummaryExporting}
              >
                {caseSummaryExporting
                  ? t('history.case_summary_exporting')
                  : t('history.case_summary_action')}
              </button>
              {selectedSessionRequiresDevicesReview ? (
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => handleResumeWorkflow(selectedSessionDevice?.id ?? null, 'devices')}
                  disabled={!selectedSessionDevice}
                >
                  {sessionUsesImaging(selectedSession)
                    ? t('history.imaging_open_devices_action')
                    : t('history.session_open_devices_action')}
                </button>
              ) : (
                <>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() =>
                      handleResumeWorkflow(selectedSessionDevice?.id ?? null, 'diagnostic')
                    }
                  >
                    {sessionUsesImaging(selectedSession)
                      ? t('history.imaging_open_diagnostic_action')
                      : t('history.session_open_diagnostic_action')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => handleResumeWorkflow(selectedSessionDevice?.id ?? null, 'scan')}
                  >
                    {sessionUsesImaging(selectedSession)
                      ? t('history.imaging_open_scan_action')
                      : t('history.session_open_scan_action')}
                  </button>
                </>
              )}
            </div>
          }
        >
          <div className="flex flex-col gap-3">
            <div className="text-sm text-secondary">
              <span className="font-medium">{t('history.selected_session')}:</span>{' '}
              {selectedSession.id}
            </div>
            <div className="text-sm text-secondary">
              <span className="font-medium">{t('history.device')}:</span>{' '}
              {selectedSession.deviceName}
            </div>
            {selectedSession.sourceDisplayName && (
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.source_display_name')}:</span>{' '}
                {selectedSession.sourceDisplayName}
              </div>
            )}
            {selectedSession.sourceKind && (
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.source_kind')}:</span>{' '}
                {buildSourceKindLabel(selectedSession.sourceKind, t)}
              </div>
            )}
            {selectedSession.sourceFormat && (
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.source_format')}:</span>{' '}
                {selectedSession.sourceFormat}
              </div>
            )}
            {selectedSession.sourceAnalysisPath && (
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.source_analysis_path')}:</span>{' '}
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-sm)' }}>
                  {selectedSession.sourceAnalysisPath}
                </span>
              </div>
            )}
            {selectedSession.reconstructedRaidSource && (
              <WarningBanner variant="success">{t('history.raid_analysis_notice')}</WarningBanner>
            )}
            {!selectedSessionDevice && (
              <WarningBanner variant="warning">
                {sessionUsesImaging(selectedSession)
                  ? t('history.imaging_source_missing_notice')
                  : t('history.session_source_missing_notice')}
              </WarningBanner>
            )}
            {selectedSession.sourceRequiresPreparation && (
              <WarningBanner variant={selectedSession.sourcePrepared ? 'info' : 'warning'}>
                {selectedSession.sourcePrepared
                  ? t('history.source_prepared_notice')
                  : t('history.source_preparation_pending_notice')}
              </WarningBanner>
            )}
            <div className="text-sm text-secondary">
              <span className="font-medium">{t('history.type')}:</span>{' '}
              {buildSessionTypeLabel(selectedSession.scanType, t)}
            </div>
            {sessionUsesImaging(selectedSession) && (
              <>
                {imagingIncidentSummary && (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-3)' }}>
                    <WarningBanner variant={imagingIncidentSummary.severity}>
                      <div className="flex flex-col gap-2">
                        <div className="font-medium">{t('history.imaging_operator_title')}</div>
                        <div className="text-sm">
                          <span className="font-medium">
                            {t('history.imaging_operator_status')}:
                          </span>{' '}
                          {t(imagingIncidentSummary.statusKey)}
                        </div>
                        <div className="text-sm">
                          <span className="font-medium">
                            {t('history.imaging_operator_summary_label')}:
                          </span>{' '}
                          {t(imagingIncidentSummary.summaryKey, {
                            bytesCopied: formatBytes(selectedSession.bytesCopied ?? 0),
                            totalBytes: formatBytes(selectedSession.totalBytes ?? 0),
                            resumeBytes: formatBytes(selectedSession.resumeFromBytes ?? 0),
                            unreadableRanges: selectedSession.unreadableRangesCount ?? 0,
                            unreadableBytes: formatBytes(selectedSession.unreadableBytes ?? 0),
                            retryPasses: selectedSession.retryPassesCompleted ?? 0,
                            rescuedBytes: formatBytes(selectedSession.rescuedAfterRetryBytes ?? 0),
                          })}
                        </div>
                        <div className="text-sm">
                          <span className="font-medium">
                            {t('history.imaging_operator_next_label')}:
                          </span>{' '}
                          {t(imagingIncidentSummary.nextStepKey)}
                        </div>
                      </div>
                    </WarningBanner>
                    <div
                      style={{
                        display: 'grid',
                        gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
                        gap: 'var(--space-3)',
                      }}
                    >
                      <div
                        style={{
                          padding: 'var(--space-3)',
                          borderRadius: 'var(--radius-lg)',
                          background: 'var(--color-bg-secondary)',
                          border: '1px solid var(--color-border)',
                        }}
                      >
                        <div className="text-xs text-secondary">
                          {t('history.imaging_operator_tile_source')}
                        </div>
                        <div className="font-medium">
                          {formatBytes(selectedSession.bytesCopied ?? 0)} /{' '}
                          {formatBytes(selectedSession.totalBytes ?? 0)}
                        </div>
                      </div>
                      <div
                        style={{
                          padding: 'var(--space-3)',
                          borderRadius: 'var(--radius-lg)',
                          background: 'var(--color-bg-secondary)',
                          border: '1px solid var(--color-border)',
                        }}
                      >
                        <div className="text-xs text-secondary">
                          {t('history.imaging_operator_tile_rescue')}
                        </div>
                        <div className="font-medium">
                          {t('history.imaging_operator_tile_rescue_value', {
                            passes: selectedSession.retryPassesCompleted ?? 0,
                            bytes: formatBytes(selectedSession.rescuedAfterRetryBytes ?? 0),
                          })}
                        </div>
                      </div>
                      <div
                        style={{
                          padding: 'var(--space-3)',
                          borderRadius: 'var(--radius-lg)',
                          background: 'var(--color-bg-secondary)',
                          border: '1px solid var(--color-border)',
                        }}
                      >
                        <div className="text-xs text-secondary">
                          {t('history.imaging_operator_tile_gaps')}
                        </div>
                        <div className="font-medium">
                          {t('history.imaging_operator_tile_gaps_value', {
                            ranges: selectedSession.unreadableRangesCount ?? 0,
                            bytes: formatBytes(selectedSession.unreadableBytes ?? 0),
                          })}
                        </div>
                      </div>
                    </div>
                  </div>
                )}
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.imaging_bytes_copied')}:</span>{' '}
                  {formatBytes(selectedSession.bytesCopied ?? 0)}
                </div>
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.imaging_total_bytes')}:</span>{' '}
                  {formatBytes(selectedSession.totalBytes ?? 0)}
                </div>
                {(selectedSession.resumeFromBytes ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">{t('history.imaging_resumed_from')}:</span>{' '}
                    {formatBytes(selectedSession.resumeFromBytes ?? 0)}
                  </div>
                )}
                {(selectedSession.unreadableRangesCount ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">{t('history.imaging_unreadable_ranges')}:</span>{' '}
                    {selectedSession.unreadableRangesCount}
                  </div>
                )}
                {(selectedSession.unreadableBytes ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">{t('history.imaging_unreadable_bytes')}:</span>{' '}
                    {formatBytes(selectedSession.unreadableBytes ?? 0)}
                  </div>
                )}
                {(selectedSession.retryPassesCompleted ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">{t('history.imaging_retry_passes')}:</span>{' '}
                    {selectedSession.retryPassesCompleted}
                  </div>
                )}
                {(selectedSession.rescuedAfterRetryBytes ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">
                      {t('history.imaging_rescued_after_retry_bytes')}:
                    </span>{' '}
                    {formatBytes(selectedSession.rescuedAfterRetryBytes ?? 0)}
                  </div>
                )}
                {(selectedSession.unreadableRanges?.length ?? 0) > 0 && (
                  <div className="text-sm text-secondary">
                    <span className="font-medium">{t('history.imaging_map_ranges')}:</span>{' '}
                    {selectedSession.unreadableRanges?.length}
                  </div>
                )}
                {selectedSessionUnreadableRangeHighlights.length > 0 && (
                  <div className="mt-4">
                    <div className="text-sm font-medium mb-2">
                      {t('history.imaging_range_sample_title')}
                    </div>
                    <div className="flex flex-col gap-2">
                      {selectedSessionUnreadableRangeHighlights.map((range) => (
                        <div
                          key={`${range.startOffset}-${range.length}`}
                          style={{
                            padding: 'var(--space-3)',
                            borderRadius: 'var(--radius-lg)',
                            background: 'var(--color-bg-secondary)',
                            border: '1px solid var(--color-border)',
                          }}
                        >
                          <div className="flex items-center justify-between gap-4">
                            <div className="text-sm font-medium">
                              {formatUnreadableRange(range)}
                            </div>
                            <div className="text-sm">{formatBytes(range.length)}</div>
                          </div>
                          <div className="text-xs text-secondary mt-1">
                            {t('history.imaging_range_sample_hint')}
                          </div>
                        </div>
                      ))}
                    </div>
                    {(selectedSession.unreadableRanges?.length ?? 0) >
                      selectedSessionUnreadableRangeHighlights.length && (
                      <div className="text-xs text-secondary mt-2">
                        {t('history.imaging_range_sample_more', {
                          shown: selectedSessionUnreadableRangeHighlights.length,
                          total: selectedSession.unreadableRanges?.length ?? 0,
                        })}
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
        </SectionCard>
      )}

      {scanHistory.length > 0 && (
        <SectionCard title={t('history.session_logs')} className="mt-6">
          <div className="flex flex-col gap-3">
            {selectedSessionId && (
              <div className="text-sm text-secondary flex items-center gap-2">
                <span className="font-medium">{t('history.selected_session')}:</span>{' '}
                {selectedSessionId}
                {logsLoading && <RefreshCw size={12} className="animate-spin" />}
              </div>
            )}

            {logsError ? (
              <ErrorState title={t('common.error')} description={logsError} />
            ) : (
              <ScanLogPanel
                logs={selectedLogs}
                emptyMessage={
                  selectedSessionId
                    ? logsLoading
                      ? t('common.loading')
                      : t('history.log_waiting')
                    : t('history.no_session_selected')
                }
              />
            )}
          </div>
        </SectionCard>
      )}

      <div className="mt-6">
        {exportHistory.length === 0 ? (
          <EmptyState
            title={t('history.export_empty')}
            description={t('history.export_empty_desc')}
          />
        ) : (
          <SectionCard title={t('history.exports_title')}>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ borderBottom: '2px solid var(--color-border)' }}>
                  <th
                    style={{
                      padding: 'var(--space-2) var(--space-3)',
                      textAlign: 'left',
                      fontWeight: 600,
                      fontSize: 'var(--font-size-sm)',
                      color: 'var(--color-text-secondary)',
                    }}
                  >
                    {t('history.date')}
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
                    {t('history.export_destination')}
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
                    {t('history.files_exported')}
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
                    {t('history.bytes_copied')}
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
                    {t('history.status')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {exportHistory.map((entry) => (
                  <tr
                    key={entry.id}
                    style={{
                      borderBottom: '1px solid var(--color-border-light)',
                      background:
                        selectedExportId === entry.id ? 'var(--color-accent-subtle)' : undefined,
                      cursor: 'pointer',
                    }}
                    onClick={() => loadExportLogs(entry)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        loadExportLogs(entry);
                      }
                    }}
                    tabIndex={0}
                  >
                    <td style={{ padding: 'var(--space-2) var(--space-3)' }}>
                      {formatSessionDate(entry.startedAtMs)}
                    </td>
                    <td
                      style={{ padding: 'var(--space-2) var(--space-3)' }}
                      className="font-medium"
                    >
                      {entry.destinationPath}
                      <div
                        className="text-xs text-secondary"
                        style={{ marginTop: 'var(--space-1)' }}
                      >
                        {entry.reconstructedRaidSource
                          ? t('history.raid_analysis_badge')
                          : (buildSourceKindLabel(entry.sourceKind, t) ??
                            t('history.source_kind_unknown'))}
                      </div>
                    </td>
                    <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'right' }}>
                      {entry.exportedFiles}/{entry.totalFiles}
                    </td>
                    <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'right' }}>
                      {formatBytes(entry.exportedBytes)}
                    </td>
                    <td style={{ padding: 'var(--space-2) var(--space-3)', textAlign: 'center' }}>
                      <span
                        className={`badge badge-risk-${entry.status === 'completed' ? 'low' : entry.status === 'error' ? 'critical' : 'medium'}`}
                      >
                        {entry.status}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </SectionCard>
        )}
      </div>

      {selectedExportId && (
        <SectionCard
          title={t('history.export_details')}
          className="mt-6"
          action={
            selectedExport ? (
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() =>
                    void handleExportSupportBundle(
                      selectedExport.id,
                      selectedExportDevice?.devicePath,
                    )
                  }
                  disabled={supportBundleExporting}
                >
                  {supportBundleExporting
                    ? t('history.support_bundle_exporting')
                    : t('history.support_bundle_action')}
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() =>
                    void handleExportCaseTimelineForExport(
                      selectedExport,
                      selectedExportDevice?.devicePath,
                    )
                  }
                  disabled={caseTimelineExporting}
                >
                  {caseTimelineExporting
                    ? t('history.case_timeline_exporting')
                    : t('history.case_timeline_action')}
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() =>
                    void handleExportCaseSummaryForExport(
                      selectedExport,
                      selectedExportDevice?.devicePath,
                    )
                  }
                  disabled={caseSummaryExporting}
                >
                  {caseSummaryExporting
                    ? t('history.case_summary_exporting')
                    : t('history.case_summary_action')}
                </button>
                {!selectedExportDevice || selectedExportNeedsPreparation ? (
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() =>
                      handleResumeWorkflow(selectedExportDevice?.id ?? null, 'devices')
                    }
                    disabled={!selectedExportDevice}
                  >
                    {t('history.export_open_devices_action')}
                  </button>
                ) : (
                  <>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      onClick={() => handleResumeWorkflow(selectedExportDevice.id, 'diagnostic')}
                    >
                      {t('history.export_open_diagnostic_action')}
                    </button>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      onClick={() => handleResumeWorkflow(selectedExportDevice.id, 'scan')}
                    >
                      {t('history.export_open_scan_action')}
                    </button>
                  </>
                )}
              </div>
            ) : undefined
          }
        >
          {!selectedExport ? (
            <div className="text-sm text-secondary">{t('history.export_details_missing')}</div>
          ) : (
            <div className="flex flex-col gap-3">
              <div className="text-sm text-secondary flex items-center gap-2">
                <span className="font-medium">{t('history.selected_export')}:</span>{' '}
                {selectedExport.id}
                {exportLogsLoading && <RefreshCw size={12} className="animate-spin" />}
              </div>
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.export_destination')}:</span>{' '}
                {selectedExport.destinationPath}
              </div>
              {selectedExport.sourceDeviceName && (
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.device')}:</span>{' '}
                  {selectedExport.sourceDeviceName}
                </div>
              )}
              {selectedExport.sourceDisplayName && (
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.source_display_name')}:</span>{' '}
                  {selectedExport.sourceDisplayName}
                </div>
              )}
              {selectedExport.sourceKind && (
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.source_kind')}:</span>{' '}
                  {buildSourceKindLabel(selectedExport.sourceKind, t)}
                </div>
              )}
              {selectedExport.sourceFormat && (
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.source_format')}:</span>{' '}
                  {selectedExport.sourceFormat}
                </div>
              )}
              {selectedExport.sourceAnalysisPath && (
                <div className="text-sm text-secondary">
                  <span className="font-medium">{t('history.source_analysis_path')}:</span>{' '}
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-sm)' }}>
                    {selectedExport.sourceAnalysisPath}
                  </span>
                </div>
              )}
              {selectedExport.reconstructedRaidSource && (
                <WarningBanner variant="success">{t('history.raid_analysis_notice')}</WarningBanner>
              )}
              {!selectedExportDevice && (
                <WarningBanner variant="warning">
                  {t('history.export_source_missing_notice')}
                </WarningBanner>
              )}
              {selectedExport.sourceRequiresPreparation && (
                <WarningBanner variant={selectedExport.sourcePrepared ? 'info' : 'warning'}>
                  {selectedExport.sourcePrepared
                    ? t('history.source_prepared_notice')
                    : t('history.source_preparation_pending_notice')}
                </WarningBanner>
              )}
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.export_selection_mode')}:</span>{' '}
                {buildExportSelectionModeLabel(selectedExport, t)}
              </div>
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.bytes_copied')}:</span>{' '}
                {formatBytes(selectedExport.exportedBytes)} /{' '}
                {formatBytes(selectedExport.totalBytes)}
              </div>
              {!selectedExport.explicitSelection &&
                selectedExport.implicitPreviewFirstExcludedCount > 0 && (
                  <WarningBanner variant="warning">
                    {t('history.export_holdout_notice', {
                      count: selectedExport.implicitPreviewFirstExcludedCount,
                    })}
                  </WarningBanner>
                )}
              {selectedExport.errors.length === 0 ? (
                <div className="text-sm text-secondary">{t('history.export_errors_empty')}</div>
              ) : (
                <div className="flex flex-col gap-2">
                  {selectedExport.errors.map((error, index) => (
                    <div key={`${error.fileId}-${index}`} className="text-sm text-secondary">
                      <span className="font-medium">{error.fileName}</span>: {error.reason}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </SectionCard>
      )}

      {exportHistory.length > 0 && (
        <SectionCard title={t('history.export_log')} className="mt-6">
          <div className="flex flex-col gap-3">
            {selectedExportId && (
              <div className="text-sm text-secondary">
                <span className="font-medium">{t('history.selected_export')}:</span>{' '}
                {selectedExportId}
              </div>
            )}

            {exportLogsError ? (
              <ErrorState title={t('common.error')} description={exportLogsError} />
            ) : (
              <ScanLogPanel
                logs={selectedExportLogs}
                emptyMessage={
                  selectedExportId
                    ? t('history.export_log_waiting')
                    : t('history.no_export_selected')
                }
              />
            )}
          </div>
        </SectionCard>
      )}
    </div>
  );
}
