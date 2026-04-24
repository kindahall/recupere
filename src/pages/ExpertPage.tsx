import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { Download } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { EmptyState } from '../components/common/EmptyState';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { ExpertEventsPanel } from '../components/expert/ExpertEventsPanel';
import { ExpertMetadataPanel } from '../components/expert/ExpertMetadataPanel';
import { ExpertPartitionsPanel } from '../components/expert/ExpertPartitionsPanel';
import { ExpertPotentialVolumesPanel } from '../components/expert/ExpertPotentialVolumesPanel';
import { HexViewerPanel } from '../components/expert/HexViewerPanel';
import { PageHeader } from '../components/layout/PageHeader';
import { FilePreviewPanel } from '../components/results/FilePreviewPanel';
import { formatBytes } from '../components/results/resultsFormat';
import {
  type DiagnosticData,
  buildRaidAnalysisImage,
  fetchDevices,
  fetchDiagnostic,
  fetchEncryptionInfo,
  fetchExportLogs,
  fetchFileAuxiliaryHexPreview,
  fetchFileAuxiliaryPreview,
  fetchFileHexPreview,
  fetchImportedRecoverySourceStatus,
  fetchRaidCandidates,
  fetchRaidMetadata,
  fetchResults,
  fetchScanLogs,
  generateLabBundle,
  prepareImportedRecoverySource,
  saveFileAuxiliaryPayload,
  saveTechnicalTimelineReport,
  unlockEncryptedDevice,
  validateRaidAnalysisBuild,
} from '../hooks/useIpc';
import { useAppStore } from '../stores/appStore';
import type {
  EncryptionInfo,
  FileHexPreview,
  FilePreview,
  ImportedRecoverySourceStatus,
  RaidAnalysisBuildRequest,
  RaidAnalysisValidation,
  RaidCandidate,
  RaidMetadata,
  RecoveredFile,
} from '../types';
import {
  recommendedImagingProfileForDevice,
  recommendedImagingProfileReasonKeyForDevice,
} from '../utils/imagingProfile';
import {
  buildPotentialVolumeScanConfig,
  sortPotentialVolumeCandidates,
} from '../utils/potentialVolumeRecovery';
import {
  type TechnicalTimelineSeverityFilter,
  type TechnicalTimelineSourceFilter,
  buildTechnicalTimeline,
  filterTechnicalTimeline,
  formatTechnicalTimelineExport,
} from '../utils/technicalTimeline';

type HexInspectableFile = Pick<
  RecoveredFile,
  | 'id'
  | 'name'
  | 'path'
  | 'sizeBytes'
  | 'startOffset'
  | 'isDeleted'
  | 'recoveryMethod'
  | 'resourceFork'
  | 'alternateDataStreams'
>;

type HexTargetOption = {
  key: string;
  kind: 'primary' | 'resource-fork' | 'ads';
  label: string;
  sizeBytes: number;
  sourceOffset?: number;
  streamName?: string;
};

function sanitizeAuxiliaryFilenameComponent(value: string): string {
  const sanitized = value.replace(/[^a-zA-Z0-9._-]+/g, '_').replace(/^[_\.]+|[_\.]+$/g, '');
  return sanitized || 'stream';
}

