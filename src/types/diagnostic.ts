// ============================================================================
// Shared TypeScript Types — Diagnostic
// ============================================================================

export type LossType =
  | 'accidental-deletion'
  | 'accidental-format'
  | 'corruption'
  | 'partition-lost'
  | 'virus-attack'
  | 'system-crash'
  | 'hardware-failure'
  | 'unknown';

export interface DiagnosticResult {
  deviceId: string;
  lossType: LossType;
  lossTypeConfidence: number; // 0-100
  recoverabilityScore: number; // 0-100 (estimate)
  probableCauses: string[];
  riskFactors: RiskFactor[];
  recommendations: Recommendation[];
  limitations: string[];
  imagingReady: boolean;
  imagingRequiresElevation: boolean;
  imagingProfile: import('./scan').ImagingProfile;
  imagingProfileReasonKey: string;
  imagingSourcePath?: string;
  imagingBlockReason?: string;
  potentialVolumesInspected: boolean;
  potentialVolumesNotice?: string;
  potentialVolumes: import('./device').PotentialVolume[];
  verdict: string;
  verdictDetails: string;
  smartData?: SmartData;
}

export interface RiskFactor {
  id: string;
  severity: 'low' | 'medium' | 'high' | 'critical';
  titleKey: string; // i18n key
  descriptionKey: string; // i18n key
}

export interface Recommendation {
  id: string;
  type:
    | 'scan-quick'
    | 'scan-deep'
    | 'scan-lost-volume'
    | 'scan-deleted-ntfs'
    | 'scan-deleted-fat32'
    | 'scan-deleted-exfat'
    | 'scan-deleted-ext4'
    | 'scan-deleted-hfsplus'
    | 'scan-deleted-apfs'
    | 'scan-signature-carving'
    | 'review-potential-volumes'
    | 'image-first'
    | 'stop-usage'
    | 'professional-help';
  priority: number;
  titleKey: string; // i18n key
  descriptionKey: string; // i18n key
  isRecommended: boolean;
  targetPotentialVolumeId?: string;
  targetPotentialVolumeLabel?: string;
  targetPotentialVolumeFilesystem?: import('./device').FilesystemType;
  targetPotentialVolumeStartOffset?: number;
  targetPotentialVolumeSizeBytes?: number;
}

export interface SmartData {
  available: boolean;
  overallHealth: 'good' | 'caution' | 'bad';
  temperature?: number;
  powerOnHours?: number;
  reallocatedSectors?: number;
  pendingSectors?: number;
  rawAttributes?: DiagnosticSmartAttribute[];
}

export interface DiagnosticSmartAttribute {
  id: number;
  name: string;
  value: number;
  worst: number;
  threshold: number;
  raw: string;
}
