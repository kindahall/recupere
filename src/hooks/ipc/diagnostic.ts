import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  AiAdvisory,
  AiRecoveryBrief,
  DetectedDevice,
  FilesystemType,
  ImportedRecoverySourceStatus,
  PotentialVolume,
} from '../../types';
import {
  getBrowserPreviewDevice,
  getBrowserPreviewImportedSourceStatus,
  getBrowserPreviewRecoveryResult,
  loadBrowserPreviewState,
} from '../../utils/browserPreviewSeed';
import {
  recommendedImagingProfileForDevice,
  recommendedImagingProfileReasonKeyForDevice,
} from '../../utils/imagingProfile';

export interface DiagnosticData {
  deviceId: string;
  recoverabilityScore: number;
  lossType: string;
  probableCauses: string[];
  riskFactors: { id: string; severity: string; titleKey: string; descriptionKey: string }[];
  recommendations: {
    id: string;
    type: string;
    priority: number;
    titleKey: string;
    descriptionKey: string;
    isRecommended: boolean;
    targetPotentialVolumeId?: string;
    targetPotentialVolumeLabel?: string;
    targetPotentialVolumeFilesystem?: FilesystemType;
    targetPotentialVolumeStartOffset?: number;
    targetPotentialVolumeSizeBytes?: number;
  }[];
  limitations: string[];
  imagingReady: boolean;
  imagingRequiresElevation: boolean;
  imagingProfile: import('../../types').ImagingProfile;
  imagingProfileReasonKey: string;
  imagingSourcePath?: string;
  imagingBlockReason?: string;
  potentialVolumesInspected: boolean;
  potentialVolumesNotice?: string;
  potentialVolumes: PotentialVolume[];
  verdict: string;
  verdictDetails: string;
}

interface RustDiagnostic {
  device_id: string;
  recoverability_score: number;
  loss_type: string;
  probable_causes: string[];
  risk_factors: { id: string; severity: string; title_key: string; description_key: string }[];
  recommendations: {
    id: string;
    rec_type: string;
    priority: number;
    title_key: string;
    description_key: string;
    is_recommended: boolean;
    target_potential_volume_id: string | null;
    target_potential_volume_label: string | null;
    target_potential_volume_filesystem: string | null;
    target_potential_volume_start_offset: number | null;
    target_potential_volume_size_bytes: number | null;
  }[];
  limitations: string[];
  imaging_ready: boolean;
  imaging_requires_elevation: boolean;
  imaging_profile: 'standard' | 'cautious';
  imaging_profile_reason_key: string;
  imaging_source_path: string | null;
  imaging_block_reason: string | null;
  potential_volumes_inspected: boolean;
  potential_volumes_notice: string | null;
  potential_volumes: {
    id: string;
    label: string;
    filesystem: string;
    start_offset: number;
    size_bytes: number | null;
    confidence_score: number;
    detection_method: string;
    notes: string[];
  }[];
  verdict: string;
  verdict_details: string;
}

interface RustAiAdvisory {
  device_id: string;
  mode: string;
  confidence_score: number;
  summary: string;
  rationale: string[];
  cautions: string[];
  next_steps: string[];
  expert_notes: string[];
  recommended_action_type: string | null;
  recommended_action_title: string | null;
  cloud_available: boolean;
}

export interface RustAiRecoveryBrief {
  scan_id: string;
  mode: string;
  confidence_score: number;
  summary: string;
  strategy_title: string;
  strategy_reasoning: string[];
  evidence: string[];
  cautions: string[];
  next_steps: string[];
  expert_notes: string[];
  priority_order: string[];
  stability_reason: string;
  blocked_by: string[];
  safe_export_strategy: string;
  complexity_summary: string;
  counts: {
    export_now: number;
    verify_with_preview: number;
    complex_recovery_review: number;
    review_first: number;
    unstable: number;
    deleted: number;
    carved: number;
    fragmented: number;
    previewable: number;
    compressed: number;
    snapshot_derived: number;
    journal_derived: number;
    apfs_catalog_preview_first: number;
    apfs_catalog_reassembled: number;
  };
}

