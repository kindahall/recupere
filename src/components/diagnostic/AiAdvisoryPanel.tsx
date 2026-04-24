import { Brain, CloudOff, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AiAdvisory, UserMode } from '../../types';
import { resolveKey } from '../../utils/i18nResolve';
import { WarningBanner } from '../common/WarningBanner';

interface AiAdvisoryPanelProps {
  advisory: AiAdvisory | null;
  loading: boolean;
  error: string | null;
  userMode: UserMode;
}

export function AiAdvisoryPanel({ advisory, loading, error, userMode }: AiAdvisoryPanelProps) {
  const { t } = useTranslation();

  if (loading) {
    return <div className="text-sm text-secondary">{t('diagnostic.ai_loading')}</div>;
  }

  if (error || !advisory) {
    return <WarningBanner variant="info">{error ?? t('diagnostic.ai_unavailable')}</WarningBanner>;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-start gap-3">
          <div
            className="flex items-center justify-center"
            style={{
              width: 40,
              height: 40,
              borderRadius: 'var(--radius-lg)',
              background: 'var(--color-bg-secondary)',
              border: '1px solid var(--color-border)',
              flexShrink: 0,
            }}
          >
            <Brain size={18} className="text-accent" />
          </div>
          <div className="flex flex-col gap-1">
            <div className="font-medium">{t('diagnostic.ai_mode_local')}</div>
            <div className="text-sm text-secondary">
              {t('diagnostic.ai_confidence', { score: advisory.confidenceScore })}
            </div>
          </div>
        </div>

        {!advisory.cloudAvailable && (
          <span className="badge-device" title={t('settings.cloud_unavailable')}>
            <CloudOff size={14} />
          </span>
        )}
      </div>

      <p className="text-sm" style={{ margin: 0 }}>
        {resolveKey(t, advisory.summary)}
      </p>

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('diagnostic.ai_next_steps')}</div>
        <div className="flex flex-col gap-2">
          {advisory.nextSteps.map((step, i) => (
            <div key={i} className="text-sm text-secondary">
              {resolveKey(t, step)}
            </div>
          ))}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('diagnostic.ai_rationale')}</div>
        <div className="flex flex-col gap-2">
          {advisory.rationale.map((item, i) => (
            <div key={i} className="text-sm text-secondary">
              {resolveKey(t, item)}
            </div>
          ))}
        </div>
      </div>

      {advisory.cautions.length > 0 && (
        <div className="flex flex-col gap-3">
          <div className="text-sm font-medium">{t('diagnostic.ai_cautions')}</div>
          {advisory.cautions.map((caution, i) => (
            <div key={i} className="flex items-start gap-2 text-sm text-secondary">
              <ShieldAlert
                size={16}
                className="text-accent"
                style={{ flexShrink: 0, marginTop: 2 }}
              />
              <span>{resolveKey(t, caution)}</span>
            </div>
          ))}
        </div>
      )}

      {userMode === 'expert' && advisory.expertNotes.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">{t('diagnostic.ai_expert_notes')}</div>
          <div className="flex flex-col gap-2">
            {advisory.expertNotes.map((note, i) => (
              <div key={i} className="text-sm text-secondary">
                {resolveKey(t, note)}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
