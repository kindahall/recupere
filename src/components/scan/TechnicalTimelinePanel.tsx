import type { TechnicalLogSource, TechnicalTimelineEntry } from '../../utils/technicalTimeline';

interface TechnicalTimelinePanelProps {
  entries: TechnicalTimelineEntry[];
  emptyMessage?: string;
  sourceLabel: (source: TechnicalLogSource) => string;
}

export function TechnicalTimelinePanel({
  entries,
  emptyMessage = 'Waiting for technical events...',
  sourceLabel,
}: TechnicalTimelinePanelProps) {
  return (
    <div className="scan-log-panel">
      {entries.map((entry, index) => (
        <div
          key={`${entry.source}-${entry.sessionId}-${entry.timestampMs}-${index}`}
          className="scan-log-entry"
        >
          <span className="scan-log-time">{new Date(entry.timestampMs).toLocaleTimeString()}</span>
          <span
            className={`scan-log-source scan-log-source-${entry.source}`}
            title={entry.sessionId}
          >
            {sourceLabel(entry.source)}
          </span>
          <span className={`scan-log-level ${entry.level}`}>{entry.level.toUpperCase()}</span>
          <span className="scan-log-message">{entry.message}</span>
        </div>
      ))}
      {entries.length === 0 && (
        <div className="scan-log-entry">
          <span className="scan-log-message text-muted">{emptyMessage}</span>
        </div>
      )}
    </div>
  );
}
