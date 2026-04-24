// Compact picker showing all tracked scans (local + remote) with live
// progress from the background poller. The active scan is highlighted;
// clicking another pill calls `focusTrackedScan` to switch context.

import { useTranslation } from 'react-i18next';
import { isTerminalScanStatus, useAppStore } from '../../stores/appStore';

function statusIcon(status: string | undefined): string {
  if (!status) return '\u23f3'; // hourglass
  switch (status) {
    case 'completed':
      return '\u2705'; // checkmark
    case 'cancelled':
      return '\u274c'; // cross
    case 'error':
      return '\u26a0\ufe0f'; // warning
    case 'paused':
      return '\u23f8\ufe0f'; // pause
    default:
      return '\u23f3'; // hourglass (scanning / initializing)
  }
}

export function TrackedScansBar() {
  const { t } = useTranslation();
  const trackedScans = useAppStore((s) => s.trackedScans);
  const activeScanId = useAppStore((s) => s.activeScanId);
  const progressByScanId = useAppStore((s) => s.progressByScanId);
  const focusTrackedScan = useAppStore((s) => s.focusTrackedScan);
  const untrackScan = useAppStore((s) => s.untrackScan);

  if (trackedScans.length <= 1) return null;

  return (
    <div className="tracked-scans-bar">
      <span className="tracked-scans-bar__label">{t('scan.tracked_bar_label')}</span>
      {trackedScans.map((scan) => {
        const isActive = scan.id === activeScanId;
        const progress = progressByScanId[scan.id];
        const percent = progress?.percentComplete;
        const status = progress?.status;
        const terminal = isTerminalScanStatus(status);
        const icon = statusIcon(status);
        const label = scan.agentId ? `\ud83c\udf10 ${scan.label}` : scan.label;
        const classes = [
          'tracked-scans-bar__pill',
          isActive ? 'is-active' : '',
          terminal && !isActive ? 'is-terminal-inactive' : '',
        ]
          .filter(Boolean)
          .join(' ');

        return (
          <span
            key={scan.id}
            className={classes}
            onClick={() => !isActive && focusTrackedScan(scan.id)}
            onKeyDown={(event) => {
              if (!isActive && (event.key === 'Enter' || event.key === ' ')) {
                event.preventDefault();
                focusTrackedScan(scan.id);
              }
            }}
            tabIndex={isActive ? -1 : 0}
            title={
              scan.agentId
                ? t('scan.tracked_bar_remote_title', { agentId: scan.agentId })
                : t('scan.tracked_bar_local_title')
            }
          >
            <span>{icon}</span>
            <span className="tracked-scans-bar__pill-label">{label}</span>
            {!terminal && percent != null && (
              <span className="tracked-scans-bar__pill-percent">{percent.toFixed(0)}%</span>
            )}
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                untrackScan(scan.id);
              }}
              className="tracked-scans-bar__pill-close"
              title={t('scan.tracked_bar_stop_tracking')}
              aria-label={t('scan.tracked_bar_stop_tracking')}
            >
              {'\u00d7'}
            </button>
          </span>
        );
      })}
    </div>
  );
}
