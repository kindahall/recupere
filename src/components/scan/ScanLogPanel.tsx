import type { ScanLogEntry } from '../../types';

interface ScanLogPanelProps {
  logs: ScanLogEntry[];
  emptyMessage?: string;
}

export function ScanLogPanel({
  logs,
  emptyMessage = 'Waiting for log entries...',
}: ScanLogPanelProps) {
  return (
    <div className="scan-log-panel">
      {logs.map((log, i) => (
        <div key={i} className="scan-log-entry">
          <span className="scan-log-time">{new Date(log.timestampMs).toLocaleTimeString()}</span>
          <span className={`scan-log-level ${log.level}`}>{log.level.toUpperCase()}</span>
          <span className="scan-log-message">{log.message}</span>
        </div>
      ))}
      {logs.length === 0 && (
        <div className="scan-log-entry">
          <span className="scan-log-message text-muted">{emptyMessage}</span>
        </div>
      )}
    </div>
  );
}
