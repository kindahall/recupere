import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface LoadingStateProps {
  message?: string;
}

export function LoadingState({ message }: LoadingStateProps) {
  const { t } = useTranslation();
  return (
    <div
      className="loading-state"
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 'var(--space-16) var(--space-8)',
      }}
    >
      <Loader2
        size={40}
        style={{
          color: 'var(--color-accent)',
          marginBottom: 'var(--space-5)',
          animation: 'spin 1.5s linear infinite',
          filter: 'drop-shadow(0 0 12px rgba(99, 102, 241, 0.5))',
        }}
      />
      <p
        className="loading-text"
        style={{
          fontSize: 'var(--font-size-md)',
          color: 'var(--color-text-secondary)',
          fontWeight: 'var(--font-weight-medium)',
        }}
      >
        {message || t('common.loading')}
      </p>
    </div>
  );
}
