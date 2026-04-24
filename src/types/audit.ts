export type AuditEvent =
  | 'device_selected'
  | 'scan_started'
  | 'scan_completed'
  | 'scan_canceled'
  | 'scan_failed'
  | 'export_started'
  | 'export_completed'
  | 'export_failed'
  | 'settings_changed'
  | 'history_purged'
  | 'imaging_started'
  | 'imaging_completed'
  | 'imaging_failed'
  | 'audit_exported'
  | 'filesystem_snapshot_captured'
  | 'filesystem_snapshot_failed'
  | 'filesystem_diff_computed'
  | 'filesystem_diff_failed';

export interface AuditRecord {
  id: number;
  timestampMs: number;
  event: AuditEvent;
  details: unknown;
  prevHash: string;
}

export type AuditChainStatus = 'ok' | 'unverified' | 'broken';

export interface AuditChainReport {
  total: number;
  ok: number;
  unverified: number;
  broken: number;
  firstBrokenId: number | null;
  chainTipHash: string;
  genesisPrevHash: string;
  statuses: Array<[number, AuditChainStatus]>;
}

export interface TraceEntry {
  timestampMs: number;
  level: string;
  target: string;
  message: string;
}
