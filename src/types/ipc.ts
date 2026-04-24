import type { ScanProgress, ScanStage, ScanStatus } from './scan';

export interface RustScanProgress {
  status: unknown;
  stage: unknown;
  percent_complete: unknown;
  bytes_scanned: unknown;
  total_bytes: unknown;
  files_found: unknown;
  errors_count: unknown;
  elapsed_seconds: unknown;
  resume_from_bytes?: unknown;
  unreadable_ranges_count?: unknown;
  unreadable_bytes?: unknown;
  rescued_after_retry_bytes?: unknown;
  retry_passes_completed?: unknown;
  unreadable_ranges?: unknown;
  estimated_remaining_seconds?: unknown;
  current_sector?: unknown;
}

const SCAN_STATUSES: readonly ScanStatus[] = [
  'idle',
  'preparing',
  'scanning',
  'paused',
  'completed',
  'cancelled',
  'error',
];

const SCAN_STAGES: readonly ScanStage[] = [
  'initializing',
  'creating-image',
  'reading-partition-table',
  'analyzing-filesystem',
  'scanning-deleted-entries',
  'carving-signatures',
  'scoring-results',
  'finalizing',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function expectFiniteNumber(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new Error(`Invalid scan progress payload: ${field} must be a finite number.`);
  }
  return value;
}

function readOptionalFiniteNumber(value: unknown, field: string): number | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }
  return expectFiniteNumber(value, field);
}

function readOptionalUnreadableRanges(
  value: unknown,
): Array<{ startOffset: number; length: number }> | undefined {
  if (value === undefined || value === null) {
    return undefined;
  }
  if (!Array.isArray(value)) {
    throw new Error('Invalid scan progress payload: unreadable_ranges must be an array.');
  }
  return value.map((entry, index) => {
    if (!isRecord(entry)) {
      throw new Error(
        `Invalid scan progress payload: unreadable_ranges[${index}] must be an object.`,
      );
    }
    return {
      startOffset: expectFiniteNumber(
        entry.start_offset,
        `unreadable_ranges[${index}].start_offset`,
      ),
      length: expectFiniteNumber(entry.length, `unreadable_ranges[${index}].length`),
    };
  });
}

export function isScanStatus(value: unknown): value is ScanStatus {
  return typeof value === 'string' && SCAN_STATUSES.includes(value as ScanStatus);
}

export function isScanStage(value: unknown): value is ScanStage {
  return typeof value === 'string' && SCAN_STAGES.includes(value as ScanStage);
}

export function mapRustScanProgress(raw: unknown): ScanProgress {
  if (!isRecord(raw)) {
    throw new Error('Invalid scan progress payload: expected an object.');
  }

  if (!isScanStatus(raw.status)) {
    throw new Error('Invalid scan progress payload: unknown status.');
  }

  if (!isScanStage(raw.stage)) {
    throw new Error('Invalid scan progress payload: unknown stage.');
  }

  return {
    status: raw.status,
    stage: raw.stage,
    percentComplete: expectFiniteNumber(raw.percent_complete, 'percent_complete'),
    bytesScanned: expectFiniteNumber(raw.bytes_scanned, 'bytes_scanned'),
    totalBytes: expectFiniteNumber(raw.total_bytes, 'total_bytes'),
    filesFound: expectFiniteNumber(raw.files_found, 'files_found'),
    errorsCount: expectFiniteNumber(raw.errors_count, 'errors_count'),
    elapsedSeconds: expectFiniteNumber(raw.elapsed_seconds, 'elapsed_seconds'),
    resumeFromBytes: readOptionalFiniteNumber(raw.resume_from_bytes, 'resume_from_bytes') ?? 0,
    unreadableRangesCount:
      readOptionalFiniteNumber(raw.unreadable_ranges_count, 'unreadable_ranges_count') ?? 0,
    unreadableBytes: readOptionalFiniteNumber(raw.unreadable_bytes, 'unreadable_bytes') ?? 0,
    rescuedAfterRetryBytes:
      readOptionalFiniteNumber(raw.rescued_after_retry_bytes, 'rescued_after_retry_bytes') ?? 0,
    retryPassesCompleted:
      readOptionalFiniteNumber(raw.retry_passes_completed, 'retry_passes_completed') ?? 0,
    unreadableRanges: readOptionalUnreadableRanges(raw.unreadable_ranges) ?? [],
    estimatedRemainingSeconds: readOptionalFiniteNumber(
      raw.estimated_remaining_seconds,
      'estimated_remaining_seconds',
    ),
    currentSector: readOptionalFiniteNumber(raw.current_sector, 'current_sector'),
  };
}