function buildBrowserPreviewRecoveryBrief(scanId: string): AiRecoveryBrief {
  const files = getBrowserPreviewRecoveryResult(scanId)?.files ?? [];
  const confidenceScore =
    files.length > 0
      ? Math.round(files.reduce((sum, file) => sum + file.recoveryScore, 0) / files.length)
      : 0;
  const deleted = files.filter((file) => file.isDeleted).length;
  const carved = files.filter((file) => file.recoveryMethod === 'carving').length;
  const fragmented = files.filter((file) => file.integrity === 'fragmented').length;
  const compressed = files.filter((file) => Boolean(file.compressionKind)).length;
  const journalDerived = files.filter(
    (file) => file.sourceView === 'journal' || file.journalDerived,
  ).length;
  const snapshotDerived = files.filter((file) => file.sourceView === 'snapshot').length;
  const exportNow = files.filter(
    (file) => file.integrity === 'intact' && file.recoveryScore >= 80,
  ).length;
  const verifyWithPreview = files.filter((file) => file.previewAvailable).length;
  const complexRecoveryReview = files.filter((file) => file.recoveryComplexity === 'high').length;
  const unstable = files.filter((file) => file.integrity === 'corrupt').length;
  const apfsCatalogPreviewFirst = files.filter(
    (file) =>
      file.isDeleted &&
      file.sourceView === 'live-catalog' &&
      (file.validatorStatus === 'unsupported' || file.validatorStatus === 'partial-unvalidated'),
  ).length;
  const apfsCatalogReassembled = files.filter(
    (file) =>
      file.isDeleted &&
      file.sourceView === 'live-catalog' &&
      file.validatorStatus === 'reassembled',
  ).length;

  return {
    scanId,
    mode: 'local-results',
    confidenceScore,
    summary:
      files.length > 0
        ? `${files.length} preview result(s) loaded for browser validation.`
        : 'No preview results are available for browser validation.',
    strategyTitle:
      exportNow > 0 ? 'Export intact files first' : 'Review unstable files before export',
    strategyReasoning: [
      'This browser-preview brief is generated from seeded fixture data.',
      exportNow > 0
        ? 'Intact, higher-score files should be prioritized for first-pass export.'
        : 'The current fixture emphasizes review and verification before export.',
    ],
    evidence: [
      `${exportNow} file(s) qualify for immediate export.`,
      `${complexRecoveryReview} file(s) need complex recovery review.`,
    ],
    cautions:
      unstable > 0
        ? ['Some files remain unstable or corrupt in the preview fixture.']
        : ['Browser preview uses simulated AI output, not the desktop engine.'],
    nextSteps: [
      'Preview the most valuable files.',
      'Validate a safe export destination.',
      'Export intact files before revisiting difficult cases.',
    ],
    expertNotes: [
      'Fixture-backed browser preview only.',
      'Confidence remains an estimate, not a guarantee.',
    ],
    priorityOrder: files
      .slice()
      .sort((a, b) => b.recoveryScore - a.recoveryScore)
      .slice(0, 5)
      .map((file) => file.id),
    stabilityReason:
      unstable > 0
        ? 'Some files are marked corrupt in the preview fixture.'
        : 'The preview fixture is stable enough for export validation.',
    blockedBy: [],
    safeExportStrategy: 'Export intact files to a separate destination first.',
    complexitySummary:
      complexRecoveryReview > 0
        ? `${complexRecoveryReview} file(s) remain in a high-complexity state.`
        : 'No high-complexity files are flagged in the preview fixture.',
    counts: {
      exportNow,
      verifyWithPreview,
      complexRecoveryReview,
      reviewFirst: Math.max(files.length - exportNow, 0),
      unstable,
      deleted,
      carved,
      fragmented,
      previewable: verifyWithPreview,
      compressed,
      snapshotDerived,
      journalDerived,
      apfsCatalogPreviewFirst,
      apfsCatalogReassembled,
    },
  };
}

function supportsDeletedRecovery(filesystem: FilesystemType): boolean {
  return (
    filesystem === 'ntfs' ||
    filesystem === 'fat32' ||
    filesystem === 'exfat' ||
    filesystem === 'ext4' ||
    filesystem === 'hfs+' ||
    filesystem === 'apfs'
  );
}

