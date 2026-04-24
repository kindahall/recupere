import { useTranslation } from 'react-i18next';
import type { RiskLevel } from '../../types';

interface RiskBadgeProps {
  level: RiskLevel;
}

export function RiskBadge({ level }: RiskBadgeProps) {
  const { t } = useTranslation();
  return <span className={`badge badge-risk-${level}`}>{t(`risk.${level}`)}</span>;
}
