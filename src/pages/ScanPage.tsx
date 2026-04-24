import { isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { FileSearch, History, Play } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ConfirmDialog } from '../components/common/ConfirmDialog';
import { EmptyState } from '../components/common/EmptyState';
import { ErrorState } from '../components/common/ErrorState';
import { LoadingState } from '../components/common/LoadingState';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { ImportedSourceStatusPanel } from '../components/device/ImportedSourceStatusPanel';
import { PageHeader } from '../components/layout/PageHeader';
import { ProgressTimeline } from '../components/scan/ProgressTimeline';
import { ScanLogPanel } from '../components/scan/ScanLogPanel';
import { TrackedScansBar } from '../components/scan/TrackedScansBar';
import {
  cancelScanFor,
  fetchDiagnostic,
  fetchEncryptionInfo,
  fetchScanHistory,
  generateImagingRescueMap,
  generateImagingSessionReport,
  pauseScanFor,
  resumeScanFor,
  saveTechnicalTimelineReport,
  startImaging,
  startPotentialVolumeScan,
  startScan,
} from '../hooks/useIpc';
import { useSelectedImportedSourceStatus } from '../hooks/useSelectedImportedSourceStatus';
import { useAppStore } from '../stores/appStore';
import type { EncryptionInfo, ImagingMapRange, ScanLogEntry, ScanStage, ScanType } from '../types';
import {
  recommendedImagingProfileForDevice,
  recommendedImagingProfileReasonKeyForDevice,
} from '../utils/imagingProfile';
import { isRaidImportedSource } from '../utils/importedSourceKind';

const stages: ScanStage[] = [
  'initializing',
  'creating-image',
  'reading-partition-table',
  'analyzing-filesystem',
  'scanning-deleted-entries',
  'carving-signatures',
  'scoring-results',
  'finalizing',
];

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

function buildSuggestedImageName(deviceName: string): string {
  const sanitizedName = deviceName
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  const dayStamp = new Date().toISOString().slice(0, 10);
  return `recupere-${sanitizedName || 'source'}-${dayStamp}.img`;
}

function getPathBasename(path: string): string {
  return path.split(/[/\\]/).pop() || path;
}

function collectImagingIncidentHighlights(logs: ScanLogEntry[]): ScanLogEntry[] {
  return logs
    .filter((log) =>
      /imaging|rescue|unreadable|zero-fill|resume|retry|administrator approval|elevation/i.test(
        log.message,
      ),
    )
    .slice(-5)
    .reverse();
}

function formatOffset(offset: number): string {
  return `0x${offset.toString(16).toUpperCase().padStart(8, '0')}`;
}

function formatRangeLabel(range: ImagingMapRange): string {
  const endOffset = range.startOffset + Math.max(range.length - 1, 0);
  return `${formatOffset(range.startOffset)} - ${formatOffset(endOffset)}`;
}

function buildUnreadableRangeHighlights(ranges: ImagingMapRange[]): ImagingMapRange[] {
  return [...ranges]
    .sort((left, right) => right.length - left.length || left.startOffset - right.startOffset)
    .slice(0, 4);
}

function scanTypeAllowedOnLockedEncryptedView(scanType: ScanType): boolean {
  return scanType === 'image';
}

