import { describe, expect, it } from 'vitest';
import {
  buildTechnicalTimeline,
  filterTechnicalTimeline,
  formatTechnicalTimelineExport,
} from './technicalTimeline';

describe('buildTechnicalTimeline', () => {
  it('merges active scan and export logs in descending timestamp order', () => {
    const timeline = buildTechnicalTimeline({
      activeScanId: 'scan-1',
      scanLogs: [
        { timestampMs: 100, level: 'info', message: 'scan-start' },
        { timestampMs: 300, level: 'warning', message: 'scan-warn' },
      ],
      activeExportId: 'export-1',
      exportLogs: [
        { timestampMs: 200, level: 'info', message: 'export-start' },
        { timestampMs: 400, level: 'error', message: 'export-fail' },
      ],
    });

    expect(timeline.map((entry) => entry.message)).toEqual([
      'export-fail',
      'scan-warn',
      'export-start',
      'scan-start',
    ]);
    expect(timeline.map((entry) => entry.source)).toEqual(['export', 'scan', 'export', 'scan']);
    expect(timeline.map((entry) => entry.sessionId)).toEqual([
      'export-1',
      'scan-1',
      'export-1',
      'scan-1',
    ]);
  });

  it('ignores logs without an active session id', () => {
    const timeline = buildTechnicalTimeline({
      activeScanId: null,
      scanLogs: [{ timestampMs: 100, level: 'info', message: 'stale-scan' }],
      activeExportId: 'export-1',
      exportLogs: [{ timestampMs: 200, level: 'info', message: 'live-export' }],
    });

    expect(timeline).toHaveLength(1);
    expect(timeline[0].message).toBe('live-export');
    expect(timeline[0].source).toBe('export');
  });

  it('keeps a deterministic order for identical timestamps', () => {
    const timeline = buildTechnicalTimeline({
      activeScanId: 'scan-1',
      scanLogs: [{ timestampMs: 100, level: 'info', message: 'scan-entry' }],
      activeExportId: 'export-1',
      exportLogs: [{ timestampMs: 100, level: 'info', message: 'export-entry' }],
    });

    expect(timeline.map((entry) => entry.message)).toEqual(['export-entry', 'scan-entry']);
  });

  it('filters the timeline by source, severity and search query', () => {
    const entries = buildTechnicalTimeline({
      activeScanId: 'scan-1',
      scanLogs: [
        { timestampMs: 100, level: 'info', message: 'scan-entry' },
        { timestampMs: 200, level: 'warning', message: 'scan-warning' },
        { timestampMs: 250, level: 'error', message: 'scan-failure' },
      ],
      activeExportId: 'export-1',
      exportLogs: [
        { timestampMs: 300, level: 'info', message: 'export-entry' },
        { timestampMs: 350, level: 'debug', message: 'export-debug' },
        { timestampMs: 400, level: 'error', message: 'export-failure' },
      ],
    });

    expect(
      filterTechnicalTimeline({
        entries,
        sourceFilter: 'scan',
      }).map((entry) => entry.message),
    ).toEqual(['scan-failure', 'scan-warning', 'scan-entry']);

    expect(
      filterTechnicalTimeline({
        entries,
        severityFilter: 'error',
      }).map((entry) => entry.message),
    ).toEqual(['export-failure', 'scan-failure']);

    expect(
      filterTechnicalTimeline({
        entries,
        severityFilter: 'warning',
      }).map((entry) => entry.message),
    ).toEqual(['scan-warning']);

    expect(
      filterTechnicalTimeline({
        entries,
        severityFilter: 'debug',
      }).map((entry) => entry.message),
    ).toEqual(['export-debug']);

    expect(
      filterTechnicalTimeline({
        entries,
        query: 'export-1',
      }).map((entry) => entry.message),
    ).toEqual(['export-failure', 'export-debug', 'export-entry']);
  });

  it('formats the filtered timeline as stable export text', () => {
    const entries = buildTechnicalTimeline({
      activeScanId: 'scan-1',
      scanLogs: [{ timestampMs: 100, level: 'warning', message: 'scan-warning' }],
      activeExportId: 'export-1',
      exportLogs: [{ timestampMs: 200, level: 'error', message: 'export-failure' }],
    });

    expect(formatTechnicalTimelineExport(entries)).toBe(
      '1970-01-01T00:00:00.200Z [EXPORT] [ERROR] (export-1) export-failure\n1970-01-01T00:00:00.100Z [SCAN] [WARNING] (scan-1) scan-warning',
    );
  });
});
