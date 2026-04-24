// Shared raw types and helpers used by multiple ipc/* modules.
import type { ScanLogEntry } from '../../types';

export interface RustTechnicalLogEntry {
  timestamp_ms: number;
  level: string;
  message: string;
}

export function mapTechnicalLog(log: RustTechnicalLogEntry): ScanLogEntry {
  return {
    timestampMs: log.timestamp_ms,
    level: log.level as ScanLogEntry['level'],
    message: log.message,
  };
}