export function ScanPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    selectedDeviceId,
    devices,
    runtimeCapabilities,
    activeScanId,
    setActiveScanId,
    activeRemoteAgentId,
    trackScan,
    scanConfig,
    setScanConfig,
    scanProgress,
    setScanProgress,
    scanLogs,
    clearScanLogs,
    setRecoveryResult,
    setScanHistory,
    userMode,
  } = useAppStore();

  const selectedDevice = devices.find((device) => device.id === selectedDeviceId);
  const isRemoteScan = Boolean(activeRemoteAgentId && activeScanId);
  const [error, setError] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [confirmCancelOpen, setConfirmCancelOpen] = useState(false);
  const [imagingReportExporting, setImagingReportExporting] = useState(false);
  const [imagingMapExporting, setImagingMapExporting] = useState(false);
  const [encryptionInfo, setEncryptionInfo] = useState<EncryptionInfo | null>(null);
  const [encryptionLoading, setEncryptionLoading] = useState(false);
  const [imagingExportNotice, setImagingExportNotice] = useState<{
    variant: 'success' | 'warning' | 'danger';
    message: string;
  } | null>(null);
  const autoStartTriedRef = useRef(false);
  const syncedHistoryScanIdRef = useRef<string | null>(null);
  const {
    status: importedSourceStatus,
    loading: importedSourceLoading,
    preparing: importedSourcePreparing,
    error: importedSourceError,
    prepare: prepareImportedSourceFromScan,
    isImportedSource,
  } = useSelectedImportedSourceStatus(selectedDevice);
  const scanImportedNeedsPreparation =
    isImportedSource &&
    !importedSourceLoading &&
    Boolean(
      importedSourceStatus?.sourceAvailable &&
        importedSourceStatus.requiresPreparation &&
        !importedSourceStatus.prepared,
    );
  const scanImportedSourceMissing =
    isImportedSource && !importedSourceLoading && importedSourceStatus?.sourceAvailable === false;
  const scanImportedSourceUnsupported =
    isImportedSource &&
    !importedSourceLoading &&
    importedSourceStatus?.supportTier === 'unsupported';
  const scanImportedGateActive =
    !isRemoteScan &&
    !activeScanId &&
    !scanProgress &&
    isImportedSource &&
    !importedSourceLoading &&
    (scanImportedSourceUnsupported ||
      scanImportedNeedsPreparation ||
      scanImportedSourceMissing ||
      (!importedSourceStatus && Boolean(importedSourceError)));
  const raidImportedSource = isRaidImportedSource(importedSourceStatus, selectedDevice);
  const catalogScanAvailable = selectedDevice ? selectedDevice.type !== 'image' : false;
  const deletedRecoveryAvailable =
    selectedDevice?.filesystem === 'ntfs' ||
    selectedDevice?.filesystem === 'fat32' ||
    selectedDevice?.filesystem === 'exfat' ||
    selectedDevice?.filesystem === 'ext4' ||
    selectedDevice?.filesystem === 'hfs+' ||
    selectedDevice?.filesystem === 'apfs';
  const imagingAvailable = runtimeCapabilities?.imagingEngine ?? false;
  const activeScanType = scanConfig?.deviceId === selectedDeviceId ? scanConfig.scanType : null;
  const imageMode = activeScanType === 'image';
  const deletedRecoveryMode = activeScanType === 'carving';
  const signatureCarvingMode = activeScanType === 'signature-carving';
  const lostVolumeMode = activeScanType === 'lost-volume';
  const reconstructionMode = activeScanType === 'reconstruction';
  const encryptionLockedScanGate =
    encryptionInfo?.workflowState === 'pre_unlock_blocked' && !activeScanId && !scanProgress;
  const lostVolumeFilesystem = scanConfig?.potentialVolumeFilesystem ?? selectedDevice?.filesystem;
  const lostVolumeLabel = scanConfig?.potentialVolumeLabel ?? t('scan.lost_volume');
  const deletedRecoveryLabel =
    selectedDevice?.filesystem === 'ntfs'
      ? t('scan.deleted_ntfs')
      : selectedDevice?.filesystem === 'exfat'
        ? t('scan.deleted_exfat')
        : selectedDevice?.filesystem === 'apfs'
          ? t('scan.deleted_apfs')
          : selectedDevice?.filesystem === 'hfs+'
            ? t('scan.deleted_hfsplus')
            : selectedDevice?.filesystem === 'ext4'
              ? t('scan.deleted_ext4')
              : t('scan.deleted_fat32');
  const deletedRecoverySubtitle =
    selectedDevice?.filesystem === 'ntfs'
      ? t('scan.deleted_ntfs_subtitle')
      : selectedDevice?.filesystem === 'exfat'
        ? t('scan.deleted_exfat_subtitle')
        : selectedDevice?.filesystem === 'apfs'
          ? t('scan.deleted_apfs_subtitle')
          : selectedDevice?.filesystem === 'hfs+'
            ? t('scan.deleted_hfsplus_subtitle')
            : selectedDevice?.filesystem === 'ext4'
              ? t('scan.deleted_ext4_subtitle')
              : t('scan.deleted_fat32_subtitle');
  const deletedRecoveryNotice =
    selectedDevice?.filesystem === 'ntfs'
      ? t('scan.deleted_ntfs_notice')
      : selectedDevice?.filesystem === 'exfat'
        ? t('scan.deleted_exfat_notice')
        : selectedDevice?.filesystem === 'apfs'
          ? t('scan.deleted_apfs_notice')
          : selectedDevice?.filesystem === 'hfs+'
            ? t('scan.deleted_hfsplus_notice')
            : selectedDevice?.filesystem === 'ext4'
              ? t('scan.deleted_ext4_notice')
              : t('scan.deleted_fat32_notice');
  const activeCapabilityAvailable = imageMode
    ? runtimeCapabilities?.imagingEngine
    : lostVolumeMode
      ? Boolean(runtimeCapabilities?.scanEngine && runtimeCapabilities?.imagingEngine)
      : signatureCarvingMode
        ? Boolean(runtimeCapabilities?.scanEngine && runtimeCapabilities?.imagingEngine)
        : Boolean(runtimeCapabilities?.scanEngine && catalogScanAvailable);
  const pageSubtitle = imageMode
    ? t('scan.image_subtitle')
    : lostVolumeMode
      ? t('scan.lost_volume_subtitle', {
          filesystem: (lostVolumeFilesystem ?? 'unknown').toUpperCase(),
          label: lostVolumeLabel,
        })
      : signatureCarvingMode
        ? t('scan.signature_carving_subtitle')
        : deletedRecoveryMode
          ? deletedRecoverySubtitle
          : selectedDevice?.type === 'image'
            ? raidImportedSource
              ? t('scan.raid_source_subtitle')
              : t('scan.imported_source_subtitle')
            : t('scan.subtitle');
  const pageNotice = imageMode
    ? scanConfig?.imagingRequiresElevation
      ? t('scan.image_elevation_notice')
      : t('scan.image_notice')
    : lostVolumeMode
      ? t('scan.lost_volume_notice', {
          filesystem: (lostVolumeFilesystem ?? 'unknown').toUpperCase(),
          label: lostVolumeLabel,
        })
      : signatureCarvingMode
        ? t('scan.signature_carving_notice')
        : deletedRecoveryMode
          ? deletedRecoveryNotice
          : selectedDevice?.type === 'image'
            ? raidImportedSource
              ? t('scan.raid_source_notice')
              : t('scan.imported_source_notice')
            : t('scan.catalog_notice');
  const inferredImagingProfile = selectedDevice
    ? recommendedImagingProfileForDevice(selectedDevice)
    : undefined;
  const inferredImagingProfileReasonKey = selectedDevice
    ? recommendedImagingProfileReasonKeyForDevice(selectedDevice)
    : undefined;
  const effectiveImagingProfile =
    (scanConfig?.deviceId === selectedDeviceId ? scanConfig.imagingProfile : undefined) ??
    inferredImagingProfile;
  const effectiveImagingProfileReasonKey =
    (scanConfig?.deviceId === selectedDeviceId ? scanConfig.imagingProfileReasonKey : undefined) ??
    inferredImagingProfileReasonKey;
  const selectedExternalRescueMapPath =
    scanConfig?.deviceId === selectedDeviceId ? scanConfig.externalRescueMapPath : undefined;
  const cautiousImagingNoticeVisible =
    effectiveImagingProfile === 'cautious' &&
    (imageMode ||
      deletedRecoveryMode ||
      signatureCarvingMode ||
      lostVolumeMode ||
      reconstructionMode);

  useEffect(() => {
    if (!selectedDeviceId) {
      setEncryptionInfo(null);
      setEncryptionLoading(false);
      return;
    }

    let cancelled = false;
    setEncryptionLoading(true);

    fetchEncryptionInfo(selectedDeviceId)
      .then((info) => {
        if (!cancelled) {
          setEncryptionInfo(info);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEncryptionInfo(null);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setEncryptionLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [selectedDeviceId]);

  const setImageRescueMapPath = useCallback(
    (nextPath?: string) => {
      if (!selectedDeviceId || !selectedDevice) {
        return;
      }

      setScanConfig({
        deviceId: selectedDeviceId,
        scanType: 'image',
        targetFilesystems: [selectedDevice.filesystem],
        enableCarving: false,
        imagingRequiresElevation:
          scanConfig?.deviceId === selectedDeviceId
            ? scanConfig.imagingRequiresElevation
            : undefined,
        imagingProfile:
          scanConfig?.deviceId === selectedDeviceId
            ? scanConfig.imagingProfile
            : inferredImagingProfile,
        imagingProfileReasonKey:
          scanConfig?.deviceId === selectedDeviceId
            ? scanConfig.imagingProfileReasonKey
            : inferredImagingProfileReasonKey,
        externalRescueMapPath: nextPath,
      });
    },
    [
      inferredImagingProfile,
      inferredImagingProfileReasonKey,
      scanConfig,
      selectedDevice,
      selectedDeviceId,
      setScanConfig,
    ],
  );

  const handleChooseExternalRescueMap = useCallback(async () => {
    if (!selectedDeviceId || !selectedDevice) {
      return;
    }

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setError(t('scan.external_rescue_map_unavailable'));
      return;
    }

    try {
      const selectedPath = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: 'Rescue Maps',
            extensions: ['map', 'log', 'txt'],
          },
        ],
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        return;
      }

      setImageRescueMapPath(selectedPath);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('scan.external_rescue_map_picker_failed'));
    }
  }, [selectedDevice, selectedDeviceId, setImageRescueMapPath, t]);

  const handleClearExternalRescueMap = useCallback(() => {
    setImageRescueMapPath(undefined);
    setError(null);
  }, [setImageRescueMapPath]);

  const beginScan = useCallback(
    async (scanType: ScanType) => {
      if (!selectedDeviceId || !selectedDevice) return;
      if (encryptionLoading) {
        setError(t('scan.encryption_status_loading'));
        return;
      }
      if (encryptionLockedScanGate && !scanTypeAllowedOnLockedEncryptedView(scanType)) {
        setError(
          t('scan.encryption_locked_start_blocked', {
            step: encryptionInfo?.saferNextStep ?? t('safety.encrypted_locked_advice'),
          }),
        );
        return;
      }
      const externalRescueMapPath =
        scanType === 'image' && scanConfig?.deviceId === selectedDeviceId
          ? scanConfig.externalRescueMapPath
          : undefined;
      if (selectedDevice.type === 'image') {
        if (importedSourceLoading) {
          setError(t('devices.imported_status_loading'));
          return;
        }
        if (scanImportedGateActive) {
          setError(
            scanImportedSourceUnsupported
              ? (importedSourceStatus?.supportNote ??
                  t('devices.imported_status_unsupported', {
                    format: importedSourceStatus?.sourceFormat ?? 'source',
                  }))
              : scanImportedSourceMissing
                ? t('scan.imported_source_missing_gate')
                : (importedSourceError ?? t('scan.imported_prepare_gate')),
          );
          return;
        }
      }
      const preservedPotentialVolumeConfig =
        scanConfig?.scanType === 'lost-volume' && scanConfig.deviceId === selectedDeviceId
          ? {
              potentialVolumeId: scanConfig.potentialVolumeId,
              potentialVolumeLabel: scanConfig.potentialVolumeLabel,
              potentialVolumeFilesystem: scanConfig.potentialVolumeFilesystem,
              potentialVolumeStartOffset: scanConfig.potentialVolumeStartOffset,
              potentialVolumeSizeBytes: scanConfig.potentialVolumeSizeBytes,
              imagingRequiresElevation: scanConfig.imagingRequiresElevation,
              imagingProfile: scanConfig.imagingProfile,
              imagingProfileReasonKey: scanConfig.imagingProfileReasonKey,
            }
          : {};
      const imagingProfileApplicable =
        scanType === 'image' ||
        scanType === 'carving' ||
        scanType === 'signature-carving' ||
        scanType === 'lost-volume' ||
        scanType === 'reconstruction';
      const initialImagingProfile =
        imagingProfileApplicable && selectedDevice
          ? recommendedImagingProfileForDevice(selectedDevice)
          : undefined;
      const initialImagingProfileReasonKey =
        imagingProfileApplicable && selectedDevice
          ? recommendedImagingProfileReasonKeyForDevice(selectedDevice)
          : undefined;

      setError(null);
      setIsStarting(true);
      clearScanLogs();
      setRecoveryResult(null);
      setScanProgress(null);
      setActiveScanId(null);
      syncedHistoryScanIdRef.current = null;
      setScanConfig({
        deviceId: selectedDeviceId,
        scanType,
        targetFilesystems: [
          scanType === 'lost-volume'
            ? (scanConfig?.potentialVolumeFilesystem ?? selectedDevice.filesystem)
            : selectedDevice.filesystem,
        ],
        enableCarving:
          scanType === 'carving' || scanType === 'signature-carving' || scanType === 'lost-volume',
        imagingProfile: initialImagingProfile,
        imagingProfileReasonKey: initialImagingProfileReasonKey,
        externalRescueMapPath,
        ...preservedPotentialVolumeConfig,
      });

      try {
        let scanId: string | null = null;

        if (scanType === 'image') {
          const diagnostic = await fetchDiagnostic(selectedDeviceId);
          if (!diagnostic.imagingReady) {
            setError(diagnostic.imagingBlockReason ?? t('scan.image_unavailable_desc'));
            return;
          }

          setScanConfig({
            deviceId: selectedDeviceId,
            scanType,
            targetFilesystems: [selectedDevice.filesystem],
            enableCarving: false,
            imagingRequiresElevation: diagnostic.imagingRequiresElevation,
            imagingProfile: diagnostic.imagingProfile,
            imagingProfileReasonKey: diagnostic.imagingProfileReasonKey,
            externalRescueMapPath,
          });

          if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
            setError(t('scan.image_destination_unavailable'));
            return;
          }

          const destinationPath = await save({
            defaultPath: buildSuggestedImageName(selectedDevice.name),
            filters: [
              {
                name: 'Disk Images',
                extensions: ['img', 'dd', 'bin'],
              },
            ],
          });

          if (!destinationPath) {
            return;
          }

          scanId = await startImaging(selectedDeviceId, destinationPath, externalRescueMapPath);
        } else if (scanType === 'lost-volume') {
          if (!scanConfig?.potentialVolumeId) {
            setError(t('scan.lost_volume_missing_candidate'));
            return;
          }

          scanId = await startPotentialVolumeScan(selectedDeviceId, scanConfig.potentialVolumeId);
        } else {
          if (scanType === 'signature-carving') {
            const diagnostic = await fetchDiagnostic(selectedDeviceId);
            if (!diagnostic.imagingReady) {
              setError(
                diagnostic.imagingBlockReason ?? t('scan.signature_carving_unavailable_desc'),
              );
              return;
            }

            setScanConfig({
              deviceId: selectedDeviceId,
              scanType,
              targetFilesystems: [selectedDevice.filesystem],
              enableCarving: true,
              imagingRequiresElevation: diagnostic.imagingRequiresElevation,
              imagingProfile: diagnostic.imagingProfile,
              imagingProfileReasonKey: diagnostic.imagingProfileReasonKey,
            });
          }
          scanId = await startScan(selectedDeviceId, scanType);
        }

        setActiveScanId(scanId);
        if (scanId) {
          trackScan({
            id: scanId,
            agentId: null,
            label: `${selectedDevice.name} • ${scanType}`,
            scanType,
            startedAtMs: Date.now(),
          });
        }
        const history = await fetchScanHistory().catch(() => []);
        setScanHistory(history);
      } catch (err) {
        setError(err instanceof Error ? err.message : t('scan.start_failed'));
      } finally {
        setIsStarting(false);
      }
    },
    [
      clearScanLogs,
      encryptionInfo?.saferNextStep,
      encryptionLoading,
      encryptionLockedScanGate,
      importedSourceError,
      importedSourceLoading,
      importedSourceStatus?.sourceFormat,
      importedSourceStatus?.supportNote,
      scanConfig,
      scanImportedGateActive,
      scanImportedSourceMissing,
      scanImportedSourceUnsupported,
      selectedDevice,
      selectedDeviceId,
      setActiveScanId,
      setRecoveryResult,
      setScanConfig,
      setScanHistory,
      setScanProgress,
      t,
      trackScan,
    ],
  );

  const handleCancel = useCallback(async () => {
    setConfirmCancelOpen(false);
    if (!activeScanId) return;
    try {
      await cancelScanFor(activeScanId, activeRemoteAgentId);
      setScanProgress(scanProgress ? { ...scanProgress, status: 'cancelled' } : null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('scan.control_failed'));
    }
  }, [activeScanId, activeRemoteAgentId, scanProgress, setScanProgress, t]);

  useEffect(() => {
    if (
      autoStartTriedRef.current ||
      !scanConfig ||
      scanConfig.deviceId !== selectedDeviceId ||
      scanConfig.scanType === 'image' ||
      encryptionLoading ||
      (encryptionLockedScanGate && !scanTypeAllowedOnLockedEncryptedView(scanConfig.scanType)) ||
      importedSourceLoading ||
      scanImportedGateActive ||
      activeScanId ||
      scanProgress ||
      isStarting ||
      !activeCapabilityAvailable
    ) {
      return;
    }

    autoStartTriedRef.current = true;
    beginScan(scanConfig.scanType);
  }, [
    activeCapabilityAvailable,
    activeScanId,
    encryptionLoading,
    encryptionLockedScanGate,
    importedSourceLoading,
    isStarting,
    scanConfig,
    scanImportedGateActive,
    scanProgress,
    selectedDeviceId,
    beginScan,
  ]);

  // Progress + logs are fed by the global background poller
  // (`useBackgroundScanPoller` in App.tsx). This effect only syncs the local
  // scan history when the focused scan reaches a terminal status.
  useEffect(() => {
    if (!activeScanId || !scanProgress) return;
    const terminal =
      scanProgress.status === 'completed' ||
      scanProgress.status === 'cancelled' ||
      scanProgress.status === 'error';
    if (!terminal || syncedHistoryScanIdRef.current === activeScanId) return;
    syncedHistoryScanIdRef.current = activeScanId;
    if (!activeRemoteAgentId) {
      fetchScanHistory()
        .then((history) => setScanHistory(history))
        .catch(() => {});
    }
  }, [activeScanId, activeRemoteAgentId, scanProgress, setScanHistory]);

  const handlePrepareImportedSource = useCallback(async () => {
    const nextStatus = await prepareImportedSourceFromScan();
    if (!nextStatus) {
      setError(importedSourceError ?? t('devices.imported_prepare_failed'));
      return;
    }

    setError(null);
    autoStartTriedRef.current = false;
  }, [importedSourceError, prepareImportedSourceFromScan, t]);

  if (!selectedDevice && !isRemoteScan) {
    return (
      <EmptyState
        title={t('scan.empty_title')}
        description={t('scan.empty_desc')}
        action={
          <button type="button" className="btn btn-primary" onClick={() => navigate('/devices')}>
            {t('devices.refresh')}
          </button>
        }
      />
    );
  }

  if (
    !isRemoteScan &&
    isImportedSource &&
    importedSourceLoading &&
    !activeScanId &&
    !scanProgress
  ) {
    return <LoadingState message={t('devices.imported_status_loading')} />;
  }

  if (scanImportedGateActive) {
    return (
      <div>
        <PageHeader title={t('scan.title')} subtitle={t('scan.imported_source_subtitle')} />

        {__ALLOW_BROWSER_PREVIEW__ && !isTauri() && (
          <div className="mb-6">
            <WarningBanner variant="info">{t('scan.preview_notice')}</WarningBanner>
          </div>
        )}

        <div className="mb-6">
          <WarningBanner
            variant={
              scanImportedSourceUnsupported || scanImportedSourceMissing || importedSourceError
                ? 'danger'
                : 'warning'
            }
          >
            {scanImportedSourceUnsupported
              ? (importedSourceStatus?.supportNote ??
                t('devices.imported_status_unsupported', {
                  format: importedSourceStatus?.sourceFormat ?? 'source',
                }))
              : scanImportedSourceMissing
                ? t('scan.imported_source_missing_gate')
                : (importedSourceError ?? t('scan.imported_prepare_gate'))}
          </WarningBanner>
        </div>

        <SectionCard
          className="mb-6"
          action={
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => navigate('/devices')}
            >
              {t('common.back')}
            </button>
          }
        >
          <ImportedSourceStatusPanel
            device={selectedDevice}
            status={importedSourceStatus}
            loading={importedSourceLoading}
            preparing={importedSourcePreparing}
            error={importedSourceError}
            onPrepare={() => {
              void handlePrepareImportedSource();
            }}
          />
        </SectionCard>
      </div>
    );
  }

  if (!isRemoteScan && !activeCapabilityAvailable && selectedDevice) {
    return (
      <div>
        <PageHeader title={t('scan.title')} subtitle={pageSubtitle} />

        <div className="mb-6">
          <WarningBanner variant="info">
            {imageMode
              ? t('scan.image_unavailable_desc')
              : lostVolumeMode
                ? t('scan.lost_volume_unavailable_desc')
                : signatureCarvingMode
                  ? t('scan.signature_carving_unavailable_desc')
                  : t('scan.unavailable_desc')}
          </WarningBanner>
        </div>

        <SectionCard>
          <EmptyState
            icon={<FileSearch className="empty-state-icon" />}
            title={
              imageMode
                ? t('scan.image_unavailable_title')
                : lostVolumeMode
                  ? t('scan.lost_volume_unavailable_title')
                  : signatureCarvingMode
                    ? t('scan.signature_carving_unavailable_title')
                    : t('scan.unavailable_title')
            }
            description={`${selectedDevice.name} • ${selectedDevice.devicePath}`}
            action={
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => navigate('/diagnostic')}
              >
                {t('common.back')}
              </button>
            }
          />
        </SectionCard>
      </div>
    );
  }

  const currentStage =
    scanProgress?.stage && stages.includes(scanProgress.stage)
      ? scanProgress.stage
      : 'initializing';
  const isCompleted = scanProgress?.status === 'completed';
  const isPaused = scanProgress?.status === 'paused';
  const isCancelled = scanProgress?.status === 'cancelled';
  const isTerminal = isCompleted || isCancelled || scanProgress?.status === 'error';
  const liveControlDisabled =
    currentStage === 'creating-image' && Boolean(scanConfig?.imagingRequiresElevation);
  const progressPercent = scanProgress?.percentComplete ?? 0;
  const filesFound = scanProgress?.filesFound ?? 0;
  const errorsCount = scanProgress?.errorsCount ?? 0;
  const elapsed = scanProgress?.elapsedSeconds ?? 0;
  const resumedImagingBytes = scanProgress?.resumeFromBytes ?? 0;
  const unreadableRangesCount = scanProgress?.unreadableRangesCount ?? 0;
  const unreadableBytes = scanProgress?.unreadableBytes ?? 0;
  const rescuedAfterRetryBytes = scanProgress?.rescuedAfterRetryBytes ?? 0;
  const retryPassesCompleted = scanProgress?.retryPassesCompleted ?? 0;
  const unreadableRanges = scanProgress?.unreadableRanges ?? [];
  const unreadableRangeHighlights = buildUnreadableRangeHighlights(unreadableRanges);
  const largestUnreadableRange = unreadableRangeHighlights[0];
  const imagingIncidentPanelVisible =
    imageMode &&
    (effectiveImagingProfile === 'cautious' ||
      resumedImagingBytes > 0 ||
      unreadableBytes > 0 ||
      retryPassesCompleted > 0 ||
      Boolean(selectedExternalRescueMapPath));
  const imagingIncidentHighlights = imagingIncidentPanelVisible
    ? collectImagingIncidentHighlights(scanLogs)
    : [];

  const handlePause = async () => {
    if (!activeScanId) return;
    try {
      await pauseScanFor(activeScanId, activeRemoteAgentId);
      setScanProgress(scanProgress ? { ...scanProgress, status: 'paused' } : null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('scan.control_failed'));
    }
  };

  const handleResume = async () => {
    if (!activeScanId) return;
    try {
      await resumeScanFor(activeScanId, activeRemoteAgentId);
      setScanProgress(scanProgress ? { ...scanProgress, status: 'scanning' } : null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('scan.control_failed'));
    }
  };

  const handleExportImagingReport = async () => {
    if (!activeScanId || !imageMode) return;
    setImagingExportNotice(null);

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setImagingExportNotice({
        variant: 'warning',
        message: t('history.imaging_report_unavailable'),
      });
      return;
    }

    setImagingReportExporting(true);
    try {
      const content = await generateImagingSessionReport(activeScanId);
      const destinationPath = await save({
        defaultPath: `recupere-imaging-session-${activeScanId}.txt`,
        filters: [{ name: 'Text report', extensions: ['txt', 'log'] }],
      });

      if (!destinationPath) {
        setImagingExportNotice({
          variant: 'warning',
          message: t('history.imaging_report_cancelled'),
        });
        return;
      }

      const savedPath = await saveTechnicalTimelineReport(destinationPath, content);
      setImagingExportNotice({
        variant: 'success',
        message: t('history.imaging_report_success', { path: savedPath }),
      });
    } catch (err) {
      setImagingExportNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('history.imaging_report_failed'),
      });
    } finally {
      setImagingReportExporting(false);
    }
  };

  const handleExportImagingMap = async () => {
    if (!activeScanId || !imageMode) return;
    setImagingExportNotice(null);

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setImagingExportNotice({
        variant: 'warning',
        message: t('history.imaging_map_unavailable'),
      });
      return;
    }

    setImagingMapExporting(true);
    try {
      const content = await generateImagingRescueMap(activeScanId);
      const destinationPath = await save({
        defaultPath: `recupere-imaging-session-${activeScanId}.map`,
        filters: [{ name: 'Rescue map', extensions: ['map', 'log'] }],
      });

      if (!destinationPath) {
        setImagingExportNotice({
          variant: 'warning',
          message: t('history.imaging_map_cancelled'),
        });
        return;
      }

      const savedPath = await saveTechnicalTimelineReport(destinationPath, content);
      setImagingExportNotice({
        variant: 'success',
        message: t('history.imaging_map_success', { path: savedPath }),
      });
    } catch (err) {
      setImagingExportNotice({
        variant: 'danger',
        message: err instanceof Error ? err.message : t('history.imaging_map_failed'),
      });
    } finally {
      setImagingMapExporting(false);
    }
  };

  return (
    <div>
      <PageHeader
        title={isCompleted ? t('scan.completed') : t('scan.title')}
        subtitle={pageSubtitle}
      />

      <TrackedScansBar />

      <div className="mb-6">
        <WarningBanner variant="info">{pageNotice}</WarningBanner>
      </div>

      {!isRemoteScan && raidImportedSource && (
        <div className="mb-6">
          <WarningBanner variant="success">{t('scan.raid_source_ready_notice')}</WarningBanner>
        </div>
      )}

      {__ALLOW_BROWSER_PREVIEW__ && !isTauri() && (
        <div className="mb-6">
          <WarningBanner variant="info">{t('scan.preview_notice')}</WarningBanner>
        </div>
      )}

      {!isRemoteScan && isImportedSource && importedSourceStatus?.prepared && (
        <div className="mb-6">
          <WarningBanner variant="success">{t('scan.imported_prepared_notice')}</WarningBanner>
        </div>
      )}

      {cautiousImagingNoticeVisible && effectiveImagingProfileReasonKey && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            {t('scan.imaging_profile_cautious_notice', {
              reason: t(effectiveImagingProfileReasonKey),
            })}
          </WarningBanner>
        </div>
      )}

      {resumedImagingBytes > 0 && (
        <div className="mb-6">
          <WarningBanner variant="info">
            {t('scan.resume_imaging_notice', {
              bytes: formatBytes(resumedImagingBytes),
            })}
          </WarningBanner>
        </div>
      )}

      {unreadableBytes > 0 && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            {t('scan.unreadable_imaging_notice', {
              ranges: unreadableRangesCount,
              bytes: formatBytes(unreadableBytes),
            })}
          </WarningBanner>
        </div>
      )}

      {liveControlDisabled && !isTerminal && (
        <div className="mb-6">
          <WarningBanner variant="warning">{t('scan.control_limited_elevated')}</WarningBanner>
        </div>
      )}

      {error && (
        <div className="mb-6">
          <ErrorState title={t('common.error')} description={error} />
        </div>
      )}

      {imagingExportNotice && (
        <div className="mb-6">
          <WarningBanner variant={imagingExportNotice.variant}>
            {imagingExportNotice.message}
          </WarningBanner>
        </div>
      )}

      {encryptionLoading && !activeScanId && !scanProgress && (
        <div className="mb-6">
          <WarningBanner variant="info">{t('scan.encryption_status_loading')}</WarningBanner>
        </div>
      )}

      {encryptionLockedScanGate && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            {t('scan.encryption_locked_scan_gate', {
              step: encryptionInfo?.saferNextStep ?? t('safety.encrypted_locked_advice'),
            })}
          </WarningBanner>
        </div>
      )}

      {!activeScanId && !scanProgress && imageMode && (
        <SectionCard className="mb-6" title={t('scan.external_rescue_map_title')}>
          <div className="flex flex-col gap-4" style={{ padding: 'var(--space-4) 0' }}>
            <div className="text-sm text-secondary">{t('scan.external_rescue_map_desc')}</div>
            <div
              className="rounded-lg p-3"
              style={{
                background: 'var(--color-bg-secondary)',
                border: '1px solid var(--color-border)',
              }}
            >
              <div className="text-sm font-medium">
                {selectedExternalRescueMapPath
                  ? t('scan.external_rescue_map_selected', {
                      name: getPathBasename(selectedExternalRescueMapPath),
                    })
                  : t('scan.external_rescue_map_none')}
              </div>
              {selectedExternalRescueMapPath && (
                <div className="mt-2 text-xs text-secondary break-all">
                  {t('scan.external_rescue_map_path', { path: selectedExternalRescueMapPath })}
                </div>
              )}
            </div>
            <div className="text-xs text-secondary">{t('scan.external_rescue_map_hint')}</div>
            <div className="flex gap-3">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => {
                  void handleChooseExternalRescueMap();
                }}
              >
                {t('scan.external_rescue_map_choose')}
              </button>
              {selectedExternalRescueMapPath && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={handleClearExternalRescueMap}
                >
                  {t('scan.external_rescue_map_clear')}
                </button>
              )}
            </div>
          </div>
        </SectionCard>
      )}

      {!activeScanId && !scanProgress && (
        <SectionCard className="mb-6">
          <div className="flex flex-col gap-4" style={{ padding: 'var(--space-4) 0' }}>
            <div className="text-md font-medium">{t('scan.start_prompt')}</div>
            {lostVolumeMode && (
              <div className="text-sm text-secondary">
                {t('scan.lost_volume_ready', {
                  filesystem: (lostVolumeFilesystem ?? 'unknown').toUpperCase(),
                  label: lostVolumeLabel,
                })}
              </div>
            )}
            <div
              className="mb-4 p-3"
              style={{
                background: 'var(--color-bg-secondary)',
                borderRadius: 'var(--radius-md)',
                borderLeft: '3px solid var(--color-accent)',
              }}
            >
              <div className="text-sm font-medium mb-1" style={{ color: 'var(--color-accent)' }}>
                {t('scan.ai_guidance_title')}
              </div>
              <div className="text-sm text-secondary">
                {deletedRecoveryAvailable
                  ? t('scan.ai_guidance_deleted')
                  : selectedDevice?.type === 'image'
                    ? t('scan.ai_guidance_imported_image')
                    : imagingAvailable
                      ? t('scan.ai_guidance_imaging')
                      : t('scan.ai_guidance_quick')}
              </div>
            </div>
            <div className="flex gap-3">
              {catalogScanAvailable && (
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => beginScan('quick')}
                  disabled={isStarting || encryptionLoading || encryptionLockedScanGate}
                >
                  <Play size={16} />
                  {t('scan.quick')}
                </button>
              )}
              {catalogScanAvailable && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => beginScan('deep')}
                  disabled={isStarting || encryptionLoading || encryptionLockedScanGate}
                >
                  <Play size={16} />
                  {t('scan.deep')}
                </button>
              )}
              {imagingAvailable && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => beginScan('image')}
                  disabled={isStarting || encryptionLoading}
                >
                  <Play size={16} />
                  {t('scan.image')}
                </button>
              )}
              {deletedRecoveryAvailable && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => beginScan('carving')}
                  disabled={isStarting || encryptionLoading || encryptionLockedScanGate}
                >
                  <Play size={16} />
                  {deletedRecoveryLabel}
                </button>
              )}
              {imagingAvailable && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => beginScan('signature-carving')}
                  disabled={isStarting || encryptionLoading || encryptionLockedScanGate}
                >
                  <Play size={16} />
                  {t('scan.signature_carving')}
                </button>
              )}
              {imagingAvailable && userMode === 'expert' && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => beginScan('reconstruction')}
                  disabled={isStarting || encryptionLoading || encryptionLockedScanGate}
                >
                  <Play size={16} />
                  {t('scan.reconstruction')}
                </button>
              )}
            </div>
          </div>
        </SectionCard>
      )}

      {(activeScanId || scanProgress) && (
        <SectionCard className="mb-6">
          <div className="flex flex-col gap-6">
            <div className="flex items-center justify-between">
              <span className="font-medium text-md">{t(`scan.stages.${currentStage}`)}</span>
              <span className="text-sm text-secondary">{progressPercent.toFixed(1)}%</span>
            </div>

            <ProgressTimeline percent={progressPercent} animated={!isCompleted} />

            <div className="grid grid-4 gap-4">
              <div className="stat-card">
                <span className="stat-card-label">
                  {imageMode ? t('scan.bytes_copied') : t('scan.files_found')}
                </span>
                <span className="stat-card-value">
                  {imageMode
                    ? `${formatBytes(scanProgress?.bytesScanned ?? 0)} / ${formatBytes(scanProgress?.totalBytes ?? 0)}`
                    : filesFound}
                </span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">{t('scan.errors')}</span>
                <span className="stat-card-value text-warning">{errorsCount}</span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">{t('scan.elapsed')}</span>
                <span className="stat-card-value">{formatDuration(elapsed)}</span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">{t('scan.remaining')}</span>
                <span className="stat-card-value">
                  {isCompleted
                    ? '—'
                    : `~${formatDuration(Math.max(0, Math.floor((elapsed / Math.max(progressPercent, 1)) * (100 - progressPercent))))}`}
                </span>
              </div>
            </div>

            {imagingIncidentPanelVisible && (
              <div
                className="rounded-lg"
                style={{
                  padding: 'var(--space-4)',
                  background: 'var(--color-bg-secondary)',
                  border: '1px solid var(--color-border)',
                }}
              >
                <div className="flex items-start justify-between gap-4 mb-4">
                  <div>
                    <div className="font-medium">{t('scan.imaging_incident_title')}</div>
                    <div className="text-sm text-secondary">
                      {t('scan.imaging_incident_subtitle')}
                    </div>
                  </div>
                </div>

                <div className="grid grid-2 gap-4">
                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.imaging_profile_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {effectiveImagingProfile === 'cautious'
                        ? t('scan.imaging_profile_cautious')
                        : t('scan.imaging_profile_standard')}
                    </span>
                    {effectiveImagingProfileReasonKey && (
                      <span className="text-xs text-secondary">
                        {t(effectiveImagingProfileReasonKey)}
                      </span>
                    )}
                  </div>

                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.retry_passes_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {retryPassesCompleted}
                    </span>
                    <span className="text-xs text-secondary">
                      {t('scan.rescued_after_retry_label', {
                        bytes: formatBytes(rescuedAfterRetryBytes),
                      })}
                    </span>
                  </div>

                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.unreadable_ranges_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {unreadableRangesCount}
                    </span>
                    <span className="text-xs text-secondary">{formatBytes(unreadableBytes)}</span>
                  </div>

                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.rescue_map_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {selectedExternalRescueMapPath ? t('common.yes') : t('common.no')}
                    </span>
                    <span className="text-xs text-secondary">
                      {selectedExternalRescueMapPath
                        ? getPathBasename(selectedExternalRescueMapPath)
                        : t('scan.external_rescue_map_none')}
                    </span>
                  </div>

                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.unreadable_sampled_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {unreadableRangeHighlights.length}
                    </span>
                    <span className="text-xs text-secondary">
                      {t('scan.unreadable_sampled_hint', {
                        total: unreadableRangesCount,
                      })}
                    </span>
                  </div>

                  <div className="stat-card">
                    <span className="stat-card-label">{t('scan.largest_unreadable_label')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-lg)' }}>
                      {largestUnreadableRange
                        ? formatBytes(largestUnreadableRange.length)
                        : t('common.none')}
                    </span>
                    <span className="text-xs text-secondary">
                      {largestUnreadableRange
                        ? formatRangeLabel(largestUnreadableRange)
                        : t('scan.unreadable_ranges_pending')}
                    </span>
                  </div>
                </div>

                <div className="mt-4 flex flex-col gap-3">
                  {unreadableBytes > 0 && (
                    <WarningBanner variant="warning">
                      {t('scan.imaging_guidance_unreadable', {
                        ranges: unreadableRangesCount,
                        bytes: formatBytes(unreadableBytes),
                      })}
                    </WarningBanner>
                  )}
                  {retryPassesCompleted > 0 && (
                    <WarningBanner variant="info">
                      {t('scan.imaging_guidance_retry', {
                        passes: retryPassesCompleted,
                        bytes: formatBytes(rescuedAfterRetryBytes),
                      })}
                    </WarningBanner>
                  )}
                  {resumedImagingBytes > 0 && (
                    <WarningBanner variant="info">
                      {t('scan.imaging_guidance_resume', {
                        bytes: formatBytes(resumedImagingBytes),
                      })}
                    </WarningBanner>
                  )}
                  {selectedExternalRescueMapPath && (
                    <WarningBanner variant="info">
                      {t('scan.imaging_guidance_rescue_map', {
                        name: getPathBasename(selectedExternalRescueMapPath),
                      })}
                    </WarningBanner>
                  )}
                  {liveControlDisabled && (
                    <WarningBanner variant="warning">
                      {t('scan.imaging_guidance_elevated')}
                    </WarningBanner>
                  )}
                </div>

                {unreadableRangeHighlights.length > 0 && (
                  <div className="mt-4">
                    <div className="text-sm font-medium mb-2">
                      {t('scan.unreadable_ranges_sample_title')}
                    </div>
                    <div className="flex flex-col gap-2">
                      {unreadableRangeHighlights.map((range) => (
                        <div
                          key={`${range.startOffset}-${range.length}`}
                          className="rounded-lg"
                          style={{
                            padding: 'var(--space-3)',
                            background: 'var(--color-bg)',
                            border: '1px solid var(--color-border)',
                          }}
                        >
                          <div className="flex items-center justify-between gap-4">
                            <div className="text-sm font-medium">{formatRangeLabel(range)}</div>
                            <div className="text-sm">{formatBytes(range.length)}</div>
                          </div>
                          <div className="text-xs text-secondary mt-1">
                            {t('scan.unreadable_ranges_sample_hint')}
                          </div>
                        </div>
                      ))}
                    </div>
                    {unreadableRangesCount > unreadableRangeHighlights.length && (
                      <div className="text-xs text-secondary mt-2">
                        {t('scan.unreadable_ranges_more_hint', {
                          shown: unreadableRangeHighlights.length,
                          total: unreadableRangesCount,
                        })}
                      </div>
                    )}
                  </div>
                )}

                {imagingIncidentHighlights.length > 0 && (
                  <div className="mt-4">
                    <div className="text-sm font-medium mb-2">
                      {t('scan.imaging_incident_events_title')}
                    </div>
                    <div className="scan-log-panel">
                      {imagingIncidentHighlights.map((log, index) => (
                        <div
                          key={`${log.timestampMs}-${log.level}-${index}`}
                          className="scan-log-entry"
                        >
                          <span className="scan-log-time">
                            {new Date(log.timestampMs).toLocaleTimeString()}
                          </span>
                          <span className={`scan-log-level ${log.level}`}>
                            {log.level.toUpperCase()}
                          </span>
                          <span className="scan-log-message">{log.message}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}

            {!isTerminal && (
              <div className="flex gap-3">
                {!isPaused ? (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={handlePause}
                    disabled={liveControlDisabled}
                  >
                    {t('scan.pause')}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={handleResume}
                    disabled={liveControlDisabled}
                  >
                    {t('scan.resume')}
                  </button>
                )}
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setConfirmCancelOpen(true)}
                  disabled={liveControlDisabled}
                >
                  {t('scan.stop')}
                </button>
                <ConfirmDialog
                  open={confirmCancelOpen}
                  title={t('scan.confirm_cancel_title')}
                  description={t('scan.confirm_cancel_desc')}
                  variant="danger"
                  confirmLabel={t('scan.stop')}
                  onConfirm={handleCancel}
                  onCancel={() => setConfirmCancelOpen(false)}
                />
              </div>
            )}

            {isTerminal && imageMode && (
              <div
                className="rounded-lg"
                style={{
                  padding: 'var(--space-4)',
                  background: 'var(--color-bg-secondary)',
                  border: '1px solid var(--color-border)',
                }}
              >
                <div className="font-medium mb-2">{t('scan.imaging_handoff_title')}</div>
                <div className="text-sm text-secondary mb-4">
                  {t('scan.imaging_handoff_subtitle')}
                </div>
                <div className="flex gap-3 flex-wrap">
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => {
                      void handleExportImagingReport();
                    }}
                    disabled={imagingReportExporting || imagingMapExporting || !activeScanId}
                  >
                    {imagingReportExporting
                      ? t('history.imaging_report_exporting')
                      : t('history.imaging_report_action')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => {
                      void handleExportImagingMap();
                    }}
                    disabled={imagingReportExporting || imagingMapExporting || !activeScanId}
                  >
                    {imagingMapExporting
                      ? t('history.imaging_map_exporting')
                      : t('history.imaging_map_action')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => navigate('/history')}
                  >
                    <History size={18} />
                    {t('scan.view_history')}
                  </button>
                </div>
              </div>
            )}

            {isTerminal && (
              <div className="flex gap-3">
                {isCancelled ? (
                  <button
                    type="button"
                    className="btn btn-primary btn-lg"
                    onClick={() => navigate('/history')}
                  >
                    <History size={18} />
                    {t('scan.view_history')}
                  </button>
                ) : imageMode ? (
                  <button
                    type="button"
                    className="btn btn-primary btn-lg"
                    onClick={() => navigate('/history')}
                  >
                    <History size={18} />
                    {t('scan.view_history')}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn btn-primary btn-lg"
                    onClick={() => navigate('/results')}
                  >
                    <FileSearch size={18} />
                    {t('scan.view_results')}
                  </button>
                )}
              </div>
            )}
          </div>
        </SectionCard>
      )}

      <SectionCard title={t('scan.log_title')}>
        <ScanLogPanel logs={scanLogs} emptyMessage={t('scan.log_waiting')} />
      </SectionCard>
    </div>
  );
}
