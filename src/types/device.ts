// ============================================================================
// Shared TypeScript Types — Device & Hardware
// ============================================================================

export type DeviceType = 'hdd' | 'ssd' | 'nvme' | 'usb' | 'sd' | 'external' | 'image';

export type FilesystemType = 'ntfs' | 'fat32' | 'exfat' | 'apfs' | 'hfs+' | 'ext4' | 'unknown';

export type RiskLevel = 'low' | 'medium' | 'high' | 'critical';

export type DeviceStatus = 'healthy' | 'degraded' | 'failing' | 'unresponsive' | 'unknown';

export interface DetectedDevice {
  id: string;
  name: string;
  devicePath: string; // e.g. /dev/disk2, \\.\PhysicalDrive1
  type: DeviceType;
  filesystem: FilesystemType;
  capacityBytes: number;
  usedBytes: number;
  status: DeviceStatus;
  riskLevel: RiskLevel;
  serial?: string;
  model?: string;
  isTrimEnabled?: boolean;
  isEncrypted?: boolean;
  smartAvailable?: boolean;
  partitions: Partition[];
}

export interface Partition {
  id: string;
  label: string;
  filesystem: FilesystemType;
  sizeBytes: number;
  startOffset: number;
  mountPath?: string;
  isMounted: boolean;
  isBootable: boolean;
}

export interface PotentialVolume {
  id: string;
  label: string;
  filesystem: FilesystemType;
  startOffset: number;
  sizeBytes?: number;
  confidenceScore: number;
  detectionMethod: string;
  notes: string[];
}

export interface SmartAttribute {
  id: number;
  name: string;
  value: number;
  worst: number;
  threshold: number;
  rawValue: string;
  status: string;
}

export interface SmartReport {
  deviceId: string;
  overallHealth: string;
  temperatureCelsius?: number;
  powerOnHours?: number;
  reallocatedSectors?: number;
  pendingSectors?: number;
  attributes: SmartAttribute[];
  errorLogCount?: number;
}

export type EncryptionType =
  | 'filevault2'
  | 'bitlocker'
  | 'luks1'
  | 'luks2'
  | 'veracrypt'
  | 'unknown';

export interface EncryptionInfo {
  detected: boolean;
  encryptionType: EncryptionType;
  canUnlock: boolean;
  workflowState: 'clear' | 'pre_unlock_blocked' | 'unsupported';
  saferNextStep: string;
  message: string;
}

export interface RaidMetadata {
  level: string;
  memberCount: number;
  stripeSizeBytes?: number;
  superblockVersion?: string;
  dataOffsetBytes?: number;
}

export interface RaidCandidateMember {
  deviceId: string;
  deviceName: string;
  devicePath: string;
}

export interface RaidCandidate {
  level: string;
  expectedMemberCount: number;
  detectedMemberCount: number;
  stripeSizeBytes: number;
  superblockVersion: string;
  members: RaidCandidateMember[];
}

export interface RaidAnalysisBuildResult {
  deviceId: string;
  path: string;
  displayName: string;
  level: string;
  memberCount: number;
  expectedMemberCount: number;
}

export interface RaidAnalysisBuildRequest {
  level: string;
  orderedMemberDeviceIds: Array<string | null>;
  stripeSizeBytes: number;
  dataOffsetBytes: number;
}

export interface RaidAnalysisValidation {
  supported: boolean;
  variant: 'supported' | 'degraded' | 'unsupported';
  confidence: 'high' | 'medium' | 'low';
  missingMemberCount: number;
  message: string;
  warnings: string[];
}

export interface ImportedRecoverySourceStatus {
  displayName: string;
  sourcePath: string;
  sourceFormat: string;
  logicalSizeBytes: number;
  supportTier?: 'supported' | 'limited' | 'unsupported';
  supportNote?: string;
  saferNextStep?: string;
  sourceAvailable: boolean;
  requiresPreparation: boolean;
  prepared: boolean;
  analysisPath?: string;
  cachePath?: string;
  cacheSizeBytes?: number;
}
