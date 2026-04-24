import { Download, FileCheck, HardDrive, Search, Stethoscope } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

const STEPS = [
  { path: '/devices', icon: HardDrive, key: 'nav.devices' },
  { path: '/diagnostic', icon: Stethoscope, key: 'nav.diagnostic' },
  { path: '/scan', icon: Search, key: 'nav.scan' },
  { path: '/results', icon: FileCheck, key: 'nav.results' },
  { path: '/export', icon: Download, key: 'nav.export' },
];

export function RecoveryStepIndicator() {
  const { t } = useTranslation();
  const location = useLocation();

  const currentIndex = STEPS.findIndex((s) => location.pathname === s.path);
  if (currentIndex < 0) return null; // Don't show on non-flow pages (home, settings, etc.)

  return (
    <div className="recovery-step-indicator">
      {STEPS.map((step, index) => {
        const Icon = step.icon;
        const isActive = index === currentIndex;
        const isCompleted = index < currentIndex;
        const isFuture = index > currentIndex;

        return (
          <div key={step.path} className="recovery-step-item">
            {index > 0 && (
              <div
                className={`recovery-step-line ${isCompleted ? 'completed' : isFuture ? 'future' : 'active'}`}
              />
            )}
            <div
              className={`recovery-step-dot ${isActive ? 'active' : isCompleted ? 'completed' : 'future'}`}
            >
              <Icon size={12} />
            </div>
            <span
              className={`recovery-step-label ${isActive ? 'active' : isCompleted ? 'completed' : 'future'}`}
            >
              {t(step.key)}
            </span>
          </div>
        );
      })}
    </div>
  );
}
