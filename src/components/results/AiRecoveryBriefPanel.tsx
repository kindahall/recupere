import { Brain, ShieldAlert, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AiRecoveryBrief, UserMode } from '../../types';
import { resolveKey } from '../../utils/i18nResolve';
import { WarningBanner } from '../common/WarningBanner';

interface AiRecoveryBriefPanelProps {
  brief: AiRecoveryBrief | null;
  loading: boolean;
  error: string | null;
  userMode: UserMode;
}

export function AiRecoveryBriefPanel({
  brief,
  loading,
  error,
  userMode,
}: AiRecoveryBriefPanelProps) {
  const { t } = useTranslation();

  if (loading) {
    return <div className="text-sm text-secondary">{t('results.ai_loading')}</div>;
  }

  if (error || !brief) {
    return <WarningBanner variant="info">{error ?? t('results.ai_unavailable')}</WarningBanner>;
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
            <div className="font-medium">{t('results.ai_strategy')}</div>
            <div className="text-sm text-secondary">{resolveKey(t, brief.strategyTitle)}</div>
          </div>
        </div>

        <div className="text-sm text-secondary">
          {t('results.ai_confidence', { score: brief.confidenceScore })}
        </div>
      </div>

      <p className="text-sm" style={{ margin: 0 }}>
        {resolveKey(t, brief.summary)}
      </p>

      <div className="grid grid-4 gap-3">
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_export_now')}</span>
          <span className="stat-card-value text-success">{brief.counts.exportNow}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_verify_with_preview')}</span>
          <span className="stat-card-value text-warning">{brief.counts.verifyWithPreview}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_complex_recovery_review')}</span>
          <span className="stat-card-value">{brief.counts.complexRecoveryReview}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_unstable')}</span>
          <span className="stat-card-value text-danger">{brief.counts.unstable}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_compressed')}</span>
          <span className="stat-card-value">{brief.counts.compressed}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_snapshot')}</span>
          <span className="stat-card-value">{brief.counts.snapshotDerived}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_journal')}</span>
          <span className="stat-card-value">{brief.counts.journalDerived}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_apfs_preview_first')}</span>
          <span className="stat-card-value text-warning">
            {brief.counts.apfsCatalogPreviewFirst}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_apfs_reassembled')}</span>
          <span className="stat-card-value text-success">
            {brief.counts.apfsCatalogReassembled}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_complex')}</span>
          <span className="stat-card-value">{brief.counts.fragmented + brief.counts.carved}</span>
        </div>
      </div>

      <div className="grid grid-2 gap-3">
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_stability_reason')}</span>
          <span className="text-sm text-secondary">{resolveKey(t, brief.stabilityReason)}</span>
        </div>
        <div className="stat-card">
          <span className="stat-card-label">{t('results.ai_safe_export_strategy')}</span>
          <span className="text-sm text-secondary">{resolveKey(t, brief.safeExportStrategy)}</span>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('results.ai_complexity_summary')}</div>
        <div className="text-sm text-secondary">{resolveKey(t, brief.complexitySummary)}</div>
      </div>

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('results.ai_reasoning')}</div>
        {brief.strategyReasoning.map((item, i) => (
          <div key={i} className="text-sm text-secondary">
            {resolveKey(t, item)}
          </div>
        ))}
      </div>

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('results.ai_evidence')}</div>
        {brief.evidence.map((item, i) => (
          <div key={i} className="flex items-start gap-2 text-sm text-secondary">
            <Sparkles size={14} className="text-accent" style={{ flexShrink: 0, marginTop: 2 }} />
            <span>{resolveKey(t, item)}</span>
          </div>
        ))}
      </div>

      {brief.cautions.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">{t('results.ai_cautions')}</div>
          {brief.cautions.map((item, i) => (
            <div key={i} className="flex items-start gap-2 text-sm text-secondary">
              <ShieldAlert
                size={15}
                className="text-warning"
                style={{ flexShrink: 0, marginTop: 2 }}
              />
              <span>{resolveKey(t, item)}</span>
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-col gap-2">
        <div className="text-sm font-medium">{t('results.ai_next_steps')}</div>
        {brief.nextSteps.map((item, i) => (
          <div key={i} className="text-sm text-secondary">
            {resolveKey(t, item)}
          </div>
        ))}
      </div>

      {userMode === 'expert' && brief.expertNotes.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">{t('results.ai_expert_notes')}</div>
          {brief.expertNotes.map((item, i) => (
            <div key={i} className="text-sm text-secondary">
              {resolveKey(t, item)}
            </div>
          ))}
        </div>
      )}

      {userMode === 'expert' && brief.blockedBy.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">{t('results.ai_blocked_by')}</div>
          {brief.blockedBy.map((item, i) => (
            <div key={i} className="text-sm text-secondary">
              {resolveKey(t, item)}
            </div>
          ))}
        </div>
      )}

      {brief.priorityOrder.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">{t('results.ai_priority_order')}</div>
          {brief.priorityOrder.map((item, index) => (
            <div key={item} className="text-sm text-secondary">
              {index + 1}. {item}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
