import { Suspense, lazy } from 'react';
import { Navigate, createBrowserRouter } from 'react-router-dom';
import { LoadingState } from './components/common/LoadingState';
import { AppShell } from './components/layout/AppShell';
import { useAppStore } from './stores/appStore';

const HomePage = lazy(() =>
  import('./pages/HomePage').then((module) => ({ default: module.HomePage })),
);
const DevicesPage = lazy(() =>
  import('./pages/DevicesPage').then((module) => ({ default: module.DevicesPage })),
);
const DiagnosticPage = lazy(() =>
  import('./pages/DiagnosticPage').then((module) => ({ default: module.DiagnosticPage })),
);
const ScanPage = lazy(() =>
  import('./pages/ScanPage').then((module) => ({ default: module.ScanPage })),
);
const ResultsPage = lazy(() =>
  import('./pages/ResultsPage').then((module) => ({ default: module.ResultsPage })),
);
const ExportPage = lazy(() =>
  import('./pages/ExportPage').then((module) => ({ default: module.ExportPage })),
);
const HistoryPage = lazy(() =>
  import('./pages/HistoryPage').then((module) => ({ default: module.HistoryPage })),
);
const ExpertPage = lazy(() =>
  import('./pages/ExpertPage').then((module) => ({ default: module.ExpertPage })),
);
const SettingsPage = lazy(() =>
  import('./pages/SettingsPage').then((module) => ({ default: module.SettingsPage })),
);
const MissingFilesPage = lazy(() =>
  import('./pages/MissingFilesPage').then((module) => ({ default: module.MissingFilesPage })),
);

function lazyRouteElement(element: React.ReactNode) {
  return <Suspense fallback={<LoadingState />}>{element}</Suspense>;
}

function RequireScanContext({ children }: { children: React.ReactNode }) {
  const selectedDeviceId = useAppStore((state) => state.selectedDeviceId);
  const activeScanId = useAppStore((state) => state.activeScanId);
  const activeRemoteAgentId = useAppStore((state) => state.activeRemoteAgentId);
  const scanConfigDeviceId = useAppStore((state) => state.scanConfig?.deviceId ?? null);

  const hasScanContext = Boolean(
    selectedDeviceId || activeScanId || activeRemoteAgentId || scanConfigDeviceId,
  );

  if (!hasScanContext) {
    return <Navigate to="/devices" replace />;
  }

  return <>{children}</>;
}

function RequireActiveScan({ children }: { children: React.ReactNode }) {
  const activeScanId = useAppStore((state) => state.activeScanId);
  const selectedDeviceId = useAppStore((state) => state.selectedDeviceId);

  if (!activeScanId) {
    return <Navigate to={selectedDeviceId ? '/scan' : '/devices'} replace />;
  }

  return <>{children}</>;
}

function RequireExpertMode({ children }: { children: React.ReactNode }) {
  // The Expert workbench exposes hex viewers, RAID detection, partition
  // forensics, and raw timeline filters. AGENTS.md forbids surfacing these
  // by default to non-expert users — bouncing them back to Settings lets
  // them opt in explicitly rather than being dropped into advanced tooling
  // via a stale bookmark or a mis-copied URL.
  const userMode = useAppStore((state) => state.userMode);
  if (userMode !== 'expert') {
    return <Navigate to="/settings?from=expert" replace />;
  }
  return <>{children}</>;
}

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: lazyRouteElement(<HomePage />) },
      { path: 'devices', element: lazyRouteElement(<DevicesPage />) },
      { path: 'diagnostic', element: lazyRouteElement(<DiagnosticPage />) },
      {
        path: 'scan',
        element: lazyRouteElement(
          <RequireScanContext>
            <ScanPage />
          </RequireScanContext>,
        ),
      },
      {
        path: 'results',
        element: lazyRouteElement(
          <RequireActiveScan>
            <ResultsPage />
          </RequireActiveScan>,
        ),
      },
      {
        path: 'export',
        element: lazyRouteElement(
          <RequireActiveScan>
            <ExportPage />
          </RequireActiveScan>,
        ),
      },
      { path: 'history', element: lazyRouteElement(<HistoryPage />) },
      { path: 'missing-files', element: lazyRouteElement(<MissingFilesPage />) },
      {
        path: 'expert',
        element: lazyRouteElement(
          <RequireExpertMode>
            <ExpertPage />
          </RequireExpertMode>,
        ),
      },
      { path: 'settings', element: lazyRouteElement(<SettingsPage />) },
    ],
  },
]);
