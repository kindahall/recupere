import { invoke, isTauri } from '@tauri-apps/api/core';
import type { RecoveredFile } from '../../types';
import { getBrowserPreviewRecoveryResult } from '../../utils/browserPreviewSeed';

export interface FileClassification {
  fileId: string;
  category: string;
  importance: string;
  description: string;
  confidence: number;
}

export interface RecoveryPrediction {
  fileId: string;
  successProbability: number;
  estimatedQuality: string;
  riskFactors: string[];
  recommendation: string;
}

export interface NarrativeReport {
  title: string;
  summary: string;
  situationAnalysis: string;
  keyFindings: string[];
  recoveryStrategy: string;
  priorityFiles: string[];
  warnings: string[];
  conclusion: string;
}

export interface ReconstructionStrategy {
  fileId: string;
  strategy: string;
  steps: string[];
  estimatedSuccess: number;
  alternativeStrategies: string[];
}

export interface FileSearchHit {
  id: string;
  name: string;
  path: string;
  extension: string;
  sizeBytes: number;
  integrity: string;
  recoveryScore: number;
  isDeleted: boolean;
}

interface RustFileClassification {
  file_id: string;
  category: string;
  importance: string;
  description: string;
  confidence: number;
}

interface RustRecoveryPrediction {
  file_id: string;
  success_probability: number;
  estimated_quality: string;
  risk_factors: string[];
  recommendation: string;
}

interface RustNarrativeReport {
  title: string;
  summary: string;
  situation_analysis: string;
  key_findings: string[];
  recovery_strategy: string;
  priority_files: string[];
  warnings: string[];
  conclusion: string;
}

interface RustReconstructionStrategy {
  file_id: string;
  strategy: string;
  steps: string[];
  estimated_success: number;
  alternative_strategies: string[];
}

function browserPreviewFiles(scanId: string): RecoveredFile[] {
  return getBrowserPreviewRecoveryResult(scanId)?.files ?? [];
}

function classifyPreviewCategory(file: RecoveredFile): string {
  const extension = file.extension.toLowerCase();
  if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic', 'bmp', 'tif', 'tiff'].includes(extension))
    return 'photos';
  if (['doc', 'docx', 'pdf', 'txt', 'rtf', 'odt', 'xls', 'xlsx', 'ppt', 'pptx'].includes(extension))
    return 'documents';
  if (['mp4', 'mov', 'avi', 'mkv', 'webm'].includes(extension)) return 'videos';
  if (['mp3', 'wav', 'flac', 'aac', 'm4a'].includes(extension)) return 'music';
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(extension)) return 'documents';
  if (['eml', 'pst', 'mbox'].includes(extension)) return 'email';
  if (
    ['js', 'ts', 'tsx', 'rs', 'py', 'java', 'go', 'cpp', 'c', 'json', 'yml', 'yaml'].includes(
      extension,
    )
  )
    return 'code';
  return 'all';
}

function classifyPreviewImportance(file: RecoveredFile): string {
  if (
    file.isDeleted &&
    file.sourceView === 'live-catalog' &&
    (file.validatorStatus === 'unsupported' || file.validatorStatus === 'partial-unvalidated')
  ) {
    return 'medium';
  }
  if (file.recoveryScore >= 90) return 'critical';
  if (file.recoveryScore >= 75) return 'high';
  if (file.recoveryScore >= 55) return 'medium';
  return 'low';
}

function previewDescription(file: RecoveredFile): string {
  const visibility = file.isDeleted ? 'Deleted' : 'Cataloged';
  if (
    file.isDeleted &&
    file.sourceView === 'live-catalog' &&
    (file.validatorStatus === 'unsupported' || file.validatorStatus === 'partial-unvalidated')
  ) {
    return `${visibility} ${file.extension.toUpperCase() || 'file'} candidate reconstructed from current APFS catalog metadata without structural validation.`;
  }
  return `${visibility} ${file.extension.toUpperCase() || 'file'} candidate scored at ${file.recoveryScore}%.`;
}

export async function classifyScanFiles(scanId: string): Promise<FileClassification[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewFiles(scanId).map((file) => ({
      fileId: file.id,
      category: classifyPreviewCategory(file),
      importance: classifyPreviewImportance(file),
      description: previewDescription(file),
      confidence: Math.max(0.35, Math.min(0.98, file.recoveryScore / 100)),
    }));
  }
  const results = await invoke<RustFileClassification[]>('classify_scan_files', { scanId });
  return results.map((c) => ({
    fileId: c.file_id,
    category: c.category,
    importance: c.importance,
    description: c.description,
    confidence: c.confidence,
  }));
}

