import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, RefreshCw } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ConfirmDialog } from '../components/common/ConfirmDialog';
import { EmptyState } from '../components/common/EmptyState';
import { ErrorState } from '../components/common/ErrorState';
import { LoadingState } from '../components/common/LoadingState';
import { WarningBanner } from '../components/common/WarningBanner';
import { DeviceCard } from '../components/device/DeviceCard';
import { DeviceInsightsPanel } from '../components/device/DeviceInsightsPanel';
import { ImportedSourceStatusPanel } from '../components/device/ImportedSourceStatusPanel';
import { RemoteAgentsSection } from '../components/device/RemoteAgentsSection';
import { SmartDashboard } from '../components/device/SmartDashboard';
import { PageHeader } from '../components/layout/PageHeader';
import {
  fetchDevices,
  fetchDiagnostic,
  fetchEncryptionInfo,
  fetchImportedRecoverySourceStatus,
  fetchRaidMetadata,
  fetchSmartReport,
  importRecoverySource,
  prepareImportedRecoverySource,
  removeImportedRecoverySource,
} from '../hooks/useIpc';
import { useAppStore } from '../stores/appStore';
import type {
  DetectedDevice,
  DeviceType,
  EncryptionInfo,
  ImportedRecoverySourceStatus,
  RaidMetadata,
  SmartReport,
} from '../types';
import { loadBrowserPreviewState } from '../utils/browserPreviewSeed';
import { getImportedSourceKind } from '../utils/importedSourceKind';

type DeviceFilter = 'all' | 'removable' | 'internal' | 'images';

const REMOVABLE_TYPES: DeviceType[] = ['usb', 'sd', 'external'];
const INTERNAL_TYPES: DeviceType[] = ['hdd', 'ssd', 'nvme'];