export function ExpertPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    devices,
    selectedDeviceId,
    activeScanId,
    activeExportId,
    recoveryResult,
    selectedFileIds,
    runtimeCapabilities,
    scanLogs,
    exportLogs,
    setScanLogs,
    setExportLogs,
    setActiveScanId,
    setScanProgress,
    clearScanLogs,
    setRecoveryResult,
    setScanConfig,
    setDevices,
    selectDevice,
  } = useAppStore();
  const device = devices.find((d) => d.id === selectedDeviceId);
  const [scanLogsError, setScanLogsError] = useState<string | null>(null);
  const [exportLogsError, setExportLogsError] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState<TechnicalTimelineSourceFilter>('all');
  const [severityFilter, setSeverityFilter] = useState<TechnicalTimelineSeverityFilter>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [timelineExporting, setTimelineExporting] = useState(false);
  const [timelineExportNotice, setTimelineExportNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [diagnostic, setDiagnostic] = useState<DiagnosticData | null>(null);
  const [diagnosticError, setDiagnosticError] = useState<string | null>(null);
  const [startingPotentialVolumeId, setStartingPotentialVolumeId] = useState<string | null>(null);
  const [hexFiles, setHexFiles] = useState<HexInspectableFile[]>([]);
  const [hexFilesError, setHexFilesError] = useState<string | null>(null);
  const [hexFileId, setHexFileId] = useState<string | null>(null);
  const [hexTargetKey, setHexTargetKey] = useState<string | null>(null);
  const [hexOffsetInput, setHexOffsetInput] = useState('0');
  const [hexPageSize, setHexPageSize] = useState(256);
  const [hexPreview, setHexPreview] = useState<FileHexPreview | null>(null);
  const [hexLoading, setHexLoading] = useState(false);
  const [hexError, setHexError] = useState<string | null>(null);
  const [auxiliaryPreview, setAuxiliaryPreview] = useState<FilePreview | null>(null);
  const [auxiliaryPreviewLoading, setAuxiliaryPreviewLoading] = useState(false);
  const [auxiliaryPreviewError, setAuxiliaryPreviewError] = useState<string | null>(null);
  const [auxiliarySaving, setAuxiliarySaving] = useState(false);
  const [auxiliarySaveNotice, setAuxiliarySaveNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [labBundleExporting, setLabBundleExporting] = useState(false);
  const [labBundleNotice, setLabBundleNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [raidMetadata, setRaidMetadata] = useState<RaidMetadata | null>(null);
  const [raidCandidates, setRaidCandidates] = useState<RaidCandidate[]>([]);
  const [encryptionInfo, setEncryptionInfo] = useState<EncryptionInfo | null>(null);
  const [importedSourceStatus, setImportedSourceStatus] =
    useState<ImportedRecoverySourceStatus | null>(null);
  const [readinessLoading, setReadinessLoading] = useState(false);
  const [readinessNotice, setReadinessNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const [raidBuildLoading, setRaidBuildLoading] = useState(false);
  const [raidMemberOrder, setRaidMemberOrder] = useState<Array<string | null>>([]);
  const [raidStripeSizeInput, setRaidStripeSizeInput] = useState('');
  const [raidDataOffsetInput, setRaidDataOffsetInput] = useState('');
  const [raidValidation, setRaidValidation] = useState<RaidAnalysisValidation | null>(null);
  const [unlockPassword, setUnlockPassword] = useState('');
  const [unlockLoading, setUnlockLoading] = useState(false);
  const hasActiveTechnicalSession = Boolean(activeScanId || activeExportId);
  const combinedTimeline = buildTechnicalTimeline({
    scanLogs,
    activeScanId,
    exportLogs,
    activeExportId,
  });
  const filteredTimeline = filterTechnicalTimeline({
    entries: combinedTimeline,
    sourceFilter,
    severityFilter,
    query: searchQuery,
  });
  const recommendedPotentialVolumeId = diagnostic?.recommendations.find(
    (recommendation) => recommendation.type === 'scan-lost-volume' && recommendation.isRecommended,
  )?.targetPotentialVolumeId;
  const orderedPotentialVolumes = diagnostic
    ? sortPotentialVolumeCandidates(diagnostic.potentialVolumes, recommendedPotentialVolumeId)
    : [];

  const mapHexInspectableFile = useCallback(
    (file: {
      id: string;
      name: string;
      path: string;
      sizeBytes: number;
      startOffset?: number;
      isDeleted?: boolean;
      recoveryMethod: string;
      resourceFork?: RecoveredFile['resourceFork'];
      alternateDataStreams?: RecoveredFile['alternateDataStreams'];
    }): HexInspectableFile => ({
      id: file.id,
      name: file.name,
      path: file.path,
      sizeBytes: file.sizeBytes,
      startOffset: file.startOffset,
      isDeleted: file.isDeleted,
      recoveryMethod: file.recoveryMethod as RecoveredFile['recoveryMethod'],
      resourceFork: file.resourceFork,
      alternateDataStreams: file.alternateDataStreams,
    }),
    [],
  );

  const buildHexTargets = useCallback(
    (file: HexInspectableFile): HexTargetOption[] => {
      const targets: HexTargetOption[] = [
        {
          key: 'primary',
          kind: 'primary',
          label: t('expert.hex_target_primary'),
          sizeBytes: file.sizeBytes,
          sourceOffset: file.startOffset,
        },
      ];

      if (file.resourceFork) {
        targets.push({
          key: 'resource-fork',
          kind: 'resource-fork',
          label: t('expert.hex_target_resource_fork'),
          sizeBytes: file.resourceFork.sizeBytes,
        });
      }

      for (const stream of file.alternateDataStreams ?? []) {
        targets.push({
          key: `ads:${stream.name}`,
          kind: 'ads',
          label: t('expert.hex_target_ads', { name: stream.name }),
          sizeBytes: stream.sizeBytes,
          streamName: stream.name,
        });
      }

      return targets;
    },
    [t],
  );

  const selectedHexFile = hexFiles.find((file) => file.id === hexFileId) ?? null;
  const hexTargets = selectedHexFile ? buildHexTargets(selectedHexFile) : [];
  const selectedHexTarget = hexTargets.find((target) => target.key === hexTargetKey) ?? null;
  const selectedHexTargetKind = selectedHexTarget?.kind ?? null;
  const selectedHexTargetStreamName = selectedHexTarget?.streamName;
  const auxiliaryPreviewAssetUrl = auxiliaryPreview?.assetPath
    ? isTauri()
      ? convertFileSrc(auxiliaryPreview.assetPath)
      : auxiliaryPreview.assetPath
    : undefined;
  const matchingRaidCandidate =
    device == null
      ? null
      : (raidCandidates.find((candidate) =>
          candidate.members.some((member) => member.deviceId === device.id),
        ) ?? null);
  const missingRaidMembers = matchingRaidCandidate
    ? Math.max(
        0,
        matchingRaidCandidate.expectedMemberCount - matchingRaidCandidate.detectedMemberCount,
      )
    : 0;
  const orderedRaidSlots = matchingRaidCandidate
    ? raidMemberOrder.map((memberId, index) => ({
        slotId: `slot-${index}-${memberId ?? 'missing'}`,
        member:
          memberId == null
            ? null
            : (matchingRaidCandidate.members.find((member) => member.deviceId === memberId) ??
              null),
      }))
    : [];
  const assignedRaidMemberIds = new Set(
    raidMemberOrder.filter((memberId): memberId is string => memberId != null),
  );
  const availableRaidMembers = matchingRaidCandidate
    ? matchingRaidCandidate.members.filter((member) => !assignedRaidMemberIds.has(member.deviceId))
    : [];
  const currentRaidBuildRequest: RaidAnalysisBuildRequest | null =
    matchingRaidCandidate == null
      ? null
      : {
          level: matchingRaidCandidate.level,
          orderedMemberDeviceIds: raidMemberOrder,
          stripeSizeBytes: Number(raidStripeSizeInput.trim()),
          dataOffsetBytes: Number(raidDataOffsetInput.trim()),
        };

  const parseHexOffsetInput = useCallback((value: string): number | null => {
    const trimmed = value.trim();
    if (!trimmed) {
      return 0;
    }

    const parsed = Number(trimmed);
    if (!Number.isInteger(parsed) || parsed < 0) {
      return null;
    }

    return parsed;
  }, []);

  const buildAuxiliarySuggestedFilename = (
    file: HexInspectableFile,
    target: HexTargetOption,
  ): string => {
    if (target.kind === 'resource-fork') {
      return `${file.name}.resource-fork.bin`;
    }
    if (target.kind === 'primary') {
      return `${file.name}.bin`;
    }

    return `${file.name}.ads.${sanitizeAuxiliaryFilenameComponent(
      target.streamName ?? 'stream',
    )}.bin`;
  };

  const loadHexPreview = useCallback(
    async (
      nextOffset: number | null = parseHexOffsetInput(hexOffsetInput),
      fileId = hexFileId,
      targetKey = hexTargetKey,
    ) => {
      if (!activeScanId || !fileId) {
        setHexPreview(null);
        return;
      }

      if (nextOffset === null) {
        setHexError(t('expert.hex_invalid_offset'));
        return;
      }

      setHexLoading(true);
      setHexError(null);

      try {
        const file = hexFiles.find((candidate) => candidate.id === fileId);
        if (!file) {
          throw new Error(t('expert.hex_no_file'));
        }

        const target =
          buildHexTargets(file).find((candidate) => candidate.key === targetKey) ??
          buildHexTargets(file)[0];
        if (!target) {
          throw new Error(t('expert.hex_no_file'));
        }

        const preview =
          target.kind === 'primary'
            ? await fetchFileHexPreview(activeScanId, fileId, nextOffset, hexPageSize)
            : await fetchFileAuxiliaryHexPreview(
                activeScanId,
                fileId,
                target.kind,
                nextOffset,
                hexPageSize,
                target.streamName,
              );
        setHexPreview(preview);
        setHexOffsetInput(String(preview.startOffset));
      } catch (err) {
        setHexPreview(null);
        setHexError(err instanceof Error ? err.message : t('expert.hex_failed'));
      } finally {
        setHexLoading(false);
      }
    },
    [
      activeScanId,
      buildHexTargets,
      hexFileId,
      hexFiles,
      hexOffsetInput,
      hexPageSize,
      hexTargetKey,
      parseHexOffsetInput,
      t,
    ],
  );

  const handleSaveAuxiliaryPayload = async () => {
    if (
      !activeScanId ||
      !selectedHexFile ||
      !selectedHexTarget ||
      selectedHexTarget.kind === 'primary'
    ) {
      return;
    }

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setAuxiliarySaveNotice({
        variant: 'warning',
        message: t('expert.aux_save_unavailable'),
      });
      return;
    }

    try {
      setAuxiliarySaving(true);
      setAuxiliarySaveNotice(null);

      const destinationPath = await save({
        defaultPath: buildAuxiliarySuggestedFilename(selectedHexFile, selectedHexTarget),
        filters: [
          {
            name: 'Binary Payload',
            extensions: ['bin'],
          },
        ],
      });

      if (!destinationPath) {
        setAuxiliarySaveNotice({
          variant: 'warning',
          message: t('expert.aux_save_cancelled'),
        });
        return;
      }

      const savedPath = await saveFileAuxiliaryPayload(
        activeScanId,
        selectedHexFile.id,
        selectedHexTarget.kind,
        destinationPath,
        device?.devicePath,
        selectedHexTarget.streamName,
      );

      setAuxiliarySaveNotice({
        variant: 'success',
        message: t('expert.aux_save_success', { path: savedPath }),
      });
    } catch (err) {
      setAuxiliarySaveNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('expert.aux_save_failed'),
      });
    } finally {
      setAuxiliarySaving(false);
    }
  };

  const handleExportFilteredTimeline = async () => {
    if (filteredTimeline.length === 0) {
      return;
    }

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setTimelineExportNotice({
        variant: 'warning',
        message: t('expert.export_filtered_log_unavailable'),
      });
      return;
    }

    const payload = formatTechnicalTimelineExport(filteredTimeline);
    const timestamp = new Date().toISOString().replace(/[:]/g, '-');
    const suggestedPath = `recupere-technical-timeline-${timestamp}.log`;

    try {
      setTimelineExporting(true);
      setTimelineExportNotice(null);

      const destinationPath = await save({
        defaultPath: suggestedPath,
        filters: [
          {
            name: 'Technical Logs',
            extensions: ['log', 'txt'],
          },
        ],
      });

      if (!destinationPath) {
        return;
      }

      const savedPath = await saveTechnicalTimelineReport(
        destinationPath,
        payload,
        device?.devicePath,
      );

      setTimelineExportNotice({
        variant: 'success',
        message: t('expert.export_filtered_log_saved', { path: savedPath }),
      });
    } catch (err) {
      setTimelineExportNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('expert.export_filtered_log_failed'),
      });
    } finally {
      setTimelineExporting(false);
    }
  };

  useEffect(() => {
    if (!selectedDeviceId) {
      setDiagnostic(null);
      setDiagnosticError(null);
      return;
    }

    let cancelled = false;

    const loadDiagnostic = async () => {
      try {
        const nextDiagnostic = await fetchDiagnostic(selectedDeviceId);
        if (!cancelled) {
          setDiagnostic(nextDiagnostic);
          setDiagnosticError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setDiagnostic(null);
          setDiagnosticError(
            err instanceof Error ? err.message : t('expert.potential_volumes_failed'),
          );
        }
      }
    };

    loadDiagnostic();

    return () => {
      cancelled = true;
    };
  }, [selectedDeviceId, t]);

  useEffect(() => {
    if (!runtimeCapabilities?.technicalLogs) {
      return;
    }

    let cancelled = false;

    const poll = async () => {
      if (activeScanId) {
        try {
          const logs = await fetchScanLogs(activeScanId);
          if (!cancelled) {
            setScanLogs(logs);
            setScanLogsError(null);
          }
        } catch (err) {
          if (!cancelled) {
            setScanLogsError(err instanceof Error ? err.message : t('expert.scan_logs_failed'));
          }
        }
      } else if (!cancelled) {
        setScanLogsError(null);
      }

      if (activeExportId) {
        try {
          const logs = await fetchExportLogs(activeExportId);
          if (!cancelled) {
            setExportLogs(logs);
            setExportLogsError(null);
          }
        } catch (err) {
          if (!cancelled) {
            setExportLogsError(err instanceof Error ? err.message : t('expert.export_logs_failed'));
          }
        }
      } else if (!cancelled) {
        setExportLogsError(null);
      }
    };

    if (!hasActiveTechnicalSession) {
      setScanLogsError(null);
      setExportLogsError(null);
      return;
    }

    poll();
    const intervalId = window.setInterval(poll, 1500);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [
    activeExportId,
    activeScanId,
    hasActiveTechnicalSession,
    runtimeCapabilities?.technicalLogs,
    setExportLogs,
    setScanLogs,
    t,
  ]);

  useEffect(() => {
    if (!activeScanId) {
      setHexFiles([]);
      setHexFilesError(null);
      return;
    }

    if (recoveryResult?.scanId === activeScanId) {
      setHexFiles(recoveryResult.files.map(mapHexInspectableFile));
      setHexFilesError(null);
      return;
    }

    let cancelled = false;

    const loadHexFiles = async () => {
      try {
        const files = await fetchResults(activeScanId);
        if (!cancelled) {
          setHexFiles(files.map(mapHexInspectableFile));
          setHexFilesError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setHexFiles([]);
          setHexFilesError(err instanceof Error ? err.message : t('expert.hex_results_failed'));
        }
      }
    };

    loadHexFiles();

    return () => {
      cancelled = true;
    };
  }, [activeScanId, mapHexInspectableFile, recoveryResult, t]);

  useEffect(() => {
    if (hexFiles.length === 0) {
      setHexFileId(null);
      setHexTargetKey(null);
      setHexPreview(null);
      setHexError(null);
      setHexOffsetInput('0');
      return;
    }

    const preferredSelectedId = Array.from(selectedFileIds).find((fileId) =>
      hexFiles.some((file) => file.id === fileId),
    );

    setHexFileId((current) => {
      if (current && hexFiles.some((file) => file.id === current)) {
        return current;
      }

      return preferredSelectedId ?? hexFiles[0].id;
    });
  }, [hexFiles, selectedFileIds]);

  useEffect(() => {
    if (hexTargets.length === 0) {
      setHexTargetKey(null);
      return;
    }

    setHexTargetKey((current) =>
      current && hexTargets.some((target) => target.key === current) ? current : hexTargets[0].key,
    );
  }, [hexTargets]);

  useEffect(() => {
    if (!activeScanId || !hexFileId || !hexTargetKey) {
      setHexPreview(null);
      setHexError(null);
      return;
    }

    setHexOffsetInput('0');
    void loadHexPreview(0, hexFileId, hexTargetKey);
  }, [activeScanId, hexFileId, hexTargetKey, loadHexPreview]);

  useEffect(() => {
    if (
      !activeScanId ||
      !hexFileId ||
      !selectedHexTargetKind ||
      selectedHexTargetKind === 'primary'
    ) {
      setAuxiliaryPreview(null);
      setAuxiliaryPreviewError(null);
      setAuxiliaryPreviewLoading(false);
      setAuxiliarySaveNotice(null);
      return;
    }

    let cancelled = false;

    const loadAuxiliaryPreview = async () => {
      setAuxiliaryPreviewLoading(true);
      setAuxiliaryPreviewError(null);

      try {
        const auxiliaryKind = selectedHexTargetKind;
        const preview = await fetchFileAuxiliaryPreview(
          activeScanId,
          hexFileId,
          auxiliaryKind,
          selectedHexTargetStreamName,
        );
        if (!cancelled) {
          setAuxiliaryPreview(preview);
        }
      } catch (err) {
        if (!cancelled) {
          setAuxiliaryPreview(null);
          setAuxiliaryPreviewError(
            err instanceof Error ? err.message : t('expert.aux_preview_failed'),
          );
        }
      } finally {
        if (!cancelled) {
          setAuxiliaryPreviewLoading(false);
        }
      }
    };

    loadAuxiliaryPreview();

    return () => {
      cancelled = true;
    };
  }, [activeScanId, hexFileId, selectedHexTargetKind, selectedHexTargetStreamName, t]);

  useEffect(() => {
    if (hexFileId === null && hexTargetKey === null) {
      setAuxiliarySaveNotice(null);
      return;
    }

    setAuxiliarySaveNotice(null);
  }, [hexFileId, hexTargetKey]);

  useEffect(() => {
    if (!matchingRaidCandidate) {
      setRaidMemberOrder([]);
      setRaidStripeSizeInput('');
      setRaidDataOffsetInput('');
      setRaidValidation(null);
      return;
    }

    setRaidMemberOrder((current) => {
      const currentSet = new Set(current);
      const candidateIds = matchingRaidCandidate.members.map((member) => member.deviceId);
      const preserved = current.filter(
        (memberId): memberId is string => memberId != null && candidateIds.includes(memberId),
      );
      const missing = candidateIds.filter((memberId) => !currentSet.has(memberId));
      const expectedMissingSlots = Math.max(
        0,
        matchingRaidCandidate.expectedMemberCount - candidateIds.length,
      );
      const nullSlots = Array.from({ length: expectedMissingSlots }, () => null);
      const next = [...preserved, ...missing, ...nullSlots];
      return next.length > 0 ? next : candidateIds;
    });
    setRaidStripeSizeInput(String(matchingRaidCandidate.stripeSizeBytes));
    setRaidDataOffsetInput(String(raidMetadata?.dataOffsetBytes ?? 0));
  }, [matchingRaidCandidate, raidMetadata?.dataOffsetBytes]);

  useEffect(() => {
    if (!device || !matchingRaidCandidate) {
      setRaidValidation(null);
      return;
    }

    const request: RaidAnalysisBuildRequest = {
      level: matchingRaidCandidate.level,
      orderedMemberDeviceIds: raidMemberOrder,
      stripeSizeBytes: Number(raidStripeSizeInput.trim()),
      dataOffsetBytes: Number(raidDataOffsetInput.trim()),
    };

    if (
      !Number.isInteger(request.stripeSizeBytes) ||
      request.stripeSizeBytes <= 0 ||
      !Number.isInteger(request.dataOffsetBytes) ||
      request.dataOffsetBytes < 0
    ) {
      setRaidValidation(null);
      return;
    }

    let cancelled = false;
    const runValidation = async () => {
      try {
        const validation = await validateRaidAnalysisBuild(device.id, request);
        if (!cancelled) {
          setRaidValidation(validation);
        }
      } catch (error) {
        if (!cancelled) {
          setRaidValidation({
            supported: false,
            variant: 'unsupported',
            confidence: 'low',
            missingMemberCount: 0,
            message: error instanceof Error ? error.message : t('expert.raid_validation_failed'),
            warnings: [],
          });
        }
      }
    };

    void runValidation();
    return () => {
      cancelled = true;
    };
  }, [device, matchingRaidCandidate, raidMemberOrder, raidStripeSizeInput, raidDataOffsetInput, t]);

  useEffect(() => {
    if (!device) {
      setRaidMetadata(null);
      setRaidCandidates([]);
      setEncryptionInfo(null);
      setImportedSourceStatus(null);
      return;
    }

    let cancelled = false;
    const loadReadiness = async () => {
      setReadinessLoading(true);
      try {
        const [raidResult, raidCandidatesResult, encryptionResult, importedResult] =
          await Promise.allSettled([
            fetchRaidMetadata(device.id),
            fetchRaidCandidates(device.id),
            fetchEncryptionInfo(device.id),
            fetchImportedRecoverySourceStatus(device.id),
          ]);

        if (cancelled) {
          return;
        }

        setRaidMetadata(raidResult.status === 'fulfilled' ? raidResult.value : null);
        setRaidCandidates(
          raidCandidatesResult.status === 'fulfilled' ? raidCandidatesResult.value : [],
        );
        setEncryptionInfo(encryptionResult.status === 'fulfilled' ? encryptionResult.value : null);
        setImportedSourceStatus(
          importedResult.status === 'fulfilled' ? importedResult.value : null,
        );

        const errors = [raidResult, raidCandidatesResult, encryptionResult, importedResult]
          .filter((result) => result.status === 'rejected')
          .map((result) =>
            result.status === 'rejected'
              ? result.reason instanceof Error
                ? result.reason.message
                : String(result.reason)
              : '',
          )
          .filter(Boolean);

        setReadinessNotice(
          errors.length > 0
            ? {
                variant: 'warning',
                message: errors.join(' | '),
              }
            : null,
        );
      } finally {
        if (!cancelled) {
          setReadinessLoading(false);
        }
      }
    };

    void loadReadiness();
    return () => {
      cancelled = true;
    };
  }, [device]);

  const handlePreviousHexPage = () => {
    const currentOffset = hexPreview?.startOffset ?? parseHexOffsetInput(hexOffsetInput) ?? 0;
    const nextOffset = Math.max(0, currentOffset - hexPageSize);
    setHexOffsetInput(String(nextOffset));
    void loadHexPreview(nextOffset);
  };

  const handleNextHexPage = () => {
    if (!hexPreview) {
      return;
    }

    const nextOffset = hexPreview.startOffset + Math.max(hexPreview.bytesRead, 1);
    setHexOffsetInput(String(nextOffset));
    void loadHexPreview(nextOffset);
  };

  const handleGenerateLabBundle = async () => {
    if (!device) {
      setLabBundleNotice({
        variant: 'warning',
        message: t('expert.lab_bundle_requires_device'),
      });
      return;
    }

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setLabBundleNotice({
        variant: 'warning',
        message: t('expert.lab_bundle_unavailable'),
      });
      return;
    }

    setLabBundleExporting(true);
    setLabBundleNotice(null);
    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        setLabBundleNotice({
          variant: 'warning',
          message: t('expert.lab_bundle_cancelled'),
        });
        return;
      }

      const outputPath = await generateLabBundle(
        device.id,
        selectedPath,
        activeScanId,
        activeExportId,
      );
      setLabBundleNotice({
        variant: 'success',
        message: t('expert.lab_bundle_success', { path: outputPath }),
      });
    } catch (error) {
      setLabBundleNotice({
        variant: 'danger',
        message: error instanceof Error ? error.message : t('expert.lab_bundle_failed'),
      });
    } finally {
      setLabBundleExporting(false);
    }
  };

  const handlePrepareImportedSource = async () => {
    if (!device) {
      return;
    }

    setReadinessLoading(true);
    setReadinessNotice(null);
    try {
      const status = await prepareImportedRecoverySource(device.id);
      setImportedSourceStatus(status);
      setReadinessNotice({
        variant: 'success',
        message: t('expert.imported_prepare_success'),
      });
    } catch (error) {
      setReadinessNotice({
        variant: 'danger',
        message: error instanceof Error ? error.message : t('expert.imported_prepare_failed'),
      });
    } finally {
      setReadinessLoading(false);
    }
  };

  const handleUnlockEncryptedDevice = async () => {
    if (!device || !encryptionInfo?.canUnlock || unlockPassword.trim().length === 0) {
      return;
    }

    setUnlockLoading(true);
    setReadinessNotice(null);
    try {
      const result = await unlockEncryptedDevice(device.id, unlockPassword);
      setUnlockPassword('');
      setReadinessNotice({
        variant: 'success',
        message: t('expert.encryption_unlock_success', { result }),
      });
      const refreshedDevices = await fetchDevices();
      setDevices(refreshedDevices);
      const refreshedEncryption = await fetchEncryptionInfo(device.id);
      setEncryptionInfo(refreshedEncryption);
    } catch (error) {
      setReadinessNotice({
        variant: 'danger',
        message: error instanceof Error ? error.message : t('expert.encryption_unlock_failed'),
      });
    } finally {
      setUnlockLoading(false);
    }
  };

  const handleBuildRaidAnalysisImage = async () => {
    if (!device || !matchingRaidCandidate || !currentRaidBuildRequest) {
      return;
    }

    if (
      !Number.isInteger(currentRaidBuildRequest.stripeSizeBytes) ||
      currentRaidBuildRequest.stripeSizeBytes <= 0
    ) {
      setReadinessNotice({
        variant: 'danger',
        message: t('expert.raid_build_invalid_stripe'),
      });
      return;
    }
    if (
      !Number.isInteger(currentRaidBuildRequest.dataOffsetBytes) ||
      currentRaidBuildRequest.dataOffsetBytes < 0
    ) {
      setReadinessNotice({
        variant: 'danger',
        message: t('expert.raid_build_invalid_offset'),
      });
      return;
    }
    if (raidMemberOrder.length !== matchingRaidCandidate.expectedMemberCount) {
      setReadinessNotice({
        variant: 'danger',
        message: t('expert.raid_build_member_count_mismatch', {
          selected: raidMemberOrder.length,
          expected: matchingRaidCandidate.expectedMemberCount,
        }),
      });
      return;
    }
    if (!raidValidation?.supported) {
      setReadinessNotice({
        variant: 'danger',
        message: raidValidation?.message ?? t('expert.raid_validation_failed'),
      });
      return;
    }

    try {
      setRaidBuildLoading(true);
      const result = await buildRaidAnalysisImage(device.id, currentRaidBuildRequest);
      const refreshedDevices = await fetchDevices();
      setDevices(refreshedDevices);
      selectDevice(result.deviceId);
      setReadinessNotice({
        variant: 'success',
        message: t('expert.raid_build_success', {
          name: result.displayName,
          path: result.path,
        }),
      });
      navigate('/diagnostic');
    } catch (error) {
      setReadinessNotice({
        variant: 'danger',
        message: error instanceof Error ? error.message : t('expert.raid_build_failed'),
      });
    } finally {
      setRaidBuildLoading(false);
    }
  };

  const moveRaidMember = (index: number, direction: -1 | 1) => {
    setRaidMemberOrder((current) => {
      const targetIndex = index + direction;
      if (targetIndex < 0 || targetIndex >= current.length) {
        return current;
      }
      const next = [...current];
      [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
      return next;
    });
  };

  const setRaidSlotMember = (index: number, memberId: string | null) => {
    setRaidMemberOrder((current) => {
      const next = [...current];
      const previous = next[index] ?? null;
      if (previous === memberId) {
        return current;
      }

      if (memberId != null) {
        const existingIndex = next.findIndex(
          (candidateId, candidateIndex) => candidateIndex !== index && candidateId === memberId,
        );
        if (existingIndex >= 0) {
          next[existingIndex] = previous;
        }
      }

      next[index] = memberId;
      return next;
    });
  };

  return (
    <div>
      <PageHeader title={t('expert.title')} subtitle={t('expert.subtitle')} />
      <div className="mb-6">
        <WarningBanner variant="info">{t('safety.banner_expert')}</WarningBanner>
      </div>

      {!device ? (
        <EmptyState title={t('devices.empty')} description={t('devices.empty_desc')} />
      ) : (
        <div className="flex flex-col gap-6">
          {/* Collapsed sections */}
          <div className="flex gap-6">
            <ExpertPartitionsPanel device={device} />

            <ExpertPotentialVolumesPanel
              diagnostic={diagnostic}
              diagnosticError={diagnosticError}
              orderedPotentialVolumes={orderedPotentialVolumes}
              recommendedPotentialVolumeId={recommendedPotentialVolumeId}
              startingPotentialVolumeId={startingPotentialVolumeId}
              onAnalyzeCandidate={(volume) => {
                if (!device) return;
                const imagingProfile =
                  diagnostic?.deviceId === device.id
                    ? diagnostic.imagingProfile
                    : recommendedImagingProfileForDevice(device);
                const imagingProfileReasonKey =
                  diagnostic?.deviceId === device.id
                    ? diagnostic.imagingProfileReasonKey
                    : recommendedImagingProfileReasonKeyForDevice(device);
                setStartingPotentialVolumeId(volume.id);
                setActiveScanId(null);
                setScanProgress(null);
                clearScanLogs();
                setRecoveryResult(null);
                setScanConfig(
                  buildPotentialVolumeScanConfig(device.id, volume, {
                    imagingRequiresElevation: diagnostic?.imagingRequiresElevation ?? false,
                    imagingProfile,
                    imagingProfileReasonKey,
                  }),
                );
                navigate('/scan');
              }}
            />
          </div>

          {/* Device Metadata — visible */}
          <ExpertMetadataPanel device={device} />

          <SectionCard title={t('expert.readiness')} subtitle={t('expert.readiness_subtitle')}>
            <div className="flex flex-col gap-3">
              {readinessNotice && (
                <WarningBanner variant={readinessNotice.variant}>
                  {readinessNotice.message}
                </WarningBanner>
              )}
              <div className="grid grid-3 gap-3">
                <div className="stat-card">
                  <span className="stat-card-label">{t('expert.readiness_raid')}</span>
                  <span className="stat-card-value">
                    {raidMetadata ? raidMetadata.level : t('common.none')}
                  </span>
                </div>
                <div className="stat-card">
                  <span className="stat-card-label">{t('expert.readiness_encryption')}</span>
                  <span className="stat-card-value">
                    {encryptionInfo?.detected
                      ? encryptionInfo.encryptionType
                      : t('expert.readiness_not_detected')}
                  </span>
                </div>
                <div className="stat-card">
                  <span className="stat-card-label">{t('expert.readiness_imported')}</span>
                  <span className="stat-card-value">
                    {importedSourceStatus
                      ? importedSourceStatus.prepared
                        ? t('expert.readiness_prepared')
                        : importedSourceStatus.requiresPreparation
                          ? t('expert.readiness_pending')
                          : t('expert.readiness_ready')
                      : t('common.none')}
                  </span>
                </div>
              </div>

              {raidMetadata && (
                <div className="flex flex-col gap-2">
                  <div className="text-sm text-secondary">
                    {t('expert.readiness_raid_detail', {
                      members: raidMetadata.memberCount,
                      stripe: raidMetadata.stripeSizeBytes ?? 0,
                    })}
                  </div>
                  <div className="text-sm text-secondary">
                    {t('expert.raid_operational_detail', {
                      offset: formatBytes(raidMetadata.dataOffsetBytes ?? 0),
                      superblock: raidMetadata.superblockVersion ?? t('common.unknown'),
                    })}
                  </div>
                </div>
              )}

              {matchingRaidCandidate && (
                <div className="flex flex-col gap-3">
                  <WarningBanner variant={missingRaidMembers > 0 ? 'warning' : 'success'}>
                    {missingRaidMembers > 0
                      ? t('expert.raid_candidate_partial', {
                          detected: matchingRaidCandidate.detectedMemberCount,
                          expected: matchingRaidCandidate.expectedMemberCount,
                          missing: missingRaidMembers,
                        })
                      : t('expert.raid_candidate_complete', {
                          count: matchingRaidCandidate.detectedMemberCount,
                        })}
                  </WarningBanner>
                  <div className="text-sm text-secondary">
                    {t('expert.raid_candidate_signature', {
                      level: matchingRaidCandidate.level,
                      stripe: formatBytes(matchingRaidCandidate.stripeSizeBytes),
                      superblock: matchingRaidCandidate.superblockVersion,
                    })}
                  </div>
                  <div className="flex flex-col gap-1">
                    {matchingRaidCandidate.members.map((member) => (
                      <div
                        key={member.deviceId}
                        className="text-sm text-secondary"
                        style={{
                          padding: 'var(--space-2) var(--space-3)',
                          borderRadius: 'var(--radius-md)',
                          border: '1px solid var(--color-border)',
                          background: 'var(--color-bg-secondary)',
                        }}
                      >
                        <span className="font-medium" style={{ color: 'var(--color-text)' }}>
                          {member.deviceName}
                        </span>
                        {' · '}
                        {member.devicePath}
                      </div>
                    ))}
                  </div>
                  <div className="text-xs text-secondary">
                    {importedSourceStatus?.prepared
                      ? t('expert.raid_next_step_prepared')
                      : importedSourceStatus?.requiresPreparation
                        ? t('expert.raid_next_step_prepare')
                        : missingRaidMembers > 0
                          ? t('expert.raid_next_step_partial')
                          : t('expert.raid_next_step_ready')}
                  </div>
                  {currentRaidBuildRequest && (
                    <div className="flex flex-col gap-2">
                      <div className="text-sm text-secondary">
                        {t('expert.raid_build_editor_title')}
                      </div>
                      <div className="flex flex-col gap-2">
                        {orderedRaidSlots.map((slot, index) => (
                          <div
                            key={slot.slotId}
                            style={{
                              display: 'grid',
                              gridTemplateColumns: 'auto 1fr auto auto',
                              gap: 'var(--space-2)',
                              alignItems: 'center',
                              padding: 'var(--space-2) var(--space-3)',
                              borderRadius: 'var(--radius-md)',
                              border: '1px solid var(--color-border)',
                              background: 'var(--color-bg-secondary)',
                            }}
                          >
                            <span className="text-xs text-secondary">
                              {t('expert.raid_member_position', { position: index + 1 })}
                            </span>
                            <select
                              className="input input-sm"
                              value={slot.member?.deviceId ?? '__missing__'}
                              onChange={(event) =>
                                setRaidSlotMember(
                                  index,
                                  event.target.value === '__missing__' ? null : event.target.value,
                                )
                              }
                              disabled={raidBuildLoading}
                            >
                              {slot.member && (
                                <option value={slot.member.deviceId}>
                                  {slot.member.deviceName}
                                </option>
                              )}
                              {!slot.member && (
                                <option value="__missing__">{t('expert.raid_missing_slot')}</option>
                              )}
                              {availableRaidMembers.map((member) => (
                                <option key={member.deviceId} value={member.deviceId}>
                                  {member.deviceName}
                                </option>
                              ))}
                              {slot.member && (
                                <option value="__missing__">{t('expert.raid_mark_missing')}</option>
                              )}
                            </select>
                            <button
                              type="button"
                              className="btn btn-ghost btn-sm"
                              onClick={() => moveRaidMember(index, -1)}
                              disabled={raidBuildLoading || index === 0}
                            >
                              {t('expert.raid_member_up')}
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost btn-sm"
                              onClick={() => moveRaidMember(index, 1)}
                              disabled={raidBuildLoading || index === orderedRaidSlots.length - 1}
                            >
                              {t('expert.raid_member_down')}
                            </button>
                          </div>
                        ))}
                      </div>
                      <div className="grid grid-2 gap-2">
                        <label className="flex flex-col gap-1 text-sm text-secondary">
                          <span>{t('expert.raid_build_stripe_label')}</span>
                          <input
                            type="number"
                            min={1}
                            className="input input-sm"
                            value={raidStripeSizeInput}
                            onChange={(event) => setRaidStripeSizeInput(event.target.value)}
                            disabled={raidBuildLoading}
                          />
                        </label>
                        <label className="flex flex-col gap-1 text-sm text-secondary">
                          <span>{t('expert.raid_build_offset_label')}</span>
                          <input
                            type="number"
                            min={0}
                            className="input input-sm"
                            value={raidDataOffsetInput}
                            onChange={(event) => setRaidDataOffsetInput(event.target.value)}
                            disabled={raidBuildLoading}
                          />
                        </label>
                      </div>
                      {raidValidation && (
                        <>
                          <WarningBanner
                            variant={
                              raidValidation.variant === 'unsupported'
                                ? 'danger'
                                : raidValidation.variant === 'degraded'
                                  ? 'warning'
                                  : 'success'
                            }
                          >
                            {t('expert.raid_validation_summary', {
                              confidence: raidValidation.confidence,
                              message: raidValidation.message,
                            })}
                          </WarningBanner>
                          {raidValidation.warnings.length > 0 && (
                            <div className="flex flex-col gap-1">
                              {raidValidation.warnings.map((warning) => (
                                <div key={warning} className="text-xs text-secondary">
                                  {warning}
                                </div>
                              ))}
                            </div>
                          )}
                        </>
                      )}
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        onClick={() => void handleBuildRaidAnalysisImage()}
                        disabled={raidBuildLoading || raidValidation?.supported === false}
                      >
                        {raidBuildLoading
                          ? t('expert.raid_build_loading')
                          : t('expert.raid_build_action')}
                      </button>
                      <div className="text-xs text-secondary">{t('expert.raid_build_hint')}</div>
                    </div>
                  )}
                </div>
              )}

              {encryptionInfo && (
                <div className="flex flex-col gap-2">
                  <div className="text-sm text-secondary">{encryptionInfo.message}</div>
                  {encryptionInfo.canUnlock && (
                    <>
                      <WarningBanner variant="warning">
                        {t('expert.encryption_unlock_warning')}
                      </WarningBanner>
                      <div className="flex gap-2">
                        <input
                          type="password"
                          className="input input-sm"
                          style={{ flex: 1 }}
                          value={unlockPassword}
                          onChange={(event) => setUnlockPassword(event.target.value)}
                          placeholder={t('expert.encryption_unlock_placeholder')}
                          disabled={unlockLoading}
                        />
                        <button
                          type="button"
                          className="btn btn-secondary btn-sm"
                          onClick={() => void handleUnlockEncryptedDevice()}
                          disabled={unlockLoading || unlockPassword.trim().length === 0}
                        >
                          {unlockLoading
                            ? t('expert.encryption_unlock_loading')
                            : t('expert.encryption_unlock_action')}
                        </button>
                      </div>
                      <div className="text-xs text-secondary">
                        {t('expert.encryption_unlock_followup')}
                      </div>
                    </>
                  )}
                </div>
              )}

              {importedSourceStatus && (
                <div className="text-sm text-secondary">
                  {importedSourceStatus.analysisPath
                    ? t('expert.imported_analysis_path', {
                        path: importedSourceStatus.analysisPath,
                      })
                    : t('expert.imported_source_path', {
                        path: importedSourceStatus.sourcePath,
                      })}
                </div>
              )}

              {importedSourceStatus?.requiresPreparation && !importedSourceStatus.prepared && (
                <div>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void handlePrepareImportedSource()}
                    disabled={readinessLoading}
                  >
                    {readinessLoading
                      ? t('expert.imported_prepare_loading')
                      : t('expert.imported_prepare_action')}
                  </button>
                </div>
              )}
            </div>
          </SectionCard>

          <SectionCard
            title={t('expert.lab_bundle')}
            subtitle={t('expert.lab_bundle_subtitle')}
            action={
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() => void handleGenerateLabBundle()}
                disabled={labBundleExporting}
              >
                <Download size={14} />
                {labBundleExporting
                  ? t('expert.lab_bundle_exporting')
                  : t('expert.lab_bundle_action')}
              </button>
            }
          >
            <div className="flex flex-col gap-3">
              <p className="text-sm text-secondary" style={{ margin: 0 }}>
                {t('expert.lab_bundle_desc')}
              </p>
              <div className="text-sm text-secondary">
                {t('expert.lab_bundle_device', {
                  name: device.name,
                  path: device.devicePath,
                })}
              </div>
              {labBundleNotice && (
                <WarningBanner variant={labBundleNotice.variant}>
                  {labBundleNotice.message}
                </WarningBanner>
              )}
            </div>
          </SectionCard>

          <SectionCard title={t('expert.hex_viewer')}>
            <div className="flex flex-col gap-4">
              <HexViewerPanel
                files={hexFiles}
                selectedFileId={hexFileId}
                onSelectFile={setHexFileId}
                targets={hexTargets}
                selectedTargetKey={hexTargetKey}
                onSelectTarget={setHexTargetKey}
                offsetInput={hexOffsetInput}
                onOffsetInputChange={setHexOffsetInput}
                pageSize={hexPageSize}
                onPageSizeChange={setHexPageSize}
                onLoad={() => void loadHexPreview()}
                onPrevious={handlePreviousHexPage}
                onNext={handleNextHexPage}
                loading={hexLoading}
                error={hexFilesError ?? hexError}
                preview={hexPreview}
              />

              {selectedHexTarget && selectedHexTarget.kind !== 'primary' && selectedHexFile && (
                <div className="flex flex-col gap-3">
                  <div className="flex items-center justify-between gap-3">
                    <div className="text-sm text-secondary">
                      {t('expert.aux_preview_title', { target: selectedHexTarget.label })}
                    </div>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      onClick={() => void handleSaveAuxiliaryPayload()}
                      disabled={auxiliarySaving}
                    >
                      <Download size={14} />
                      {auxiliarySaving ? t('expert.aux_save_saving') : t('expert.aux_save_action')}
                    </button>
                  </div>
                  {auxiliarySaveNotice && (
                    <WarningBanner variant={auxiliarySaveNotice.variant}>
                      {auxiliarySaveNotice.message}
                    </WarningBanner>
                  )}
                  <FilePreviewPanel
                    file={selectedHexFile}
                    preview={auxiliaryPreview}
                    loading={auxiliaryPreviewLoading}
                    error={auxiliaryPreviewError}
                    assetUrl={auxiliaryPreviewAssetUrl}
                  />
                </div>
              )}
            </div>
          </SectionCard>

          <ExpertEventsPanel
            runtimeCapabilities={runtimeCapabilities}
            hasActiveTechnicalSession={hasActiveTechnicalSession}
            activeScanId={activeScanId}
            activeExportId={activeExportId}
            scanLogsError={scanLogsError}
            exportLogsError={exportLogsError}
            searchQuery={searchQuery}
            setSearchQuery={setSearchQuery}
            sourceFilter={sourceFilter}
            setSourceFilter={setSourceFilter}
            severityFilter={severityFilter}
            setSeverityFilter={setSeverityFilter}
            combinedTimeline={combinedTimeline}
            filteredTimeline={filteredTimeline}
            timelineExporting={timelineExporting}
            timelineExportNotice={timelineExportNotice}
            onExportFilteredTimeline={handleExportFilteredTimeline}
          />
        </div>
      )}
    </div>
  );
}