function deletedRecoveryRecommendationType(
  filesystem: FilesystemType,
): DiagnosticData['recommendations'][number]['type'] | null {
  switch (filesystem) {
    case 'ntfs':
      return 'scan-deleted-ntfs';
    case 'fat32':
      return 'scan-deleted-fat32';
    case 'exfat':
      return 'scan-deleted-exfat';
    case 'ext4':
      return 'scan-deleted-ext4';
    case 'hfs+':
      return 'scan-deleted-hfsplus';
    case 'apfs':
      return 'scan-deleted-apfs';
    default:
      return null;
  }
}

function hasMountedVolume(device: DetectedDevice): boolean {
  return device.partitions.some((partition) => partition.isMounted || Boolean(partition.mountPath));
}

function buildBrowserPreviewDiagnostic(deviceId: string): DiagnosticData {
  const previewState = loadBrowserPreviewState();
  const device = getBrowserPreviewDevice(deviceId);

  if (!device) {
    throw new Error('Diagnostic data is unavailable for the selected device.');
  }

  const importedSourceStatus: ImportedRecoverySourceStatus | null =
    device.type === 'image' ? getBrowserPreviewImportedSourceStatus(deviceId) : null;

  if (device.type === 'image' && importedSourceStatus?.sourceAvailable === false) {
    throw new Error(
      'The imported source file is missing, so preview diagnostic data is unavailable.',
    );
  }

  if (
    device.type === 'image' &&
    importedSourceStatus?.requiresPreparation &&
    !importedSourceStatus.prepared
  ) {
    throw new Error('Prepare this imported source before preview diagnostic can continue.');
  }

  const scanEngine = previewState?.runtimeCapabilities?.scanEngine ?? false;
  const imagingEngine = previewState?.runtimeCapabilities?.imagingEngine ?? false;
  const mountedVolumeAvailable = hasMountedVolume(device);
  const deletedRecoveryType = deletedRecoveryRecommendationType(device.filesystem);
  const deletedRecoveryAvailable = Boolean(deletedRecoveryType && scanEngine);
  const signatureCarvingAvailable = Boolean(scanEngine && imagingEngine);
  const imagingProfile = recommendedImagingProfileForDevice(device);
  const imagingProfileReasonKey = recommendedImagingProfileReasonKeyForDevice(device);
  const imagePreparationReady =
    device.type === 'image'
      ? importedSourceStatus
        ? importedSourceStatus.sourceAvailable &&
          (!importedSourceStatus.requiresPreparation || importedSourceStatus.prepared)
        : true
      : imagingEngine;

  const riskFactors: DiagnosticData['riskFactors'] = [
    {
      id: 'preview-metadata-first',
      severity: 'low',
      titleKey: 'risk.metadata_only.title',
      descriptionKey: deletedRecoveryAvailable
        ? 'risk.metadata_only.description_with_deleted'
        : signatureCarvingAvailable
          ? 'risk.metadata_only.description_with_carving'
          : 'risk.metadata_only.description_basic',
    },
  ];

  if (device.isTrimEnabled) {
    riskFactors.push({
      id: 'preview-trim',
      severity: 'high',
      titleKey: 'risk.trim.title',
      descriptionKey: 'risk.trim.description',
    });
  }

  if (device.isEncrypted) {
    riskFactors.push({
      id: 'preview-encryption',
      severity: 'high',
      titleKey: 'risk.encryption.title',
      descriptionKey: 'risk.encryption.description',
    });
  }

  if (device.status === 'failing' || device.status === 'unresponsive') {
    riskFactors.push({
      id: 'preview-health',
      severity: 'critical',
      titleKey: 'risk.health.title',
      descriptionKey: 'risk.health.description',
    });
  } else if (device.status === 'degraded') {
    riskFactors.push({
      id: 'preview-degraded',
      severity: 'medium',
      titleKey: 'risk.degraded.title',
      descriptionKey: 'risk.degraded.description',
    });
  }

  if (device.filesystem === 'unknown') {
    riskFactors.push({
      id: 'preview-unknown-fs',
      severity: 'medium',
      titleKey: 'risk.unknown_fs.title',
      descriptionKey: 'risk.unknown_fs.description',
    });
  }

  const probableCauses: string[] = [
    'diagnostic.observed.metadata_only',
    'diagnostic.observed.device_type_fs',
    mountedVolumeAvailable && device.type !== 'image'
      ? 'diagnostic.observed.mount_available'
      : 'diagnostic.observed.no_mount',
  ];

  if (device.status !== 'healthy') {
    probableCauses.push('diagnostic.observed.status_caution');
  }

  const limitations: string[] = [];
  if (!mountedVolumeAvailable && device.type !== 'image') {
    limitations.push('diagnostic.limitation.no_mount');
  }
  if (device.isTrimEnabled) {
    limitations.push('diagnostic.limitation.trim_erased');
  }
  if (device.isEncrypted) {
    limitations.push('diagnostic.limitation.encryption_required');
  }
  if (device.riskLevel === 'high' || device.riskLevel === 'critical') {
    limitations.push('diagnostic.limitation.high_risk_image_first');
  }
  if (supportsDeletedRecovery(device.filesystem)) {
    switch (device.filesystem) {
      case 'ntfs':
        limitations.push('diagnostic.limitation.deleted_recovery_limited');
        break;
      case 'fat32':
        limitations.push('diagnostic.limitation.deleted_recovery_limited');
        break;
      case 'exfat':
        limitations.push('diagnostic.limitation.deleted_recovery_limited');
        break;
      case 'ext4':
        limitations.push('diagnostic.limitation.ext4_mvp');
        break;
      case 'hfs+':
        limitations.push('diagnostic.limitation.hfsplus_mvp');
        break;
      case 'apfs':
        limitations.push('diagnostic.limitation.apfs_mvp');
        break;
      default:
        break;
    }
    limitations.push('diagnostic.limitation.scores_estimate_deleted');
  } else if (signatureCarvingAvailable) {
    limitations.push('diagnostic.limitation.carving_limited');
  } else {
    limitations.push('diagnostic.limitation.catalog_only');
    limitations.push('diagnostic.limitation.scores_estimate_metadata');
  }

  const recommendationCandidates: Array<{
    type: DiagnosticData['recommendations'][number]['type'];
    titleKey: string;
    descriptionKey: string;
  }> = [];

  const highRiskPhysicalSource =
    device.type !== 'image' &&
    (device.riskLevel === 'high' ||
      device.riskLevel === 'critical' ||
      device.status === 'failing' ||
      device.status === 'unresponsive');

  if (highRiskPhysicalSource && imagingEngine) {
    recommendationCandidates.push({
      type: 'image-first',
      titleKey: 'recommendation.image_first.title',
      descriptionKey: 'recommendation.image_first.description_default',
    });
  }

  if (device.type === 'image') {
    if (deletedRecoveryAvailable && deletedRecoveryType && imagePreparationReady) {
      recommendationCandidates.push({
        type: deletedRecoveryType,
        titleKey: 'recommendation.scan_deleted.title',
        descriptionKey: 'recommendation.scan_deleted.description',
      });
    }
    if (signatureCarvingAvailable && imagePreparationReady) {
      recommendationCandidates.push({
        type: 'scan-signature-carving',
        titleKey: 'recommendation.scan_signature_carving.title',
        descriptionKey: 'recommendation.scan_signature_carving.description',
      });
    }
  } else {
    if (mountedVolumeAvailable && scanEngine) {
      recommendationCandidates.push({
        type: 'scan-quick',
        titleKey: 'recommendation.scan_quick.title',
        descriptionKey: deletedRecoveryAvailable
          ? 'recommendation.scan_now.description_with_deleted'
          : 'recommendation.scan_now.description_default',
      });
      recommendationCandidates.push({
        type: 'scan-deep',
        titleKey: 'recommendation.scan_deep.title',
        descriptionKey: deletedRecoveryAvailable
          ? 'recommendation.scan_now.description_with_deleted'
          : 'recommendation.scan_now.description_default',
      });
    } else if (imagingEngine) {
      recommendationCandidates.push({
        type: 'image-first',
        titleKey: 'recommendation.image_first.title',
        descriptionKey: 'recommendation.image_first.description_default',
      });
    } else {
      recommendationCandidates.push({
        type: 'professional-help',
        titleKey: 'recommendation.wait_mount.title',
        descriptionKey: 'recommendation.wait_mount.description',
      });
    }

    if (deletedRecoveryAvailable && deletedRecoveryType && imagePreparationReady) {
      recommendationCandidates.push({
        type: deletedRecoveryType,
        titleKey: 'recommendation.scan_deleted.title',
        descriptionKey: 'recommendation.scan_deleted.description',
      });
    }
    if (signatureCarvingAvailable && imagePreparationReady) {
      recommendationCandidates.push({
        type: 'scan-signature-carving',
        titleKey: 'recommendation.scan_signature_carving.title',
        descriptionKey: 'recommendation.scan_signature_carving.description',
      });
    }
  }

  if (
    recommendationCandidates.length === 0 ||
    device.isEncrypted ||
    device.riskLevel === 'critical'
  ) {
    recommendationCandidates.push({
      type: 'professional-help',
      titleKey: 'recommendation.professional_help.title',
      descriptionKey: 'recommendation.professional_help.description',
    });
  }

  const recommendations = recommendationCandidates.map((recommendation, index) => ({
    id: `preview-rec-${index + 1}`,
    type: recommendation.type,
    priority: index + 1,
    titleKey: recommendation.titleKey,
    descriptionKey: recommendation.descriptionKey,
    isRecommended: index === 0,
  }));

  let recoverabilityScore = 84;
  if (device.filesystem === 'unknown') recoverabilityScore -= 12;
  if (device.status === 'degraded') recoverabilityScore -= 10;
  if (device.status === 'failing') recoverabilityScore -= 24;
  if (device.status === 'unresponsive') recoverabilityScore -= 42;
  if (device.riskLevel === 'high') recoverabilityScore -= 12;
  if (device.riskLevel === 'critical') recoverabilityScore -= 26;
  if (device.isTrimEnabled) recoverabilityScore -= 10;
  if (device.isEncrypted) recoverabilityScore -= 8;
  recoverabilityScore = Math.max(8, Math.min(96, recoverabilityScore));

  const verdict =
    device.status === 'unresponsive' || device.riskLevel === 'critical'
      ? 'critical'
      : device.status === 'failing' ||
          device.riskLevel === 'high' ||
          device.filesystem === 'unknown'
        ? 'risky'
        : 'simple';

  return {
    deviceId,
    recoverabilityScore,
    lossType: supportsDeletedRecovery(device.filesystem)
      ? 'accidental-deletion'
      : device.filesystem === 'unknown'
        ? 'unknown'
        : 'corruption',
    probableCauses,
    riskFactors,
    recommendations,
    limitations,
    imagingReady: imagePreparationReady,
    imagingRequiresElevation: false,
    imagingProfile,
    imagingProfileReasonKey,
    imagingSourcePath:
      importedSourceStatus?.analysisPath ?? importedSourceStatus?.sourcePath ?? device.devicePath,
    imagingBlockReason: imagePreparationReady
      ? undefined
      : 'This preview source still needs an explicit local read-only preparation step.',
    potentialVolumesInspected: false,
    potentialVolumesNotice: undefined,
    potentialVolumes: [],
    verdict,
    verdictDetails:
      'Browser preview generated this assessment from seeded device metadata and explicit imported-source readiness. Use the desktop backend for real hardware-backed diagnostics.',
  };
}