function groupByType(devices: DetectedDevice[]): Map<DeviceType, DetectedDevice[]> {
  const groups = new Map<DeviceType, DetectedDevice[]>();
  for (const device of devices) {
    const list = groups.get(device.type) || [];
    list.push(device);
    groups.set(device.type, list);
  }
  return groups;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(value >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function DevicesPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const previewState = loadBrowserPreviewState();
  const {
    devices,
    setDevices,
    setDevicesLoading,
    devicesLoading,
    runtimeCapabilities,
    setScanConfig,
  } = useAppStore();
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmImageOpen, setConfirmImageOpen] = useState(false);
  const [pendingImageDevice, setPendingImageDevice] = useState<DetectedDevice | null>(null);
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false);
  const [pendingRemoveDevice, setPendingRemoveDevice] = useState<DetectedDevice | null>(null);
  const [filter, setFilter] = useState<DeviceFilter>('all');
  const [smartReports, setSmartReports] = useState<Record<string, SmartReport | null>>({});
  const [smartLoading, setSmartLoading] = useState<Record<string, boolean>>({});
  const [smartErrors, setSmartErrors] = useState<Record<string, string | null>>({});
  const [smartExpanded, setSmartExpanded] = useState<Record<string, boolean>>({});
  const [advancedSignals, setAdvancedSignals] = useState<
    Record<
      string,
      {
        raidMetadata: RaidMetadata | null;
        encryptionInfo: EncryptionInfo | null;
      } | null
    >
  >({});
  const [advancedLoading, setAdvancedLoading] = useState<Record<string, boolean>>({});
  const [advancedErrors, setAdvancedErrors] = useState<Record<string, string | null>>({});
  const [advancedExpanded, setAdvancedExpanded] = useState<Record<string, boolean>>({});
  const [importedSourceStatuses, setImportedSourceStatuses] = useState<
    Record<string, ImportedRecoverySourceStatus | null>
  >(() => previewState?.importedSourceStatusesByDeviceId ?? {});
  const [importedSourceLoading, setImportedSourceLoading] = useState<Record<string, boolean>>({});
  const [importedSourcePreparing, setImportedSourcePreparing] = useState<Record<string, boolean>>(
    {},
  );
  const [importedSourceErrors, setImportedSourceErrors] = useState<Record<string, string | null>>(
    {},
  );

  // `silent=true` is used by the background poller so we don't flash the
  // "loading…" state every 3 seconds.
  const loadDevices = useCallback(
    async (silent = false) => {
      if (!silent) {
        setDevicesLoading(true);
        setError(null);
        setNotice(null);
      }
      try {
        const result = await fetchDevices();
        setDevices(result);
        if (!silent) setError(null);
      } catch (err) {
        if (!silent) {
          setError(err instanceof Error ? err.message : 'Failed to detect devices');
        }
      } finally {
        if (!silent) setDevicesLoading(false);
      }
    },
    [setDevices, setDevicesLoading],
  );

  // Initial load + native hotplug detection.
  // The Rust backend runs a `device_watcher` thread that emits a
  // `device:changed` Tauri event whenever a removable drive is plugged in or
  // unplugged (FSEvents on macOS, inotify on Linux, fast diff loop on
  // Windows). We listen to it here and refresh the list instantly.
  // A 15s safety poll runs in addition, in case the native watcher misses
  // anything (e.g. permissions, exotic mount paths).
  useEffect(() => {
    loadDevices();

    let cancelled = false;
    let timer: number | null = null;
    let unlisten: (() => void) | null = null;

    // Listen to the native hotplug event when the desktop runtime is active.
    if (isTauri()) {
      listen('device:changed', () => {
        if (!cancelled) loadDevices(true);
      })
        .then((un) => {
          if (cancelled) un();
          else unlisten = un;
        })
        .catch(() => {
          // Tauri events are unavailable in browser preview; the safety poll
          // and visibilitychange listener still cover the gap.
        });
    }

    // Safety poll: covers the rare case where the native watcher misses an
    // event (permission denied on /Volumes, exotic Linux mount path, …).
    const tick = async () => {
      if (cancelled) return;
      if (document.visibilityState === 'visible') {
        await loadDevices(true);
      }
      if (!cancelled) {
        timer = window.setTimeout(tick, 15000);
      }
    };
    timer = window.setTimeout(tick, 15000);

    // Refresh immediately when the window comes back to the foreground.
    const onVisibility = () => {
      if (document.visibilityState === 'visible') {
        loadDevices(true);
      }
    };
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
      if (unlisten) unlisten();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [loadDevices]);

  useEffect(() => {
    let cancelled = false;
    const importedDevices = devices.filter((device) => device.type === 'image');

    for (const device of importedDevices) {
      if (Object.prototype.hasOwnProperty.call(importedSourceStatuses, device.id)) {
        continue;
      }

      setImportedSourceLoading((prev) => ({ ...prev, [device.id]: true }));
      setImportedSourceErrors((prev) => ({ ...prev, [device.id]: null }));

      void fetchImportedRecoverySourceStatus(device.id)
        .then((status) => {
          if (!cancelled) {
            setImportedSourceStatuses((prev) => ({ ...prev, [device.id]: status }));
          }
        })
        .catch((err) => {
          if (!cancelled) {
            setImportedSourceStatuses((prev) => ({ ...prev, [device.id]: null }));
            setImportedSourceErrors((prev) => ({
              ...prev,
              [device.id]: err instanceof Error ? err.message : String(err),
            }));
          }
        })
        .finally(() => {
          if (!cancelled) {
            setImportedSourceLoading((prev) => ({ ...prev, [device.id]: false }));
          }
        });
    }

    return () => {
      cancelled = true;
    };
  }, [devices, importedSourceStatuses]);

  const filteredDevices = useMemo(() => {
    switch (filter) {
      case 'removable':
        return devices.filter((d) => REMOVABLE_TYPES.includes(d.type));
      case 'internal':
        return devices.filter((d) => INTERNAL_TYPES.includes(d.type));
      case 'images':
        return devices.filter((d) => d.type === 'image');
      default:
        return devices;
    }
  }, [devices, filter]);

  const groupedDevices = useMemo(() => groupByType(filteredDevices), [filteredDevices]);

  const handleToggleSmart = useCallback(
    async (device: DetectedDevice) => {
      const isExpanded = smartExpanded[device.id];
      setSmartExpanded((prev) => ({ ...prev, [device.id]: !isExpanded }));

      // Load SMART data if not already loaded
      if (!isExpanded && smartReports[device.id] === undefined) {
        setSmartLoading((prev) => ({ ...prev, [device.id]: true }));
        setSmartErrors((prev) => ({ ...prev, [device.id]: null }));
        try {
          const report = await fetchSmartReport(device.id);
          setSmartReports((prev) => ({ ...prev, [device.id]: report }));
        } catch (err) {
          setSmartReports((prev) => ({ ...prev, [device.id]: null }));
          setSmartErrors((prev) => ({
            ...prev,
            [device.id]: err instanceof Error ? err.message : String(err),
          }));
        } finally {
          setSmartLoading((prev) => ({ ...prev, [device.id]: false }));
        }
      }
    },
    [smartExpanded, smartReports],
  );

  const handleToggleAdvanced = useCallback(
    async (device: DetectedDevice) => {
      const isExpanded = advancedExpanded[device.id];
      setAdvancedExpanded((prev) => ({ ...prev, [device.id]: !isExpanded }));

      if (!isExpanded && advancedSignals[device.id] === undefined) {
        setAdvancedLoading((prev) => ({ ...prev, [device.id]: true }));
        setAdvancedErrors((prev) => ({ ...prev, [device.id]: null }));
        try {
          const [raidResult, encryptionResult] = await Promise.allSettled([
            fetchRaidMetadata(device.id),
            fetchEncryptionInfo(device.id),
          ]);

          setAdvancedSignals((prev) => ({
            ...prev,
            [device.id]: {
              raidMetadata: raidResult.status === 'fulfilled' ? raidResult.value : null,
              encryptionInfo:
                encryptionResult.status === 'fulfilled' ? encryptionResult.value : null,
            },
          }));

          const errors = [
            raidResult.status === 'rejected' ? raidResult.reason : null,
            encryptionResult.status === 'rejected' ? encryptionResult.reason : null,
          ]
            .filter(Boolean)
            .map((error) => (error instanceof Error ? error.message : String(error)));

          if (errors.length > 0) {
            setAdvancedErrors((prev) => ({ ...prev, [device.id]: errors.join(' | ') }));
          }
        } finally {
          setAdvancedLoading((prev) => ({ ...prev, [device.id]: false }));
        }
      }
    },
    [advancedExpanded, advancedSignals],
  );

  const handleDiagnose = (device: DetectedDevice) => {
    if (device.type === 'image') {
      const importedStatus = importedSourceStatuses[device.id];
      if (importedSourceLoading[device.id] || importedStatus === undefined) {
        setNotice(t('devices.imported_status_loading'));
        return;
      }
      if (!importedStatus?.sourceAvailable) {
        setNotice(t('devices.imported_status_missing'));
        return;
      }
      if ((importedStatus?.requiresPreparation ?? false) && !importedStatus?.prepared) {
        setNotice(t('devices.imported_diagnose_blocked'));
        return;
      }
    }

    useAppStore.getState().selectDevice(device.id);
    navigate('/diagnostic');
  };

  const requestCreateImage = (device: DetectedDevice) => {
    setPendingImageDevice(device);
    setConfirmImageOpen(true);
  };

  const requestRemoveSource = (device: DetectedDevice) => {
    setPendingRemoveDevice(device);
    setConfirmRemoveOpen(true);
  };

  const handleCreateImage = useCallback(
    async (device: DetectedDevice) => {
      setConfirmImageOpen(false);
      setPendingImageDevice(null);
      useAppStore.getState().selectDevice(device.id);
      if (!runtimeCapabilities?.imagingEngine) {
        setNotice(t('devices.imaging_unavailable'));
        return;
      }
      let imagingRequiresElevation = false;
      let imagingProfile: 'standard' | 'cautious' = 'standard';
      let imagingProfileReasonKey = 'imaging.profile_reason_standard';
      try {
        const diagnostic = await fetchDiagnostic(device.id);
        if (!diagnostic.imagingReady) {
          setNotice(diagnostic.imagingBlockReason ?? t('devices.imaging_unavailable'));
          return;
        }
        imagingRequiresElevation = diagnostic.imagingRequiresElevation;
        imagingProfile = diagnostic.imagingProfile;
        imagingProfileReasonKey = diagnostic.imagingProfileReasonKey;
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to validate imaging source');
        return;
      }
      setScanConfig({
        deviceId: device.id,
        scanType: 'image',
        targetFilesystems: [device.filesystem],
        enableCarving: false,
        imagingRequiresElevation,
        imagingProfile,
        imagingProfileReasonKey,
      });
      navigate('/scan');
    },
    [runtimeCapabilities, setScanConfig, navigate, t],
  );

  const handleImportSource = useCallback(async () => {
    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setNotice(t('devices.import_unavailable'));
      return;
    }

    try {
      const selectedPath = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: 'Recovery Sources',
            extensions: ['img', 'dd', 'raw', 'bin', 'e01', 'vmdk', 'vhd', 'vhdx'],
          },
        ],
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        return;
      }

      const importedStatus = await importRecoverySource(selectedPath);
      await loadDevices(false);
      const sourceKind = getImportedSourceKind(importedStatus, null);
      const sourceClassLabelKey =
        sourceKind === 'raid-analysis'
          ? 'devices.imported_source_class_raid'
          : sourceKind === 'forensic-image'
            ? 'devices.imported_source_class_forensic'
            : sourceKind === 'virtual-disk'
              ? 'devices.imported_source_class_virtual'
              : sourceKind === 'raw-image'
                ? 'devices.imported_source_class_raw'
                : 'devices.imported_source_class_generic';
      setNotice(
        importedStatus.requiresPreparation
          ? t('devices.import_success_preparation_needed', {
              name: importedStatus.displayName,
              format: importedStatus.sourceFormat,
              classLabel: t(sourceClassLabelKey),
              logicalSize: formatBytes(importedStatus.logicalSizeBytes),
            })
          : t('devices.import_success_ready', {
              name: importedStatus.displayName,
              format: importedStatus.sourceFormat,
              classLabel: t(sourceClassLabelKey),
              logicalSize: formatBytes(importedStatus.logicalSizeBytes),
            }),
      );
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('devices.import_failed'));
    }
  }, [loadDevices, t]);

  const handleRemoveSource = useCallback(async () => {
    if (!pendingRemoveDevice) {
      setConfirmRemoveOpen(false);
      return;
    }

    try {
      await removeImportedRecoverySource(pendingRemoveDevice.devicePath);
      setConfirmRemoveOpen(false);
      setPendingRemoveDevice(null);
      await loadDevices(false);
      setNotice(t('devices.remove_success'));
      setError(null);
    } catch (err) {
      setConfirmRemoveOpen(false);
      setPendingRemoveDevice(null);
      setError(err instanceof Error ? err.message : t('devices.remove_failed'));
    }
  }, [loadDevices, pendingRemoveDevice, t]);

  const handlePrepareImportedSource = useCallback(
    async (device: DetectedDevice) => {
      setImportedSourcePreparing((prev) => ({ ...prev, [device.id]: true }));
      setImportedSourceErrors((prev) => ({ ...prev, [device.id]: null }));

      try {
        const status = await prepareImportedRecoverySource(device.id);
        setImportedSourceStatuses((prev) => ({ ...prev, [device.id]: status }));
        setNotice(
          t('devices.imported_prepare_success', {
            format: status.sourceFormat,
          }),
        );
        setError(null);
      } catch (err) {
        setImportedSourceErrors((prev) => ({
          ...prev,
          [device.id]: err instanceof Error ? err.message : t('devices.imported_prepare_failed'),
        }));
      } finally {
        setImportedSourcePreparing((prev) => ({ ...prev, [device.id]: false }));
      }
    },
    [t],
  );

  if (devicesLoading) {
    return <LoadingState message={t('common.loading')} />;
  }

  if (error) {
    return <ErrorState title={t('common.error')} description={error} onRetry={loadDevices} />;
  }

  const filters: { key: DeviceFilter; label: string }[] = [
    { key: 'all', label: t('devices.filter_all') },
    { key: 'internal', label: t('devices.filter_internal') },
    { key: 'removable', label: t('devices.filter_removable') },
    { key: 'images', label: t('devices.filter_images') },
  ];

  return (
    <div>
      <PageHeader
        title={t('devices.title')}
        subtitle={t('devices.subtitle')}
        actions={
          <>
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => loadDevices(false)}
            >
              <RefreshCw size={14} />
              {t('devices.refresh')}
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={handleImportSource}>
              <FolderOpen size={14} />
              {t('devices.import_source')}
            </button>
          </>
        }
      />

      {notice && (
        <div className="mb-6">
          <ErrorState title={t('common.unavailable')} description={notice} />
        </div>
      )}

      {/* Filter bar */}
      <div className="device-filter-bar mb-6">
        {filters.map((f) => (
          <button
            type="button"
            key={f.key}
            className={`btn btn-sm ${filter === f.key ? 'btn-primary' : 'btn-ghost'}`}
            onClick={() => setFilter(f.key)}
          >
            {f.label}
            {f.key !== 'all' && (
              <span className="ml-1 text-xs">
                (
                {f.key === 'removable'
                  ? devices.filter((d) => REMOVABLE_TYPES.includes(d.type)).length
                  : f.key === 'internal'
                    ? devices.filter((d) => INTERNAL_TYPES.includes(d.type)).length
                    : devices.filter((d) => d.type === 'image').length}
                )
              </span>
            )}
          </button>
        ))}
      </div>

      {filteredDevices.length === 0 ? (
        <EmptyState title={t('devices.empty')} description={t('devices.empty_desc')} />
      ) : (
        <div className="flex flex-col gap-6">
          {Array.from(groupedDevices.entries()).map(([type, typeDevices]) => (
            <div key={type}>
              <h3
                className="text-sm font-semibold text-secondary mb-3"
                style={{ textTransform: 'uppercase', letterSpacing: '0.05em' }}
              >
                {t(`device_type.${type}`)} ({typeDevices.length})
              </h3>
              <div className="flex flex-col gap-3">
                {typeDevices.map((device) => (
                  <div key={device.id} className="flex flex-col gap-2">
                    {(() => {
                      const importedStatus = importedSourceStatuses[device.id];
                      const importedStatusLoading =
                        device.type === 'image' &&
                        (importedSourceLoading[device.id] || importedStatus === undefined);
                      const diagnoseDisabled =
                        device.type === 'image' &&
                        (importedStatusLoading ||
                          !importedStatus?.sourceAvailable ||
                          ((importedStatus?.requiresPreparation ?? false) &&
                            !importedStatus?.prepared));
                      const diagnoseDisabledReason = diagnoseDisabled
                        ? !importedStatus?.sourceAvailable
                          ? t('devices.imported_status_missing')
                          : importedStatusLoading
                            ? t('devices.imported_status_loading')
                            : t('devices.imported_diagnose_blocked')
                        : undefined;
                      const advancedDisabled =
                        device.type === 'image' &&
                        (importedStatusLoading ||
                          !importedStatus?.sourceAvailable ||
                          ((importedStatus?.requiresPreparation ?? false) &&
                            !importedStatus?.prepared));
                      const advancedDisabledReason = advancedDisabled
                        ? !importedStatus?.sourceAvailable
                          ? t('devices.imported_status_missing')
                          : importedStatusLoading
                            ? t('devices.imported_status_loading')
                            : t('devices.imported_advanced_blocked')
                        : undefined;

                      return (
                        <>
                          <DeviceCard
                            device={device}
                            onDiagnose={handleDiagnose}
                            onCreateImage={requestCreateImage}
                            onRemoveSource={requestRemoveSource}
                            diagnoseDisabled={diagnoseDisabled}
                            diagnoseDisabledReason={diagnoseDisabledReason}
                          />
                          {device.type === 'image' && (
                            <ImportedSourceStatusPanel
                              device={device}
                              status={importedStatus ?? null}
                              loading={importedStatusLoading}
                              preparing={!!importedSourcePreparing[device.id]}
                              error={importedSourceErrors[device.id]}
                              onPrepare={() => {
                                void handlePrepareImportedSource(device);
                              }}
                            />
                          )}
                          <div className="flex">
                            <button
                              type="button"
                              className="btn btn-ghost btn-xs"
                              onClick={() => handleToggleSmart(device)}
                            >
                              {smartExpanded[device.id] ? '▾' : '▸'} {t('devices.smart_title')}
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost btn-xs"
                              onClick={() => {
                                if (advancedDisabled) {
                                  setNotice(
                                    advancedDisabledReason ??
                                      t('devices.imported_advanced_blocked'),
                                  );
                                  return;
                                }
                                void handleToggleAdvanced(device);
                              }}
                              disabled={advancedDisabled}
                              title={advancedDisabled ? advancedDisabledReason : undefined}
                            >
                              {advancedExpanded[device.id] ? '▾' : '▸'}{' '}
                              {t('devices.advanced_title')}
                            </button>
                          </div>
                          {smartExpanded[device.id] &&
                            (smartErrors[device.id] ? (
                              <div className="mt-2">
                                <WarningBanner variant="danger">
                                  {t('results.smart_load_failed', {
                                    error: smartErrors[device.id],
                                  })}
                                </WarningBanner>
                              </div>
                            ) : (
                              <SmartDashboard
                                report={smartReports[device.id] ?? null}
                                loading={!!smartLoading[device.id]}
                              />
                            ))}
                          {advancedExpanded[device.id] && (
                            <DeviceInsightsPanel
                              raidMetadata={advancedSignals[device.id]?.raidMetadata ?? null}
                              encryptionInfo={advancedSignals[device.id]?.encryptionInfo ?? null}
                              loading={!!advancedLoading[device.id]}
                              error={advancedErrors[device.id]}
                            />
                          )}
                        </>
                      );
                    })()}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      <RemoteAgentsSection />

      <ConfirmDialog
        open={confirmImageOpen}
        title={t('devices.confirm_image_title')}
        description={t('devices.confirm_image_desc')}
        variant="warning"
        confirmLabel={t('devices.create_image')}
        onConfirm={() => pendingImageDevice && handleCreateImage(pendingImageDevice)}
        onCancel={() => {
          setConfirmImageOpen(false);
          setPendingImageDevice(null);
        }}
      />

      <ConfirmDialog
        open={confirmRemoveOpen}
        title={t('devices.confirm_remove_source_title')}
        description={t('devices.confirm_remove_source_desc')}
        variant="warning"
        confirmLabel={t('devices.remove_source')}
        onConfirm={handleRemoveSource}
        onCancel={() => {
          setConfirmRemoveOpen(false);
          setPendingRemoveDevice(null);
        }}
      />
    </div>
  );
}
