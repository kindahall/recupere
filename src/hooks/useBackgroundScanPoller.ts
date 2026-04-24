// Background poller for ALL tracked scans — not just the focused one.
//
// Runs on a single 1.5 s interval. On each tick it polls progress + logs for
// every tracked scan whose last-known status is non-terminal. Results from
// the polls feed into the per-scan maps (`progressByScanId`, `logsByScanId`)
// in the global store. The focused scan's legacy fields (`scanProgress`,
// `scanLogs`) are kept in sync automatically by the `updateTrackedScan*`
// actions.
//
// Mount this hook once at the app root (e.g. `App.tsx` or the layout shell).

import { useEffect, useRef } from 'react';
import { isTerminalScanStatus, useAppStore } from '../stores/appStore';
import { fetchScanLogsFor, fetchScanProgressFor } from './useIpc';

const POLL_INTERVAL_MS = 1500;

export function useBackgroundScanPoller() {
  const trackedScans = useAppStore((s) => s.trackedScans);
  const progressByScanId = useAppStore((s) => s.progressByScanId);
  const updateTrackedScanProgress = useAppStore((s) => s.updateTrackedScanProgress);
  const updateTrackedScanLogs = useAppStore((s) => s.updateTrackedScanLogs);

  // Keep refs to avoid stale closures in the interval callback.
  const trackedRef = useRef(trackedScans);
  const progressRef = useRef(progressByScanId);
  trackedRef.current = trackedScans;
  progressRef.current = progressByScanId;

  useEffect(() => {
    const poll = async () => {
      const scans = trackedRef.current;
      const progress = progressRef.current;

      for (const scan of scans) {
        const lastStatus = progress[scan.id]?.status;
        if (isTerminalScanStatus(lastStatus)) continue;

        try {
          const [newProgress, newLogs] = await Promise.all([
            fetchScanProgressFor(scan.id, scan.agentId),
            fetchScanLogsFor(scan.id, scan.agentId),
          ]);
          updateTrackedScanProgress(scan.id, newProgress);
          updateTrackedScanLogs(scan.id, newLogs);
        } catch {
          // Swallow per-scan errors — the next tick will retry.
        }
      }
    };

    if (trackedScans.length === 0) return;

    // Fire once immediately, then on interval.
    poll();
    const handle = window.setInterval(poll, POLL_INTERVAL_MS);
    return () => window.clearInterval(handle);
  }, [trackedScans.length, updateTrackedScanProgress, updateTrackedScanLogs]);
}