export async function fetchDiagnostic(deviceId: string): Promise<DiagnosticData> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return buildBrowserPreviewDiagnostic(deviceId);
  }
  const d = await invoke<RustDiagnostic>('get_diagnostic', { deviceId });
  return {
    deviceId: d.device_id,
    recoverabilityScore: d.recoverability_score,
    lossType: d.loss_type,
    probableCauses: d.probable_causes,
    riskFactors: d.risk_factors.map((rf) => ({
      id: rf.id,
      severity: rf.severity,
      titleKey: rf.title_key,
      descriptionKey: rf.description_key,
    })),
    recommendations: d.recommendations.map((r) => ({
      id: r.id,
      type: r.rec_type,
      priority: r.priority,
      titleKey: r.title_key,
      descriptionKey: r.description_key,
      isRecommended: r.is_recommended,
      targetPotentialVolumeId: r.target_potential_volume_id ?? undefined,
      targetPotentialVolumeLabel: r.target_potential_volume_label ?? undefined,
      targetPotentialVolumeFilesystem:
        (r.target_potential_volume_filesystem as FilesystemType | null) ?? undefined,
      targetPotentialVolumeStartOffset: r.target_potential_volume_start_offset ?? undefined,
      targetPotentialVolumeSizeBytes: r.target_potential_volume_size_bytes ?? undefined,
    })),
    limitations: d.limitations,
    imagingReady: d.imaging_ready,
    imagingRequiresElevation: d.imaging_requires_elevation,
    imagingProfile: d.imaging_profile,
    imagingProfileReasonKey: d.imaging_profile_reason_key,
    imagingSourcePath: d.imaging_source_path ?? undefined,
    imagingBlockReason: d.imaging_block_reason ?? undefined,
    potentialVolumesInspected: d.potential_volumes_inspected,
    potentialVolumesNotice: d.potential_volumes_notice ?? undefined,
    potentialVolumes: d.potential_volumes.map((volume) => ({
      id: volume.id,
      label: volume.label,
      filesystem: volume.filesystem as FilesystemType,
      startOffset: volume.start_offset,
      sizeBytes: volume.size_bytes ?? undefined,
      confidenceScore: volume.confidence_score,
      detectionMethod: volume.detection_method,
      notes: volume.notes,
    })),
    verdict: d.verdict,
    verdictDetails: d.verdict_details,
  };
}