export async function predictScanRecovery(scanId: string): Promise<RecoveryPrediction[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewFiles(scanId).map((file) => {
      const base = Math.max(0.15, Math.min(0.99, file.recoveryScore / 100));
      const integrityPenalty =
        file.integrity === 'corrupt'
          ? 0.4
          : file.integrity === 'fragmented'
            ? 0.2
            : file.integrity === 'partial'
              ? 0.1
              : 0;
      const successProbability = Math.max(0.05, Math.min(0.99, base - integrityPenalty));
      return {
        fileId: file.id,
        successProbability,
        estimatedQuality:
          successProbability >= 0.85
            ? 'excellent'
            : successProbability >= 0.65
              ? 'good'
              : successProbability >= 0.4
                ? 'partial'
                : 'poor',
        riskFactors: [
          ...(file.isDeleted ? ['Deleted entry'] : []),
          ...(file.recoveryMethod === 'carving' ? ['Signature carving only'] : []),
          ...(file.integrity !== 'intact' ? [`Integrity: ${file.integrity}`] : []),
        ],
        recommendation:
          successProbability >= 0.7
            ? 'Export after preview verification.'
            : 'Review manually before committing to export.',
      };
    });
  }
  const results = await invoke<RustRecoveryPrediction[]>('predict_scan_recovery', { scanId });
  return results.map((p) => ({
    fileId: p.file_id,
    successProbability: p.success_probability,
    estimatedQuality: p.estimated_quality,
    riskFactors: p.risk_factors,
    recommendation: p.recommendation,
  }));
}

export async function generateNarrativeReport(scanId: string): Promise<NarrativeReport> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const files = browserPreviewFiles(scanId);
    const topFiles = files
      .slice()
      .sort((a, b) => b.recoveryScore - a.recoveryScore)
      .slice(0, 3)
      .map((file) => file.name);
    return {
      title: 'Browser Preview Recovery Summary',
      summary:
        files.length > 0
          ? `${files.length} fixture-backed result(s) are available for review.`
          : 'No preview recovery results are available.',
      situationAnalysis:
        'This narrative is generated from browser-preview fixture data and is intended for UI validation only.',
      keyFindings: [
        `${files.filter((file) => file.integrity === 'intact').length} intact file(s) are ready for first-pass export.`,
        `${files.filter((file) => file.isDeleted).length} deleted file(s) remain visible in the current result set.`,
      ],
      recoveryStrategy:
        'Prioritize intact, high-score files, then review carved or fragmented files with previews before export.',
      priorityFiles: topFiles,
      warnings: ['This report is simulated outside the desktop backend.'],
      conclusion:
        files.length > 0
          ? 'The fixture suggests a safe export-first workflow.'
          : 'Load fixture data or use the desktop backend to generate a real narrative report.',
    };
  }
  const r = await invoke<RustNarrativeReport>('generate_narrative_report', { scanId });
  return {
    title: r.title,
    summary: r.summary,
    situationAnalysis: r.situation_analysis,
    keyFindings: r.key_findings,
    recoveryStrategy: r.recovery_strategy,
    priorityFiles: r.priority_files,
    warnings: r.warnings,
    conclusion: r.conclusion,
  };
}

export async function suggestFileReconstruction(
  scanId: string,
  fileId: string,
): Promise<ReconstructionStrategy> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const file = browserPreviewFiles(scanId).find((candidate) => candidate.id === fileId);
    return {
      fileId,
      strategy:
        file?.integrity === 'corrupt'
          ? 'Attempt header/footer repair before export.'
          : 'Preserve the recovered bytes and verify the preview output first.',
      steps: [
        'Preview the file to confirm format readability.',
        'Export to a separate destination.',
        'Run targeted repair tooling only on the exported copy if needed.',
      ],
      estimatedSuccess: Math.max(0.1, Math.min(0.95, (file?.recoveryScore ?? 40) / 100)),
      alternativeStrategies: [
        'Export intact files first to reduce risk.',
        'Retry from an image if the desktop backend is available.',
      ],
    };
  }
  const r = await invoke<RustReconstructionStrategy>('suggest_file_reconstruction', {
    scanId,
    fileId,
  });
  return {
    fileId: r.file_id,
    strategy: r.strategy,
    steps: r.steps,
    estimatedSuccess: r.estimated_success,
    alternativeStrategies: r.alternative_strategies,
  };
}

