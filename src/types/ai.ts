// ============================================================================
// Shared TypeScript Types — AI Advisory
// ============================================================================

export interface AiAdvisory {
  deviceId: string;
  mode: 'local';
  confidenceScore: number;
  summary: string;
  rationale: string[];
  cautions: string[];
  nextSteps: string[];
  expertNotes: string[];
  recommendedActionType?: string;
  recommendedActionTitle?: string;
  cloudAvailable: boolean;
}

export interface AiRecoveryCounts {
  exportNow: number;
  verifyWithPreview: number;
  complexRecoveryReview: number;
  reviewFirst: number;
  unstable: number;
  deleted: number;
  carved: number;
  fragmented: number;
  previewable: number;
  compressed: number;
  snapshotDerived: number;
  journalDerived: number;
  apfsCatalogPreviewFirst: number;
  apfsCatalogReassembled: number;
}

export interface AiRecoveryBrief {
  scanId: string;
  mode: 'local-results';
  confidenceScore: number;
  summary: string;
  strategyTitle: string;
  strategyReasoning: string[];
  evidence: string[];
  cautions: string[];
  nextSteps: string[];
  expertNotes: string[];
  priorityOrder: string[];
  stabilityReason: string;
  blockedBy: string[];
  safeExportStrategy: string;
  complexitySummary: string;
  counts: AiRecoveryCounts;
}
