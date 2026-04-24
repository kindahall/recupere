import { useTranslation } from 'react-i18next';
import type { FileIntegrity } from '../../types';

interface IntegrityBadgeProps {
  integrity: FileIntegrity;
}

export function IntegrityBadge({ integrity }: IntegrityBadgeProps) {
  const { t } = useTranslation();
  return (
    <span className={`badge badge-integrity-${integrity}`}>{t(`integrity.${integrity}`)}</span>
  );
}
