import type { RecoveredFile } from '../../types';

export const filterInputStyle = {
  width: '100%',
  padding: 'var(--space-2) var(--space-3)',
  borderRadius: 'var(--radius-md)',
  border: '1px solid var(--color-border)',
  background: 'var(--color-bg-secondary)',
  color: 'var(--color-text-primary)',
} as const;

export const filterLabelStyle = {
  display: 'block',
  fontSize: 'var(--font-size-xs)',
  color: 'var(--color-text-secondary)',
  marginBottom: 'var(--space-1)',
} as const;

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

export function formatTimestamp(timestamp: string): string {
  return timestamp.replace('T', ' ');
}

export function getScoreColor(score: number): string {
  if (score >= 75) return 'var(--color-success)';
  if (score >= 50) return 'var(--color-warning)';
  return 'var(--color-danger)';
}

export function classifyRecoveryImportance(score: number): 'critical' | 'high' | 'medium' | 'low' {
  if (score >= 90) return 'critical';
  if (score >= 75) return 'high';
  if (score >= 55) return 'medium';
  return 'low';
}

export function parseOptionalNumber(value: string): number | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : undefined;
}

export function parseOptionalMegabytes(value: string): number | undefined {
  const parsed = parseOptionalNumber(value);
  return typeof parsed === 'number' && parsed >= 0 ? parsed * 1024 * 1024 : undefined;
}

export function translationKeyForSourceView(sourceView: RecoveredFile['sourceView']): string {
  switch (sourceView) {
    case 'snapshot':
      return 'results.source_view_snapshot';
    case 'journal':
      return 'results.source_view_journal';
    case 'recovery-image':
      return 'results.source_view_recovery_image';
    case 'mounted-volume':
      return 'results.source_view_mounted_volume';
    case 'live-catalog':
      return 'results.source_view_live_catalog';
    case 'mixed':
      return 'results.source_view_mixed';
    default:
      return 'results.source_view_unknown';
  }
}

export function translationKeyForValidatorStatus(
  validatorStatus: RecoveredFile['validatorStatus'],
): string {
  switch (validatorStatus) {
    case 'validated':
      return 'results.validator_status_validated';
    case 'reassembled':
      return 'results.validator_status_reassembled';
    case 'partial-unvalidated':
      return 'results.validator_status_partial_unvalidated';
    case 'failed':
      return 'results.validator_status_failed';
    case 'unsupported':
      return 'results.validator_status_unsupported';
    case 'office-validated':
      return 'results.validator_status_office_validated';
    case 'zip-validated':
      return 'results.validator_status_zip_validated';
    default:
      return 'results.validator_status_unknown';
  }
}

export function translationKeyForRecoveryComplexity(
  recoveryComplexity: RecoveredFile['recoveryComplexity'],
): string {
  switch (recoveryComplexity) {
    case 'low':
      return 'results.complexity_low';
    case 'medium':
      return 'results.complexity_medium';
    case 'high':
      return 'results.complexity_high';
    default:
      return 'results.complexity_low';
  }
}
