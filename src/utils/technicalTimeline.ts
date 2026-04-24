import type { ScanLogEntry } from '../types';

export type TechnicalLogSource = 'scan' | 'export';

export interface TechnicalTimelineEntry extends ScanLogEntry {
  source: TechnicalLogSource;
  sessionId: string;
  order: number;
}

export type TechnicalTimelineSourceFilter = 'all' | 'scan' | 'export';
export type TechnicalTimelineSeverityFilter = 'all' | ScanLogEntry['level'];

interface BuildTechnicalTimelineInput {
  scanLogs: ScanLogEntry[];
  activeScanId?: string | null;
  exportLogs: ScanLogEntry[];
  activeExportId?: string | null;
}

interface FilterTechnicalTimelineInput {
  entries: TechnicalTimelineEntry[];
  sourceFilter?: TechnicalTimelineSourceFilter;
  severityFilter?: TechnicalTimelineSeverityFilter;
  query?: string;
}

export function buildTechnicalTimeline({
  scanLogs,
  activeScanId,
  exportLogs,
  activeExportId,
}: BuildTechnicalTimelineInput): TechnicalTimelineEntry[] {
  let order = 0;
  const timeline: TechnicalTimelineEntry[] = [];

  if (activeScanId) {
    for (const log of scanLogs) {
      timeline.push({
        ...log,
        source: 'scan',
        sessionId: activeScanId,
        order: order++,
      });
    }
  }

  if (activeExportId) {
    for (const log of exportLogs) {
      timeline.push({
        ...log,
        source: 'export',
        sessionId: activeExportId,
        order: order++,
      });
    }
  }

  return timeline.sort((left, right) => {
    if (left.timestampMs !== right.timestampMs) {
      return right.timestampMs - left.timestampMs;
    }

    return right.order - left.order;
  });
}

export function filterTechnicalTimeline({
  entries,
  sourceFilter = 'all',
  severityFilter = 'all',
  query = '',
}: FilterTechnicalTimelineInput): TechnicalTimelineEntry[] {
  const normalizedQuery = query.trim().toLowerCase();

  return entries.filter((entry) => {
    if (sourceFilter !== 'all' && entry.source !== sourceFilter) {
      return false;
    }

    if (severityFilter !== 'all' && entry.level !== severityFilter) {
      return false;
    }

    if (!normalizedQuery) {
      return true;
    }

    return (
      entry.message.toLowerCase().includes(normalizedQuery) ||
      entry.sessionId.toLowerCase().includes(normalizedQuery) ||
      entry.source.toLowerCase().includes(normalizedQuery)
    );
  });
}

export function formatTechnicalTimelineExport(entries: TechnicalTimelineEntry[]): string {
  return entries
    .map((entry) => {
      const timestamp = new Date(entry.timestampMs).toISOString();
      return `${timestamp} [${entry.source.toUpperCase()}] [${entry.level.toUpperCase()}] (${entry.sessionId}) ${entry.message}`;
    })
    .join('\n');
}
