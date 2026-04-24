import { invoke, isTauri } from '@tauri-apps/api/core';
import type { AuditChainReport, AuditRecord, TraceEntry } from '../../types';

interface RustAuditRecord {
  id: number;
  timestamp_ms: number;
  event: AuditRecord['event'];
  details: unknown;
  prev_hash: string;
}

interface RustAuditChainReport {
  total: number;
  ok: number;
  unverified: number;
  broken: number;
  first_broken_id: number | null;
  chain_tip_hash: string;
  genesis_prev_hash: string;
  statuses: Array<[number, AuditChainReport['statuses'][number][1]]>;
}

interface RustTraceEntry {
  timestamp_ms: number;
  level: string;
  target: string;
  message: string;
}

function mapAuditRecord(record: RustAuditRecord): AuditRecord {
  return {
    id: record.id,
    timestampMs: record.timestamp_ms,
    event: record.event,
    details: record.details,
    prevHash: record.prev_hash,
  };
}

function mapAuditChainReport(report: RustAuditChainReport): AuditChainReport {
  return {
    total: report.total,
    ok: report.ok,
    unverified: report.unverified,
    broken: report.broken,
    firstBrokenId: report.first_broken_id,
    chainTipHash: report.chain_tip_hash,
    genesisPrevHash: report.genesis_prev_hash,
    statuses: report.statuses,
  };
}

function mapTraceEntry(entry: RustTraceEntry): TraceEntry {
  return {
    timestampMs: entry.timestamp_ms,
    level: entry.level,
    target: entry.target,
    message: entry.message,
  };
}

export async function fetchAuditTrail(): Promise<AuditRecord[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return [];
  }
  const records = await invoke<RustAuditRecord[]>('get_audit_trail');
  return records.map(mapAuditRecord);
}

export async function verifyAuditTrail(): Promise<AuditChainReport> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return {
      total: 0,
      ok: 0,
      unverified: 0,
      broken: 0,
      firstBrokenId: null,
      chainTipHash: '0000000000000000000000000000000000000000000000000000000000000000',
      genesisPrevHash: '0000000000000000000000000000000000000000000000000000000000000000',
      statuses: [],
    };
  }
  const report = await invoke<RustAuditChainReport>('verify_audit_trail');
  return mapAuditChainReport(report);
}

export async function fetchRecentTraces(limit?: number): Promise<TraceEntry[]> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return [];
  }
  const traces = await invoke<RustTraceEntry[]>('get_recent_traces', { limit });
  return traces.map(mapTraceEntry);
}

export async function clearRecentTraces(): Promise<void> {
  if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
    return;
  }
  await invoke('clear_recent_traces');
}
