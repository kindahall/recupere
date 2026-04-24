import { isTauri } from '@tauri-apps/api/core';
import { AlertTriangle, ChevronDown, ChevronUp, Disc, Search, Shield, Zap } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { EmptyState } from '../components/common/EmptyState';
import { ErrorState } from '../components/common/ErrorState';
import { LoadingState } from '../components/common/LoadingState';
import { RiskBadge } from '../components/common/RiskBadge';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { ImportedSourceStatusPanel } from '../components/device/ImportedSourceStatusPanel';
import { AiAdvisoryPanel } from '../components/diagnostic/AiAdvisoryPanel';
import { RecoveryScore } from '../components/diagnostic/RecoveryScore';
import { PageHeader } from '../components/layout/PageHeader';
import {
  type DiagnosticData,
  fetchAiAdvisory,
  fetchDiagnostic,
  fetchEncryptionInfo,
} from '../hooks/useIpc';
import { useSelectedImportedSourceStatus } from '../hooks/useSelectedImportedSourceStatus';
import { useAppStore } from '../stores/appStore';
import type { AiAdvisory, EncryptionInfo, RiskLevel, ScanType } from '../types';
import { resolveKey } from '../utils/i18nResolve';
import { isRaidImportedSource } from '../utils/importedSourceKind';
import { buildPotentialVolumeScanConfig } from '../utils/potentialVolumeRecovery';

const recIcons: Record<string, typeof Zap> = {
  'scan-quick': Zap,
  'scan-deep': Search,
  'scan-lost-volume': Disc,
  'scan-deleted-ntfs': Disc,
  'scan-deleted-fat32': Disc,
  'scan-deleted-exfat': Disc,
  'scan-deleted-ext4': Disc,
  'scan-deleted-hfsplus': Disc,
  'scan-deleted-apfs': Disc,
  'scan-signature-carving': Search,
  'review-potential-volumes': Search,
  'image-first': Disc,
  'stop-usage': AlertTriangle,
  'professional-help': Shield,
};

function isActionAvailable(type: string, scanEngine: boolean, imagingEngine: boolean) {
  if (type === 'review-potential-volumes') return true;
  if (type === 'scan-lost-volume' || type === 'scan-signature-carving') {
    return scanEngine && imagingEngine;
  }
  if (
    type === 'scan-quick' ||
    type === 'scan-deep' ||
    type === 'scan-deleted-ntfs' ||
    type === 'scan-deleted-fat32' ||
    type === 'scan-deleted-exfat' ||
    type === 'scan-deleted-ext4' ||
    type === 'scan-deleted-hfsplus' ||
    type === 'scan-deleted-apfs'
  ) {
    return scanEngine;
  }
  if (type === 'image-first') return imagingEngine;
  return false;
}

function recommendationNeedsImaging(type: string) {
  return type === 'image-first' || type === 'scan-signature-carving' || type === 'scan-lost-volume';
}

function isRecommendationActionable(type: string) {
  return (
    type === 'scan-quick' ||
    type === 'scan-deep' ||
    type === 'scan-lost-volume' ||
    type === 'scan-deleted-ntfs' ||
    type === 'scan-deleted-fat32' ||
    type === 'scan-deleted-exfat' ||
    type === 'scan-deleted-ext4' ||
    type === 'scan-deleted-hfsplus' ||
    type === 'scan-deleted-apfs' ||
    type === 'scan-signature-carving' ||
    type === 'image-first' ||
    type === 'review-potential-volumes'
  );
}