export async function fetchAiAdvisory(deviceId: string): Promise<AiAdvisory> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return {
      deviceId,
      mode: 'local',
      confidenceScore: 52,
      summary: 'Browser preview advisory loaded from local fixture state.',
      rationale: [
        'Use the desktop backend for real device diagnostics.',
        'Preview mode is intended for UI validation and smoke coverage.',
      ],
      cautions: ['Diagnostic conclusions remain simulated outside Tauri.'],
      nextSteps: ['Switch to the desktop app for hardware-backed analysis.'],
      expertNotes: ['No native device inspection is available in browser preview.'],
      recommendedActionType: 'review-first',
      recommendedActionTitle: 'Review fixture-backed guidance',
      cloudAvailable: false,
    };
  }
  const advisory = await invoke<RustAiAdvisory>('get_ai_advisory', { deviceId });
  return {
    deviceId: advisory.device_id,
    mode: advisory.mode as AiAdvisory['mode'],
    confidenceScore: advisory.confidence_score,
    summary: advisory.summary,
    rationale: advisory.rationale,
    cautions: advisory.cautions,
    nextSteps: advisory.next_steps,
    expertNotes: advisory.expert_notes,
    recommendedActionType: advisory.recommended_action_type ?? undefined,
    recommendedActionTitle: advisory.recommended_action_title ?? undefined,
    cloudAvailable: advisory.cloud_available,
  };
}

