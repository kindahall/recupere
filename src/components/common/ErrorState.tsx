import { AlertCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface ErrorStateProps {
  title?: string;
  description?: string;
  onRetry?: () => void;
}

export function ErrorState({ title, description, onRetry }: ErrorStateProps) {
  const { t } = useTranslation();
  return (
    <div className="error-state">
      <AlertCircle className="error-state-icon" />
      <h3 className="error-state-title">{title || t('common.error')}</h3>
      {description && <p className="error-state-desc">{description}</p>}
      {onRetry && (
        <button type="button" className="btn btn-primary" onClick={onRetry}>
          {t('common.retry')}
        </button>
      )}
    </div>
  );
}