export async function smartSelectByCategory(scanId: string, category: string): Promise<string[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return browserPreviewFiles(scanId)
      .filter((file) => category === 'all' || classifyPreviewCategory(file) === category)
      .map((file) => file.id);
  }
  return invoke<string[]>('smart_select_by_category', { scanId, category });
}

export async function buildCloudAiPrompt(scanId: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const files = browserPreviewFiles(scanId);
    return [
      'Browser preview cloud AI prompt',
      `Scan ID: ${scanId}`,
      `Files: ${files.length}`,
      `Top candidates: ${
        files
          .slice(0, 5)
          .map((file) => `${file.name} (${file.recoveryScore}%)`)
          .join(', ') || 'none'
      }`,
    ].join('\n');
  }
  return invoke<string>('build_cloud_ai_prompt', { scanId });
}

export async function runGemmaAnalysis(scanId: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return `Gemma analysis is unavailable in browser preview for scan ${scanId}.`;
  }
  return invoke<string>('run_gemma_analysis', { scanId });
}

export interface GemmaAnalysisProgress {
  inProgress: boolean;
  finished: boolean;
  output: string;
  tokensGenerated: number;
  elapsedSeconds: number;
  error?: string;
}

export async function startGemmaAnalysis(scanId: string): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    void scanId;
    return;
  }
  await invoke('start_gemma_analysis', { scanId });
}

export async function fetchGemmaAnalysisProgress(): Promise<GemmaAnalysisProgress> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return {
      inProgress: false,
      finished: true,
      output: 'Gemma analysis is unavailable in browser preview.',
      tokensGenerated: 0,
      elapsedSeconds: 0,
    };
  }
  const r = await invoke<{
    in_progress: boolean;
    finished: boolean;
    output: string;
    tokens_generated: number;
    elapsed_seconds: number;
    error: string | null;
  }>('get_gemma_analysis_progress');
  return {
    inProgress: r.in_progress,
    finished: r.finished,
    output: r.output,
    tokensGenerated: r.tokens_generated,
    elapsedSeconds: r.elapsed_seconds,
    error: r.error ?? undefined,
  };
}

export async function aiAutopilotScan(deviceId: string): Promise<{
  action: string;
  reason: string;
  nextStep: string;
  scanType: string | null;
  imageFirst: boolean;
}> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return {
      action: 'review',
      reason: 'Browser preview cannot inspect real hardware.',
      nextStep: 'Use the desktop backend for AI-guided scan recommendations.',
      scanType: null,
      imageFirst: false,
    };
  }
  const r = await invoke<{
    action: string;
    reason: string;
    next_step: string;
    scan_type: string | null;
    image_first: boolean;
  }>('ai_autopilot_scan', { deviceId });
  return {
    action: r.action,
    reason: r.reason,
    nextStep: r.next_step,
    scanType: r.scan_type,
    imageFirst: r.image_first,
  };
}

export async function chatWithAi(scanId: string, userMessage: string): Promise<string> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return `Browser preview assistant: no live model is available for scan ${scanId}. Your message was: ${userMessage}`;
  }
  return invoke<string>('chat_with_ai', { scanId, userMessage });
}

export async function searchFileByName(scanId: string, query: string): Promise<FileSearchHit[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    const normalizedQuery = query.trim().toLowerCase();
    return browserPreviewFiles(scanId)
      .filter(
        (file) =>
          normalizedQuery.length === 0 ||
          file.name.toLowerCase().includes(normalizedQuery) ||
          file.path.toLowerCase().includes(normalizedQuery),
      )
      .map((file) => ({
        id: file.id,
        name: file.name,
        path: file.path,
        extension: file.extension,
        sizeBytes: file.sizeBytes,
        integrity: file.integrity,
        recoveryScore: file.recoveryScore,
        isDeleted: Boolean(file.isDeleted),
      }));
  }
  const results = await invoke<
    {
      id: string;
      name: string;
      path: string;
      extension: string;
      size_bytes: number;
      integrity: string;
      recovery_score: number;
      is_deleted: boolean;
    }[]
  >('search_file_by_name', { scanId, query });
  return results.map((r) => ({
    id: r.id,
    name: r.name,
    path: r.path,
    extension: r.extension,
    sizeBytes: r.size_bytes,
    integrity: r.integrity,
    recoveryScore: r.recovery_score,
    isDeleted: r.is_deleted,
  }));
}
