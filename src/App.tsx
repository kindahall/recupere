import { RouterProvider } from 'react-router-dom';
import { Onboarding } from './components/onboarding/Onboarding';
import { useBackgroundScanPoller } from './hooks/useBackgroundScanPoller';
import { router } from './router';
import { useAppStore } from './stores/appStore';
import './i18n';

function App() {
  const onboardingComplete = useAppStore((s) => s.onboardingComplete);

  // Poll ALL tracked scans in the background (not just the focused one).
  useBackgroundScanPoller();

  if (!onboardingComplete) {
    return <Onboarding />;
  }

  return <RouterProvider router={router} />;
}

export default App;