export function DiagnosticPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    userMode,
    selectedDeviceId,
    devices,
    runtimeCapabilities,
    runtimeCapabilitiesLoading,
    setScanConfig,
    setActiveScanId,
    setScanProgress,
    clearScanLogs,
    setRecoveryResult,
  } = useAppStore();
  const [diagnostic, setDiagnostic] = useState<DiagnosticData | null>(null);
  const [aiAdvisory, setAiAdvisory] = useState<AiAdvisory | null>(null);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const [encryptionInfo, setEncryptionInfo] = useState<EncryptionInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);

  const selectedDevice = devices.find((d) => d.id === selectedDeviceId);
  const isSsd = selectedDevice?.type === 'ssd' || selectedDevice?.type === 'nvme';
  const {
    status: importedSourceStatus,
    loading: importedSourceLoading,
    preparing: importedSourcePreparing,
    error: importedSourceError,
    prepare: prepareImportedSourceFromDiagnostic,
    isImportedSource,
  } = useSelectedImportedSourceStatus(selectedDevice);
  const importedSourceNeedsPreparation =
    isImportedSource &&
    !importedSourceLoading &&
    Boolean(
      importedSourceStatus?.sourceAvailable &&
        importedSourceStatus.requiresPreparation &&
        !importedSourceStatus.prepared,
    );
  const importedSourceMissing =
    isImportedSource && !importedSourceLoading && importedSourceStatus?.sourceAvailable === false;
  const importedSourceUnsupported =
    isImportedSource &&
    !importedSourceLoading &&
    importedSourceStatus?.supportTier === 'unsupported';
  const importedSourceGateActive =
    isImportedSource &&
    !importedSourceLoading &&
    (importedSourceUnsupported ||
      importedSourceNeedsPreparation ||
      importedSourceMissing ||
      (!importedSourceStatus && Boolean(importedSourceError)));
  const raidImportedSource = isRaidImportedSource(importedSourceStatus, selectedDevice);

  const loadDiagnostic = useCallback(async () => {
    if (!selectedDeviceId) {
      setLoading(false);
      setDiagnostic(null);
      setAiAdvisory(null);
      setAiError(null);
      setAiLoading(false);
      setEncryptionInfo(null);
      return;
    }

    setLoading(true);
    setError(null);
    setAiError(null);
    setAiAdvisory(null);
    setAiLoading(false);
    setEncryptionInfo(null);
    try {
      const [result, encryption] = await Promise.all([
        fetchDiagnostic(selectedDeviceId),
        fetchEncryptionInfo(selectedDeviceId),
      ]);
      setDiagnostic(result);
      setEncryptionInfo(encryption);
      if (runtimeCapabilities?.aiAdvisory) {
        setAiLoading(true);
        try {
          const advisory = await fetchAiAdvisory(selectedDeviceId);
          setAiAdvisory(advisory);
        } catch (advisoryError) {
          setAiError(
            advisoryError instanceof Error ? advisoryError.message : t('diagnostic.ai_unavailable'),
          );
        } finally {
          setAiLoading(false);
        }
      } else {
        setAiLoading(false);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Diagnostic failed');
    } finally {
      setLoading(false);
    }
  }, [selectedDeviceId, runtimeCapabilities?.aiAdvisory, t]);

  const handlePrepareImportedSource = useCallback(async () => {
    const nextStatus = await prepareImportedSourceFromDiagnostic();
    if (!nextStatus) {
      setNotice(importedSourceError ?? t('devices.imported_prepare_failed'));
      return;
    }

    setNotice(
      t('devices.imported_prepare_success', {
        format: nextStatus.sourceFormat,
      }),
    );
  }, [importedSourceError, prepareImportedSourceFromDiagnostic, t]);

  useEffect(() => {
    if (!selectedDeviceId) {
      setLoading(false);
      setDiagnostic(null);
      setAiAdvisory(null);
      setAiError(null);
      setAiLoading(false);
      setEncryptionInfo(null);
      return;
    }

    if (isImportedSource) {
      if (importedSourceLoading) {
        setLoading(true);
        return;
      }

      if (importedSourceGateActive) {
        setLoading(false);
        setDiagnostic(null);
        setAiAdvisory(null);
        setAiError(null);
        setAiLoading(false);
        return;
      }
    }

    void loadDiagnostic();
  }, [
    importedSourceGateActive,
    importedSourceLoading,
    isImportedSource,
    loadDiagnostic,
    selectedDeviceId,
  ]);

  if (!selectedDeviceId || !selectedDevice) {
    return (
      <EmptyState
        title={t('diagnostic.empty_title')}
        description={t('diagnostic.empty_desc')}
        action={
          <button type="button" className="btn btn-primary" onClick={() => navigate('/devices')}>
            {t('home.cta_analyze')}
          </button>
        }
      />
    );
  }

  if (isImportedSource && importedSourceLoading) {
    return <LoadingState message={t('devices.imported_status_loading')} />;
  }

  if (importedSourceGateActive) {
    return (
      <div>
        <PageHeader title={t('diagnostic.title')} subtitle={t('diagnostic.subtitle')} />

        {__ALLOW_BROWSER_PREVIEW__ && !isTauri() && (
          <div className="mb-6">
            <WarningBanner variant="info">{t('diagnostic.preview_notice')}</WarningBanner>
          </div>
        )}

        <div className="mb-6">
          <WarningBanner
            variant={
              importedSourceUnsupported || importedSourceMissing || importedSourceError
                ? 'danger'
                : 'warning'
            }
          >
            {importedSourceUnsupported
              ? (importedSourceStatus?.supportNote ??
                t('devices.imported_status_unsupported', {
                  format: importedSourceStatus?.sourceFormat ?? 'source',
                }))
              : importedSourceMissing
                ? t('diagnostic.imported_source_missing_gate')
                : (importedSourceError ?? t('diagnostic.imported_prepare_gate'))}
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

  if (loading || runtimeCapabilitiesLoading) {
    return <LoadingState message={t('common.loading')} />;
  }

  if (error || !diagnostic) {
    return (
      <ErrorState
        title={t('common.error')}
        description={error ?? t('diagnostic.unavailable')}
        onRetry={loadDiagnostic}
      />
    );
  }

  // Find the primary recommended action for the hero button
  const primaryRec = diagnostic.recommendations.find((r) => r.isRecommended);
  const otherRecs = diagnostic.recommendations.filter((r) => !r.isRecommended);
  const heroRec = primaryRec ?? diagnostic.recommendations[0] ?? null;
  const heroRecActionable = heroRec ? isRecommendationActionable(heroRec.type) : false;
  const apfsRelevant =
    selectedDevice.filesystem === 'apfs' ||
    diagnostic.recommendations.some(
      (rec) =>
        rec.type === 'scan-deleted-apfs' ||
        (rec.type === 'scan-lost-volume' && rec.targetPotentialVolumeFilesystem === 'apfs'),
    ) ||
    diagnostic.limitations.some(
      (lim) =>
        lim === 'diagnostic.limitation.apfs_mvp' ||
        lim === 'diagnostic.limitation.apfs_support_limited' ||
        lim === 'diagnostic.limitation.apfs_snapshot_gap' ||
        lim === 'diagnostic.limitation.apfs_pre_unlock_gap',
    );
  const apfsCurrentCatalogAvailable =
    selectedDevice.filesystem === 'apfs' &&
    selectedDevice.partitions.some((partition) => partition.isMounted);
  const apfsDeletedRecoveryAvailable = diagnostic.recommendations.some(
    (rec) => rec.type === 'scan-deleted-apfs',
  );
  const apfsSnapshotRecoveryUnavailable = diagnostic.limitations.some(
    (lim) => lim === 'diagnostic.limitation.apfs_snapshot_gap',
  );
  const apfsSnapshotsDetected = diagnostic.limitations.some(
    (lim) => lim === 'diagnostic.limitation.apfs_snapshots_present_unavailable',
  );
  const apfsPreUnlockBlocked =
    encryptionInfo?.workflowState === 'pre_unlock_blocked' &&
    (selectedDevice.filesystem === 'apfs' ||
      diagnostic.limitations.some((lim) => lim === 'diagnostic.limitation.apfs_pre_unlock_gap'));

  const handleRecAction = (rec: (typeof diagnostic.recommendations)[number]) => {
    const deletedRecoveryRecommendation =
      rec.type === 'scan-lost-volume' ||
      rec.type === 'scan-deleted-ntfs' ||
      rec.type === 'scan-deleted-fat32' ||
      rec.type === 'scan-deleted-exfat' ||
      rec.type === 'scan-deleted-ext4' ||
      rec.type === 'scan-deleted-hfsplus' ||
      rec.type === 'scan-deleted-apfs';
    const signatureCarvingRecommendation = rec.type === 'scan-signature-carving';
    const actionable = isRecommendationActionable(rec.type);
    const available =
      isActionAvailable(
        rec.type,
        runtimeCapabilities?.scanEngine ?? false,
        runtimeCapabilities?.imagingEngine ?? false,
      ) &&
      (!recommendationNeedsImaging(rec.type) || diagnostic.imagingReady);

    if (!actionable) return;
    if (!available) {
      setNotice(
        rec.type === 'image-first' && diagnostic.imagingBlockReason
          ? diagnostic.imagingBlockReason
          : t('diagnostic.action_unavailable'),
      );
      return;
    }
    if (
      encryptionInfo?.detected &&
      encryptionInfo.workflowState === 'pre_unlock_blocked' &&
      rec.type !== 'image-first'
    ) {
      setNotice(encryptionInfo.saferNextStep);
      return;
    }
    if (rec.type === 'review-potential-volumes') {
      navigate('/expert');
      return;
    }
    setActiveScanId(null);
    setScanProgress(null);
    clearScanLogs();
    setRecoveryResult(null);
    if (rec.type === 'scan-lost-volume') {
      if (
        !rec.targetPotentialVolumeId ||
        !rec.targetPotentialVolumeLabel ||
        !rec.targetPotentialVolumeFilesystem ||
        rec.targetPotentialVolumeStartOffset === undefined
      ) {
        setNotice(t('diagnostic.lost_volume_missing_target'));
        return;
      }
      setScanConfig(
        buildPotentialVolumeScanConfig(
          selectedDeviceId,
          {
            id: rec.targetPotentialVolumeId,
            label: rec.targetPotentialVolumeLabel,
            filesystem: rec.targetPotentialVolumeFilesystem,
            startOffset: rec.targetPotentialVolumeStartOffset,
            sizeBytes: rec.targetPotentialVolumeSizeBytes,
          },
          {
            imagingRequiresElevation: diagnostic.imagingRequiresElevation,
            imagingProfile: diagnostic.imagingProfile,
            imagingProfileReasonKey: diagnostic.imagingProfileReasonKey,
          },
        ),
      );
    } else {
      setScanConfig({
        deviceId: selectedDeviceId,
        scanType: (rec.type === 'image-first'
          ? 'image'
          : signatureCarvingRecommendation
            ? 'signature-carving'
            : rec.type === 'scan-deep'
              ? 'deep'
              : deletedRecoveryRecommendation
                ? 'carving'
                : 'quick') as ScanType,
        targetFilesystems: [selectedDevice.filesystem],
        enableCarving: deletedRecoveryRecommendation || signatureCarvingRecommendation,
        imagingRequiresElevation: diagnostic.imagingRequiresElevation,
        imagingProfile: diagnostic.imagingProfile,
        imagingProfileReasonKey: diagnostic.imagingProfileReasonKey,
      });
    }
    navigate('/scan');
  };

  return (
    <div>
      <PageHeader title={t('diagnostic.title')} subtitle={t('diagnostic.subtitle')} />

      {__ALLOW_BROWSER_PREVIEW__ && !isTauri() && (
        <div className="mb-6">
          <WarningBanner variant="info">{t('diagnostic.preview_notice')}</WarningBanner>
        </div>
      )}

      {raidImportedSource && (
        <div className="mb-6">
          <WarningBanner variant="info">{t('diagnostic.raid_source_notice')}</WarningBanner>
        </div>
      )}

      {isSsd && (
        <div className="mb-6">
          <WarningBanner variant="warning">{t('safety.banner_ssd')}</WarningBanner>
        </div>
      )}

      {diagnostic.imagingProfile === 'cautious' && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            {t('diagnostic.imaging_profile_cautious_notice', {
              reason: t(diagnostic.imagingProfileReasonKey),
            })}
          </WarningBanner>
        </div>
      )}

      {encryptionInfo?.detected && encryptionInfo.workflowState === 'pre_unlock_blocked' && (
        <div className="mb-6">
          <WarningBanner variant="warning">
            <strong>{t('safety.encrypted_locked_title')}</strong>{' '}
            {t('safety.encrypted_locked_body', {
              encryption_type: encryptionInfo.encryptionType.toUpperCase(),
            })}{' '}
            {t('diagnostic.encryption_locked_notice', {
              step: encryptionInfo.saferNextStep,
            })}
          </WarningBanner>
        </div>
      )}

      {apfsRelevant && (
        <div className="mb-6" style={{ maxWidth: 600, margin: '0 auto' }}>
          <SectionCard
            title={t('diagnostic.apfs_operator_title')}
            subtitle={t('diagnostic.apfs_operator_subtitle')}
          >
            <div className="grid grid-2 gap-4">
              <div className="stat-card">
                <span className="stat-card-label">
                  {t('diagnostic.apfs_operator_current_catalog')}
                </span>
                <span
                  className="stat-card-value"
                  style={{
                    color: apfsCurrentCatalogAvailable
                      ? 'var(--color-success)'
                      : 'var(--color-text-primary)',
                  }}
                >
                  {apfsCurrentCatalogAvailable ? t('common.yes') : t('common.no')}
                </span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">
                  {t('diagnostic.apfs_operator_deleted_orphans')}
                </span>
                <span
                  className="stat-card-value"
                  style={{
                    color: apfsDeletedRecoveryAvailable
                      ? 'var(--color-warning)'
                      : 'var(--color-text-primary)',
                  }}
                >
                  {apfsDeletedRecoveryAvailable ? t('common.yes') : t('common.no')}
                </span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">{t('diagnostic.apfs_operator_snapshots')}</span>
                <span
                  className="stat-card-value"
                  style={{
                    color: apfsSnapshotRecoveryUnavailable
                      ? 'var(--color-danger)'
                      : 'var(--color-text-primary)',
                  }}
                >
                  {apfsSnapshotsDetected
                    ? t('diagnostic.apfs_operator_detected_not_delivered')
                    : apfsSnapshotRecoveryUnavailable
                      ? t('diagnostic.apfs_operator_not_delivered')
                      : t('diagnostic.apfs_operator_unknown')}
                </span>
              </div>
              <div className="stat-card">
                <span className="stat-card-label">{t('diagnostic.apfs_operator_preunlock')}</span>
                <span
                  className="stat-card-value"
                  style={{
                    color: apfsPreUnlockBlocked ? 'var(--color-danger)' : 'var(--color-success)',
                  }}
                >
                  {apfsPreUnlockBlocked
                    ? t('diagnostic.apfs_operator_blocked')
                    : t('diagnostic.apfs_operator_clear')}
                </span>
              </div>
            </div>
            <div className="text-sm text-secondary mt-4">
              {t('diagnostic.apfs_operator_notice')}
            </div>
          </SectionCard>
        </div>
      )}

      {notice && (
        <div className="mb-6">
          <WarningBanner variant="warning">{notice}</WarningBanner>
        </div>
      )}

      {/* 1. Score + Verdict — hero section */}
      <div
        className="flex flex-col items-center gap-4"
        style={{ padding: 'var(--space-6) 0', maxWidth: 480, margin: '0 auto' }}
      >
        <RecoveryScore
          score={diagnostic.recoverabilityScore}
          label={t('diagnostic.recoverability_note')}
          size={140}
        />

        <div
          style={{
            width: '100%',
            padding: 'var(--space-4)',
            borderRadius: 'var(--radius-lg)',
            background:
              diagnostic.verdict === 'simple'
                ? 'var(--color-success)'
                : diagnostic.verdict === 'risky'
                  ? 'var(--color-warning)'
                  : diagnostic.verdict === 'critical'
                    ? 'var(--color-danger)'
                    : 'var(--color-critical)',
            color: 'white',
            textAlign: 'center',
          }}
        >
          <div className="font-semibold text-md">
            {diagnostic.verdict === 'simple'
              ? t('diagnostic.verdict_simple')
              : diagnostic.verdict === 'risky'
                ? t('diagnostic.verdict_risky')
                : diagnostic.verdict === 'critical'
                  ? t('diagnostic.verdict_critical')
                  : t('diagnostic.verdict_lab')}
          </div>
        </div>

        {/* 2. One-line summary — loss type */}
        <span className="text-secondary text-md">
          {t(`diagnostic.loss_types.${diagnostic.lossType}`)}
        </span>

        {/* 3. Primary action button — always follows the backend recommendation */}
        <button
          type="button"
          className="btn btn-primary"
          style={{ width: '100%', padding: 'var(--space-3) var(--space-4)', fontSize: '1rem' }}
          onClick={() => {
            if (heroRec) {
              handleRecAction(heroRec);
            }
          }}
          disabled={!heroRecActionable}
        >
          {heroRec ? t(heroRec.titleKey) : t('diagnostic.start_scan')}
        </button>
      </div>

      {/* 4. Collapsible technical details */}
      <div style={{ maxWidth: 600, margin: '0 auto' }}>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          style={{
            width: '100%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 'var(--space-2)',
          }}
          onClick={() => setDetailsOpen((prev) => !prev)}
        >
          {detailsOpen ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
          {t('diagnostic.limitations')}
        </button>

        {detailsOpen && (
          <div className="flex flex-col gap-4 mt-4">
            {/* Risk factors */}
            {diagnostic.riskFactors.length > 0 && (
              <SectionCard title={t('diagnostic.risk_factors')}>
                <div className="flex flex-col gap-3">
                  {diagnostic.riskFactors.map((rf) => (
                    <div
                      key={rf.id}
                      className="flex items-start gap-3"
                      style={{ padding: 'var(--space-2) 0' }}
                    >
                      <RiskBadge level={rf.severity as RiskLevel} />
                      <div>
                        <div className="font-medium">{t(rf.titleKey)}</div>
                        {userMode === 'expert' && (
                          <div className="text-sm text-secondary mt-1">{t(rf.descriptionKey)}</div>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </SectionCard>
            )}

            {/* Limitations */}
            {diagnostic.limitations.length > 0 && (
              <SectionCard title={t('diagnostic.limitations')}>
                <div className="flex flex-col gap-2">
                  {diagnostic.limitations.map((lim, i) => (
                    <div key={i} className="flex items-start gap-2">
                      <AlertTriangle
                        size={14}
                        className="text-warning"
                        style={{ marginTop: 3, flexShrink: 0 }}
                      />
                      <span className="text-sm text-secondary">{resolveKey(t, lim)}</span>
                    </div>
                  ))}
                </div>
              </SectionCard>
            )}

            {/* Probable causes */}
            {diagnostic.probableCauses.length > 0 && (
              <SectionCard title={t('diagnostic.probable_causes')}>
                <div className="flex flex-col gap-3">
                  {diagnostic.probableCauses.map((cause, i) => (
                    <div key={i} className="flex items-start gap-3">
                      <div
                        style={{
                          width: 6,
                          height: 6,
                          borderRadius: '50%',
                          background: 'var(--color-accent)',
                          marginTop: 6,
                          flexShrink: 0,
                        }}
                      />
                      <span className="text-md">{resolveKey(t, cause)}</span>
                    </div>
                  ))}
                </div>
              </SectionCard>
            )}
          </div>
        )}
      </div>

      {/* 5. AI Advisory — expert mode only */}
      {userMode === 'expert' && runtimeCapabilities?.aiAdvisory && (
        <div style={{ maxWidth: 600, margin: 'var(--space-6) auto 0' }}>
          <SectionCard title={t('diagnostic.ai_title')}>
            <AiAdvisoryPanel
              advisory={aiAdvisory}
              loading={aiLoading}
              error={aiError}
              userMode={userMode}
            />
          </SectionCard>
        </div>
      )}

      {/* 6. Recommendations — simplified */}
      <div style={{ maxWidth: 600, margin: 'var(--space-6) auto 0' }}>
        <SectionCard title={t('diagnostic.recommendations')}>
          <div className="flex flex-col gap-3">
            {/* Highlighted recommended action */}
            {primaryRec &&
              (() => {
                const Icon = recIcons[primaryRec.type] || Search;
                return (
                  <button
                    type="button"
                    className="section-card"
                    aria-label={t('diagnostic.recommendations')}
                    style={{
                      cursor: 'pointer',
                      borderColor: 'var(--color-accent)',
                      padding: 'var(--space-3)',
                    }}
                    onClick={() => handleRecAction(primaryRec)}
                  >
                    <div className="flex items-center gap-3">
                      <div
                        style={{
                          width: 36,
                          height: 36,
                          borderRadius: 'var(--radius-lg)',
                          background: 'linear-gradient(135deg, #4A7CFF, #6B95FF)',
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          color: 'white',
                          flexShrink: 0,
                        }}
                      >
                        <Icon size={18} />
                      </div>
                      <div style={{ flex: 1 }}>
                        <div className="font-semibold">{t(primaryRec.titleKey)}</div>
                        <div className="text-sm text-secondary">{t(primaryRec.descriptionKey)}</div>
                      </div>
                      <span
                        className="badge badge-risk-low"
                        style={{
                          background: 'var(--color-accent-subtle)',
                          color: 'var(--color-accent)',
                          border: '1px solid rgba(74, 124, 255, 0.2)',
                          flexShrink: 0,
                        }}
                      >
                        {t('diagnostic.recommended')}
                      </span>
                    </div>
                  </button>
                );
              })()}

            {/* Other recommendations as simple list */}
            {otherRecs.map((rec) => {
              const Icon = recIcons[rec.type] || Search;
              const available =
                isActionAvailable(
                  rec.type,
                  runtimeCapabilities?.scanEngine ?? false,
                  runtimeCapabilities?.imagingEngine ?? false,
                ) &&
                (!recommendationNeedsImaging(rec.type) || diagnostic.imagingReady);
              if (!available) {
                return (
                  <div
                    key={rec.id}
                    className="flex items-center gap-3"
                    style={{
                      padding: 'var(--space-2) var(--space-3)',
                      cursor: 'default',
                      opacity: 0.6,
                      borderRadius: 'var(--radius-md)',
                    }}
                  >
                    <Icon size={16} className="text-secondary" style={{ flexShrink: 0 }} />
                    <span className="text-md">{t(rec.titleKey)}</span>
                  </div>
                );
              }

              return (
                <button
                  type="button"
                  key={rec.id}
                  className="flex items-center gap-3"
                  aria-label={t('diagnostic.recommendations')}
                  style={{
                    padding: 'var(--space-2) var(--space-3)',
                    cursor: 'pointer',
                    opacity: 1,
                    borderRadius: 'var(--radius-md)',
                  }}
                  onClick={() => handleRecAction(rec)}
                >
                  <Icon size={16} className="text-secondary" style={{ flexShrink: 0 }} />
                  <span className="text-md">{t(rec.titleKey)}</span>
                </button>
              );
            })}
          </div>
        </SectionCard>
      </div>
    </div>
  );
}
