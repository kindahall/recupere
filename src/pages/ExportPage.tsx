import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { EmptyState } from '../components/common/EmptyState';
import { WarningBanner } from '../components/common/WarningBanner';
import { ExportWizard } from '../components/export/ExportWizard';
import { PageHeader } from '../components/layout/PageHeader';
import {
  activateLicense,
  fetchExportLogs,
  fetchExportProgress,
  fetchRecoveryContextHints,
  startExport,
  validateExportDestination,
} from '../hooks/useIpc';
import { useSelectedImportedSourceStatus } from '../hooks/useSelectedImportedSourceStatus';
import { useAppStore } from '../stores/appStore';
import type { RecoveryContextHint } from '../types';
import {
  applyRecoveryContextHints,
  buildRecoveryContextLookupInputs,
  collectFilesystemContextRoots,
  countFilesystemMemoryReprioritizedFiles,
  pathsAppearRelated,
} from '../utils/filesystemMemoryContext';
import { isRaidImportedSource } from '../utils/importedSourceKind';
import { buildDefaultExportBatch, isApfsCatalogReassembledCandidate } from '../utils/resultFilters';

export function ExportPage() {
  const { t } = useTranslation();
  const {
    devices,
    selectedDeviceId,
    activeScanId,
    scanConfig,
    recoveryResult,
    filesystemMemoryComparison,
    selectedFileIds,
    runtimeCapabilities,
    activeExportId,
    exportProgress,
    exportLogs,
    setActiveExportId,
    setExportProgress,
    setExportLogs,
    clearExportLogs,
  } = useAppStore();
  const selectedDevice = devices.find((d) => d.id === selectedDeviceId);
  const { status: importedSourceStatus } = useSelectedImportedSourceStatus(selectedDevice);
  const raidImportedSource = isRaidImportedSource(importedSourceStatus, selectedDevice);

  const [destination, setDestination] = useState('');
  const [validationMsg, setValidationMsg] = useState<string | null>(null);
  const [isSafe, setIsSafe] = useState<boolean | null>(null);
  const [availableSpace, setAvailableSpace] = useState<number>(0);
  const [validating, setValidating] = useState(false);
  const [conflictStrategy, setConflictStrategy] = useState<'rename' | 'skip' | 'overwrite'>(
    'rename',
  );
  const [preserveStructure, setPreserveStructure] = useState(true);
  const [verifyIntegrity, setVerifyIntegrity] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [logsError, setLogsError] = useState<string | null>(null);
  const [filesystemMemoryHints, setFilesystemMemoryHints] = useState<RecoveryContextHint[]>([]);
  const [filesystemMemoryLoading, setFilesystemMemoryLoading] = useState(false);
  const [filesystemMemoryError, setFilesystemMemoryError] = useState<string | null>(null);

  const filesystemContextRoots = useMemo(
    () => collectFilesystemContextRoots(selectedDevice, importedSourceStatus),
    [importedSourceStatus, selectedDevice],
  );
  const filesystemMemoryApplicable = Boolean(
    filesystemMemoryComparison &&
      filesystemContextRoots.some((root) =>
        pathsAppearRelated(root, filesystemMemoryComparison.targetPath),
      ),
  );
  const recoveryFiles = useMemo(
    () => applyRecoveryContextHints(recoveryResult?.files ?? [], filesystemMemoryHints),
    [filesystemMemoryHints, recoveryResult?.files],
  );
  const implicitExportBatch = useMemo(
    () => buildDefaultExportBatch(recoveryFiles),
    [recoveryFiles],
  );
  const filesToExport =
    selectedFileIds.size === 0
      ? implicitExportBatch.files
      : recoveryFiles.filter((file) => selectedFileIds.has(file.id));
  const implicitPreviewFirstExcludedCount =
    selectedFileIds.size === 0 ? implicitExportBatch.excludedPreviewFirstCount : 0;

  const deletedRecoveryExport = filesToExport.some((file) => file.isDeleted);
  const carvedExport = filesToExport.some((file) => file.recoveryMethod === 'carving');
  const recoveredFilesystem = scanConfig?.potentialVolumeFilesystem ?? selectedDevice?.filesystem;
  const lostVolumeMode =
    scanConfig?.deviceId === selectedDeviceId && scanConfig?.scanType === 'lost-volume';

  const resourceForkFilesCount = filesToExport.filter((file) => file.resourceFork).length;
  const resourceForkBytes = filesToExport.reduce(
    (sum, file) => sum + (file.resourceFork?.sizeBytes ?? 0),
    0,
  );
  const alternateDataStreamsCount = filesToExport.reduce(
    (sum, file) => sum + (file.alternateDataStreams?.length ?? 0),
    0,
  );
  const alternateDataStreamsBytes = filesToExport.reduce(
    (sum, file) =>
      sum + (file.alternateDataStreams?.reduce((s, stream) => s + stream.sizeBytes, 0) ?? 0),
    0,
  );
  const snapshotFilesCount = filesToExport.filter((file) => file.sourceView === 'snapshot').length;
  const journalFilesCount = filesToExport.filter(
    (file) => file.sourceView === 'journal' || file.journalDerived,
  ).length;
  const apfsCurrentCatalogFilesCount = filesToExport.filter(
    (file) => file.sourceView === 'live-catalog' || file.sourceView === 'mounted-volume',
  ).length;
  const apfsReassembledFilesCount = filesToExport.filter((file) =>
    isApfsCatalogReassembledCandidate(file),
  ).length;
  const compressedFilesCount = filesToExport.filter((file) => Boolean(file.compressionKind)).length;
  const highComplexityFilesCount = filesToExport.filter(
    (file) => file.recoveryComplexity === 'high' || (file.assemblySegmentCount ?? 1) > 1,
  ).length;
  const filesystemMemoryContextFilesCount = filesToExport.filter(
    (file) => file.filesystemMemoryContext,
  ).length;
  const filesystemMemoryAdjustedFilesCount = countFilesystemMemoryReprioritizedFiles(filesToExport);

  const requiredBytes = filesToExport.reduce(
    (sum, file) =>
      sum +
      file.sizeBytes +
      (file.resourceFork?.sizeBytes ?? 0) +
      (file.alternateDataStreams?.reduce((s, stream) => s + stream.sizeBytes, 0) ?? 0),
    0,
  );

  const exportPercent =
    exportProgress && exportProgress.totalBytes > 0
      ? Math.min(100, (exportProgress.exportedBytes / exportProgress.totalBytes) * 100)
      : exportProgress && exportProgress.totalFiles > 0
        ? Math.min(100, (exportProgress.exportedFiles / exportProgress.totalFiles) * 100)
        : 0;
  const exportRunning =
    exportProgress?.status === 'preparing' ||
    exportProgress?.status === 'exporting' ||
    exportProgress?.status === 'verifying';

  useEffect(() => {
    if (!activeExportId) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const progress = await fetchExportProgress(activeExportId);
        if (!cancelled) setExportProgress(progress);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : t('export.progress_failed'));
      }
      try {
        const logs = await fetchExportLogs(activeExportId);
        if (!cancelled) {
          setExportLogs(logs);
          setLogsError(null);
        }
      } catch (err) {
        if (!cancelled) setLogsError(err instanceof Error ? err.message : t('export.log_failed'));
      }
    };

    poll();
    if (!exportRunning && exportProgress)
      return () => {
        cancelled = true;
      };
    const intervalId = window.setInterval(poll, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [activeExportId, exportProgress, exportRunning, setExportLogs, setExportProgress, t]);

  useEffect(() => {
    if (
      !activeScanId ||
      !recoveryResult ||
      recoveryResult.scanId !== activeScanId ||
      !filesystemMemoryComparison ||
      !filesystemMemoryApplicable
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
            err instanceof Error ? err.message : t('export.filesystem_memory_context_error'),
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
  }, [activeScanId, filesystemMemoryApplicable, filesystemMemoryComparison, recoveryResult, t]);

  const handleValidateDestination = async () => {
    if (!destination || !selectedDevice || !runtimeCapabilities?.exportValidation) return;
    setValidating(true);
    setValidationMsg(t('export.validating'));
    try {
      const result = await validateExportDestination(destination, selectedDevice.devicePath);
      setIsSafe(result.isSafe);
      setAvailableSpace(result.availableBytes);
      setValidationMsg(result.message);
      setError(null);
    } catch (err) {
      setValidationMsg(err instanceof Error ? err.message : t('export.validation_unknown'));
      setIsSafe(false);
    } finally {
      setValidating(false);
    }
  };

  const handleStartExport = async () => {
    if (!activeScanId || !destination || isSafe !== true || !runtimeCapabilities?.exportEngine)
      return;
    setError(null);
    setLogsError(null);
    clearExportLogs();
    setActiveExportId(null);
    setExportProgress(null);
    try {
      const exportId = await startExport(activeScanId, {
        destinationPath: destination,
        selectedFileIds: filesToExport.map((file) => file.id),
        conflictStrategy,
        preserveStructure,
        verifyIntegrity,
        explicitSelection: selectedFileIds.size > 0,
        implicitPreviewFirstExcludedCount,
      });
      setActiveExportId(exportId);
    } catch (err) {
      setError(err instanceof Error ? err.message : t('export.start_failed'));
    }
  };

  const licenseStatus = useAppStore((s) => s.licenseStatus);
  const setLicenseStatus = useAppStore((s) => s.setLicenseStatus);
  const [licenseKey, setLicenseKey] = useState('');
  const [activating, setActivating] = useState(false);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [purchaseNotice, setPurchaseNotice] = useState<string | null>(null);

  const handleActivate = async () => {
    const trimmed = licenseKey.trim();
    if (trimmed.length === 0) return;
    setActivating(true);
    setActivationError(null);
    try {
      const info = await activateLicense(trimmed);
      if (info.valid) {
        setLicenseStatus('pro');
        setLicenseKey('');
      } else {
        setActivationError(info.message);
      }
    } catch (err) {
      setActivationError(err instanceof Error ? err.message : 'Activation failed');
    } finally {
      setActivating(false);
    }
  };

  const handleBuy = () => {
    const url = import.meta.env.VITE_STRIPE_CHECKOUT_URL?.trim();
    if (!url) {
      setPurchaseNotice(t('paywall.checkout_unavailable'));
      return;
    }
    setPurchaseNotice(null);
    window.open(url, '_blank', 'noopener,noreferrer');
  };

  const deletedSubtitle =
    recoveredFilesystem === 'ntfs'
      ? t('export.deleted_ntfs_subtitle')
      : recoveredFilesystem === 'exfat'
        ? t('export.deleted_exfat_subtitle')
        : recoveredFilesystem === 'apfs'
          ? t('export.deleted_apfs_subtitle')
          : recoveredFilesystem === 'hfs+'
            ? t('export.deleted_hfsplus_subtitle')
            : recoveredFilesystem === 'ext4'
              ? t('export.deleted_ext4_subtitle')
              : t('export.deleted_fat32_subtitle');

  const pageSubtitle = carvedExport
    ? t('export.carved_subtitle')
    : lostVolumeMode
      ? t('export.lost_volume_subtitle', {
          filesystem: (recoveredFilesystem ?? 'unknown').toUpperCase(),
          label: scanConfig?.potentialVolumeLabel ?? t('scan.lost_volume'),
        })
      : deletedRecoveryExport
        ? deletedSubtitle
        : raidImportedSource
          ? t('export.raid_source_subtitle')
          : t('export.subtitle');
  const raidAnalysisPath = importedSourceStatus?.analysisPath ?? importedSourceStatus?.sourcePath;
  const apfsExportOperatorVisible =
    recoveredFilesystem === 'apfs' || snapshotFilesCount > 0 || journalFilesCount > 0;

  const hasExportableSelection = Boolean(
    recoveryResult && activeScanId && filesToExport.length > 0,
  );

  if (licenseStatus === 'free') {
    return (
      <div>
        <PageHeader
          title={t('export.title')}
          subtitle={hasExportableSelection ? pageSubtitle : t('paywall.description')}
        />

        {!hasExportableSelection && (
          <div className="mb-6">
            <WarningBanner variant="info">{t('export.empty_desc')}</WarningBanner>
          </div>
        )}

        {purchaseNotice && (
          <div className="mb-6">
            <WarningBanner variant="warning">{purchaseNotice}</WarningBanner>
          </div>
        )}

        <div style={{ maxWidth: 500, margin: '0 auto', padding: 'var(--space-8)' }}>
          {/* Header */}
          <div style={{ textAlign: 'center', marginBottom: 'var(--space-6)' }}>
            <div
              style={{
                width: 64,
                height: 64,
                borderRadius: '50%',
                background: 'linear-gradient(135deg, var(--color-accent), #06B6D4)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                margin: '0 auto var(--space-4)',
                boxShadow: '0 0 30px rgba(59, 130, 246, 0.3)',
              }}
            >
              <span style={{ fontSize: 28, color: 'white' }}>&#9889;</span>
            </div>
            <h2
              style={{
                fontSize: 'var(--font-size-xl)',
                fontWeight: 700,
                marginBottom: 'var(--space-2)',
              }}
            >
              Recupere Pro
            </h2>
            <p style={{ color: 'var(--color-text-secondary)', fontSize: 'var(--font-size-md)' }}>
              {t('paywall.description')}
            </p>
          </div>

          {/* What you get */}
          <div
            style={{
              background: 'var(--color-bg-elevated)',
              border: '2px solid var(--color-accent)',
              borderRadius: 'var(--radius-xl)',
              padding: 'var(--space-6)',
              marginBottom: 'var(--space-6)',
            }}
          >
            <div
              style={{
                fontSize: 'var(--font-size-sm)',
                color: 'var(--color-text-primary)',
                lineHeight: 2,
              }}
            >
              <div>&#10003; {t('paywall.pro_1')}</div>
              <div>&#10003; {t('paywall.pro_2')}</div>
              <div>&#10003; {t('paywall.pro_3')}</div>
              <div>&#10003; {t('paywall.pro_4')}</div>
            </div>
          </div>

          {/* Buy button — opens Stripe Checkout */}
          <button
            type="button"
            className="btn btn-primary"
            style={{
              width: '100%',
              padding: 'var(--space-4)',
              fontSize: 'var(--font-size-md)',
              fontWeight: 700,
              marginBottom: 'var(--space-4)',
            }}
            onClick={handleBuy}
          >
            {t('paywall.buy_now')}
          </button>

          {/* Already have a key */}
          <div
            style={{
              borderTop: '1px solid var(--color-border)',
              paddingTop: 'var(--space-4)',
              marginTop: 'var(--space-2)',
            }}
          >
            <p
              className="text-sm text-secondary"
              style={{ marginBottom: 'var(--space-3)', textAlign: 'center' }}
            >
              {t('paywall.already_purchased')}
            </p>
            <div style={{ display: 'flex', gap: 'var(--space-3)' }}>
              <input
                type="text"
                className="input input-sm"
                style={{ flex: 1, fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-xs)' }}
                value={licenseKey}
                onChange={(e) => {
                  setLicenseKey(e.target.value);
                  setActivationError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleActivate();
                }}
                placeholder={t('paywall.license_placeholder')}
                disabled={activating}
              />
              <button
                type="button"
                className="btn btn-secondary"
                onClick={handleActivate}
                disabled={activating || licenseKey.trim().length === 0}
              >
                {activating ? '...' : t('paywall.activate')}
              </button>
            </div>
            {activationError && (
              <div
                className="text-sm"
                style={{
                  color: 'var(--color-danger)',
                  marginTop: 'var(--space-3)',
                  textAlign: 'center',
                }}
              >
                {activationError}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  if (!hasExportableSelection) {
    return (
      <div>
        <PageHeader title={t('export.title')} subtitle={t('export.subtitle')} />
        <EmptyState title={t('export.empty_title')} description={t('export.empty_desc')} />
      </div>
    );
  }

  return (
    <div>
      <PageHeader title={t('export.title')} subtitle={pageSubtitle} />

      {raidImportedSource && (
        <div className="mb-6">
          <WarningBanner variant="success">
            {t('export.raid_source_notice', {
              path: raidAnalysisPath ?? '—',
            })}
          </WarningBanner>
        </div>
      )}

      {filesystemMemoryLoading && filesystemMemoryApplicable && (
        <div className="mb-6">
          <WarningBanner variant="info">
            {t('export.filesystem_memory_context_loading')}
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
        filesystemMemoryContextFilesCount > 0 && (
          <div className="mb-6">
            <WarningBanner variant="info">
              {t('export.filesystem_memory_context_summary', {
                count: filesystemMemoryContextFilesCount,
                targetPath: filesystemMemoryComparison?.targetPath ?? '—',
              })}
            </WarningBanner>
          </div>
        )}

      {!filesystemMemoryLoading &&
        !filesystemMemoryError &&
        filesystemMemoryApplicable &&
        filesystemMemoryAdjustedFilesCount > 0 && (
          <div className="mb-6">
            <WarningBanner variant="info">
              {t('export.filesystem_memory_score_notice', {
                count: filesystemMemoryAdjustedFilesCount,
              })}
            </WarningBanner>
          </div>
        )}

      <div className="mb-6">
        <WarningBanner variant="danger">{t('export.destination_warning')}</WarningBanner>
      </div>

      <ExportWizard
        files={filesToExport}
        implicitPreviewFirstExcludedCount={implicitPreviewFirstExcludedCount}
        requiredBytes={requiredBytes}
        resourceForkFilesCount={resourceForkFilesCount}
        resourceForkBytes={resourceForkBytes}
        alternateDataStreamsCount={alternateDataStreamsCount}
        alternateDataStreamsBytes={alternateDataStreamsBytes}
        apfsExportOperatorVisible={apfsExportOperatorVisible}
        apfsCurrentCatalogFilesCount={apfsCurrentCatalogFilesCount}
        apfsReassembledFilesCount={apfsReassembledFilesCount}
        snapshotFilesCount={snapshotFilesCount}
        journalFilesCount={journalFilesCount}
        compressedFilesCount={compressedFilesCount}
        highComplexityFilesCount={highComplexityFilesCount}
        filesystemMemoryContextFilesCount={filesystemMemoryContextFilesCount}
        filesystemMemoryAdjustedFilesCount={filesystemMemoryAdjustedFilesCount}
        deletedRecoveryExport={deletedRecoveryExport}
        carvedExport={carvedExport}
        lostVolumeMode={lostVolumeMode}
        recoveredFilesystem={recoveredFilesystem}
        scanConfigLabel={scanConfig?.potentialVolumeLabel}
        raidImportedSource={raidImportedSource}
        raidAnalysisPath={raidAnalysisPath}
        destination={destination}
        setDestination={(d) => {
          setDestination(d);
          setValidationMsg(null);
          setIsSafe(null);
        }}
        conflictStrategy={conflictStrategy}
        setConflictStrategy={setConflictStrategy}
        preserveStructure={preserveStructure}
        setPreserveStructure={setPreserveStructure}
        verifyIntegrity={verifyIntegrity}
        setVerifyIntegrity={setVerifyIntegrity}
        isSafe={isSafe}
        validationMsg={validationMsg}
        availableSpace={availableSpace}
        onValidateDestination={handleValidateDestination}
        validating={validating}
        exportValidationAvailable={!!runtimeCapabilities?.exportValidation}
        exportEngineAvailable={!!runtimeCapabilities?.exportEngine}
        exportRunning={exportRunning}
        exportProgress={exportProgress}
        exportLogs={exportLogs}
        exportPercent={exportPercent}
        activeExportId={activeExportId}
        logsError={logsError}
        error={error}
        onStartExport={handleStartExport}
      />
    </div>
  );
}
