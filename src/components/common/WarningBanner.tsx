import { AlertCircle, AlertTriangle, CheckCircle, Info } from 'lucide-react';
import type { ReactNode } from 'react';

type BannerVariant = 'info' | 'warning' | 'danger' | 'success';

interface WarningBannerProps {
  variant: BannerVariant;
  children: ReactNode;
}

const icons: Record<BannerVariant, typeof Info> = {
  info: Info,
  warning: AlertTriangle,
  danger: AlertCircle,
  success: CheckCircle,
};

export function WarningBanner({ variant, children }: WarningBannerProps) {
  const Icon = icons[variant];
  return (
    <div className={`warning-banner warning-banner-${variant}`}>
      <Icon className="banner-icon" />
      <span className="banner-text">{children}</span>
    </div>
  );
}