export function mapAiRecoveryBrief(brief: RustAiRecoveryBrief): AiRecoveryBrief {
  return {
    scanId: brief.scan_id,
    mode: brief.mode as AiRecoveryBrief['mode'],
    confidenceScore: brief.confidence_score,
    summary: brief.summary,
    strategyTitle: brief.strategy_title,
    strategyReasoning: brief.strategy_reasoning,
    evidence: brief.evidence,
    cautions: brief.cautions,
    nextSteps: brief.next_steps,
    expertNotes: brief.expert_notes,
    priorityOrder: brief.priority_order,
    stabilityReason: brief.stability_reason,
    blockedBy: brief.blocked_by,
    safeExportStrategy: brief.safe_export_strategy,
    complexitySummary: brief.complexity_summary,
    counts: {
      exportNow: brief.counts.export_now,
      verifyWithPreview: brief.counts.verify_with_preview,
      complexRecoveryReview: brief.counts.complex_recovery_review,
      reviewFirst: brief.counts.review_first,
      unstable: brief.counts.unstable,
      deleted: brief.counts.deleted,
      carved: brief.counts.carved,
      fragmented: brief.counts.fragmented,
      previewable: brief.counts.previewable,
      compressed: brief.counts.compressed,
      snapshotDerived: brief.counts.snapshot_derived,
      journalDerived: brief.counts.journal_derived,
      apfsCatalogPreviewFirst: brief.counts.apfs_catalog_preview_first,
      apfsCatalogReassembled: brief.counts.apfs_catalog_reassembled,
    },
  };
}

export async function fetchScanAiBrief(scanId: string): Promise<AiRecoveryBrief> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return buildBrowserPreviewRecoveryBrief(scanId);
  }
  const brief = await invoke<RustAiRecoveryBrief>('get_scan_ai_brief', { scanId });
  return mapAiRecoveryBrief(brief);
}
