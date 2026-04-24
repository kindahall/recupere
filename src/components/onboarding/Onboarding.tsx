// ============================================================================
// Récupère — First-launch Onboarding
// ============================================================================
// Polished multi-step onboarding shown the first time the app is launched.
// Steps: Welcome → Mode (AI Guided / Manual) → AI Setup (Gemma) → Ready.
// The AI Setup step is automatically skipped when the user picks Manual mode.
// State is persisted via the appStore localStorage layer so the onboarding
// only ever runs once.
// ============================================================================

import { openUrl } from '@tauri-apps/plugin-opener';
import {
  ArrowLeft,
  ArrowRight,
  Brain,
  Check,
  Download,
  ExternalLink,
  Lock,
  RefreshCw,
  Shield,
  Sparkles,
  Wrench,
  Zap,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  type GemmaPullProgress,
  type GemmaStatus,
  fetchGemmaPullProgress,
  fetchGemmaStatus,
  startGemmaPull,
} from '../../hooks/useIpc';
import { useAppStore } from '../../stores/appStore';

type StepKey = 'welcome' | 'mode' | 'ai-setup' | 'ready';

export function Onboarding() {
  const { t } = useTranslation();
  const setAppMode = useAppStore((s) => s.setAppMode);
  const setOnboardingComplete = useAppStore((s) => s.setOnboardingComplete);

  const [step, setStep] = useState<StepKey>('welcome');
  const [chosenMode, setChosenMode] = useState<'ai-guided' | 'manual' | null>(null);
  const [gemmaStatus, setGemmaStatus] = useState<GemmaStatus | null>(null);
  const [pullProgress, setPullProgress] = useState<GemmaPullProgress | null>(null);
  const [pullStarting, setPullStarting] = useState(false);
  const [pullError, setPullError] = useState<string | null>(null);
  const pullPollRef = useRef<number | null>(null);

  // Build the active list of steps depending on the chosen mode.
  const steps: StepKey[] =
    chosenMode === 'manual'
      ? ['welcome', 'mode', 'ready']
      : ['welcome', 'mode', 'ai-setup', 'ready'];
  const stepIndex = steps.indexOf(step);
  const totalSteps = steps.length;

  // ---- Gemma status polling on the AI step --------------------------------
  useEffect(() => {
    if (step !== 'ai-setup') return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const s = await fetchGemmaStatus();
        if (!cancelled) setGemmaStatus(s);
      } catch {
        if (!cancelled) setGemmaStatus(null);
      }
    };
    refresh();
    const interval = window.setInterval(refresh, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [step]);

  // ---- Pull progress polling ---------------------------------------------
  const stopPullPolling = useCallback(() => {
    if (pullPollRef.current !== null) {
      window.clearInterval(pullPollRef.current);
      pullPollRef.current = null;
    }
  }, []);
  useEffect(() => () => stopPullPolling(), [stopPullPolling]);

  const handleStartPull = async () => {
    setPullError(null);
    setPullStarting(true);
    try {
      await startGemmaPull();
      stopPullPolling();
      pullPollRef.current = window.setInterval(async () => {
        try {
          const p = await fetchGemmaPullProgress();
          setPullProgress(p);
          if (p.finished) {
            stopPullPolling();
            setPullStarting(false);
            if (p.error) {
              setPullError(p.error);
            } else {
              // Refresh the Gemma status — should now be ready.
              try {
                const status = await fetchGemmaStatus();
                setGemmaStatus(status);
              } catch {
                /* ignore */
              }
            }
          }
        } catch {
          /* keep polling on transient errors */
        }
      }, 750);
    } catch (err) {
      setPullStarting(false);
      setPullError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleOpenOllama = async () => {
    try {
      await openUrl('https://ollama.com/download');
    } catch {
      /* opener unavailable in browser preview */
    }
  };

  // ---- Navigation ---------------------------------------------------------
  const goNext = () => {
    const next = steps[stepIndex + 1];
    if (next) setStep(next);
  };
  const goBack = () => {
    const prev = steps[stepIndex - 1];
    if (prev) setStep(prev);
  };

  const handleFinish = () => {
    if (chosenMode) {
      setAppMode(chosenMode);
    }
    setOnboardingComplete(true);
  };

  // ---- Step content -------------------------------------------------------
  const renderStep = () => {
    switch (step) {
      case 'welcome':
        return <WelcomeStep onNext={goNext} t={t} />;
      case 'mode':
        return (
          <ModeStep
            t={t}
            chosen={chosenMode}
            onChoose={(mode) => {
              setChosenMode(mode);
              // Auto-advance after choosing.
              setTimeout(() => {
                if (mode === 'ai-guided') setStep('ai-setup');
                else setStep('ready');
              }, 200);
            }}
          />
        );
      case 'ai-setup':
        return (
          <AiSetupStep
            t={t}
            status={gemmaStatus}
            pullProgress={pullProgress}
            pullStarting={pullStarting}
            pullError={pullError}
            onInstallOllama={handleOpenOllama}
            onStartPull={handleStartPull}
            onRefresh={async () => {
              try {
                setGemmaStatus(await fetchGemmaStatus());
              } catch {
                /* ignore */
              }
            }}
          />
        );
      case 'ready':
        return <ReadyStep t={t} chosenMode={chosenMode} />;
      default:
        return null;
    }
  };

  const canSkipForward = step === 'ai-setup'; // user can move on without finishing AI setup
  const isLastStep = step === 'ready';

  return (
    <div className="onboarding-root">
      <div className="onboarding-bg-orb onboarding-bg-orb-1" />
      <div className="onboarding-bg-orb onboarding-bg-orb-2" />

      <div className="onboarding-shell">
        {/* Progress dots */}
        <div
          className="onboarding-progress"
          role="progressbar"
          tabIndex={0}
          aria-valuenow={stepIndex + 1}
          aria-valuemin={1}
          aria-valuemax={totalSteps}
          aria-label={`Step ${stepIndex + 1} of ${totalSteps}`}
        >
          {steps.map((s, i) => (
            <div
              key={s}
              className={`onboarding-progress-dot ${
                i < stepIndex ? 'is-done' : i === stepIndex ? 'is-active' : ''
              }`}
            />
          ))}
        </div>

        {/* Current step content */}
        <div key={step} className="onboarding-card">
          {renderStep()}
        </div>

        {/* Footer navigation */}
        <div className="onboarding-footer">
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={goBack}
            disabled={stepIndex === 0}
            style={{ visibility: stepIndex === 0 ? 'hidden' : 'visible' }}
          >
            <ArrowLeft size={14} />
            {t('onboarding.back')}
          </button>

          <span className="text-sm text-secondary">
            {stepIndex + 1} / {totalSteps}
          </span>

          {isLastStep ? (
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={handleFinish}
              data-testid="onboarding-finish"
            >
              {t('onboarding.finish')}
              <ArrowRight size={14} />
            </button>
          ) : step === 'mode' ? (
            <span style={{ width: 90 }} />
          ) : (
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={goNext}
              data-testid="onboarding-next"
            >
              {canSkipForward && (!gemmaStatus || !gemmaStatus.ready)
                ? t('onboarding.skip')
                : t('onboarding.continue')}
              <ArrowRight size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ============================================================================
// Step components
// ============================================================================

interface StepProps {
  t: (key: string) => string;
}

function WelcomeStep({ onNext, t }: StepProps & { onNext: () => void }) {
  return (
    <div className="onboarding-step onboarding-step-welcome">
      <div className="onboarding-logo">
        <Shield size={48} color="white" />
      </div>
      <h1 className="onboarding-title onboarding-title-gradient">Récupère</h1>
      <p className="onboarding-tagline">{t('onboarding.welcome_tagline')}</p>

      <div className="onboarding-feature-grid">
        <div className="onboarding-feature">
          <Lock size={20} />
          <div>
            <div className="onboarding-feature-title">{t('onboarding.feature_safe_title')}</div>
            <div className="onboarding-feature-desc">{t('onboarding.feature_safe_desc')}</div>
          </div>
        </div>
        <div className="onboarding-feature">
          <Brain size={20} />
          <div>
            <div className="onboarding-feature-title">{t('onboarding.feature_ai_title')}</div>
            <div className="onboarding-feature-desc">{t('onboarding.feature_ai_desc')}</div>
          </div>
        </div>
        <div className="onboarding-feature">
          <Zap size={20} />
          <div>
            <div className="onboarding-feature-title">{t('onboarding.feature_fast_title')}</div>
            <div className="onboarding-feature-desc">{t('onboarding.feature_fast_desc')}</div>
          </div>
        </div>
      </div>

      <button
        type="button"
        className="btn btn-primary"
        onClick={onNext}
        data-testid="onboarding-get-started"
        style={{ marginTop: 'var(--space-6)', minWidth: 200 }}
      >
        {t('onboarding.get_started')}
        <ArrowRight size={16} />
      </button>
    </div>
  );
}

function ModeStep({
  t,
  chosen,
  onChoose,
}: StepProps & {
  chosen: 'ai-guided' | 'manual' | null;
  onChoose: (mode: 'ai-guided' | 'manual') => void;
}) {
  return (
    <div className="onboarding-step">
      <h2 className="onboarding-step-title">{t('onboarding.mode_title')}</h2>
      <p className="onboarding-step-subtitle">{t('onboarding.mode_subtitle')}</p>

      <div className="onboarding-mode-grid">
        <button
          type="button"
          className={`onboarding-mode-card ${chosen === 'ai-guided' ? 'is-selected' : ''}`}
          onClick={() => onChoose('ai-guided')}
          data-testid="onboarding-mode-ai"
        >
          <div className="onboarding-mode-icon onboarding-mode-icon-ai">
            <Brain size={28} />
          </div>
          <div className="onboarding-mode-badge">
            <Sparkles size={11} />
            {t('onboarding.mode_recommended')}
          </div>
          <h3 className="onboarding-mode-title">{t('onboarding.mode_ai_title')}</h3>
          <p className="onboarding-mode-desc">{t('onboarding.mode_ai_desc')}</p>
          <ul className="onboarding-mode-features">
            <li>
              <Check size={12} /> {t('onboarding.mode_ai_feat_1')}
            </li>
            <li>
              <Check size={12} /> {t('onboarding.mode_ai_feat_2')}
            </li>
            <li>
              <Check size={12} /> {t('onboarding.mode_ai_feat_3')}
            </li>
          </ul>
        </button>

        <button
          type="button"
          className={`onboarding-mode-card ${chosen === 'manual' ? 'is-selected' : ''}`}
          onClick={() => onChoose('manual')}
          data-testid="onboarding-mode-manual"
        >
          <div className="onboarding-mode-icon onboarding-mode-icon-manual">
            <Wrench size={28} />
          </div>
          <h3 className="onboarding-mode-title">{t('onboarding.mode_manual_title')}</h3>
          <p className="onboarding-mode-desc">{t('onboarding.mode_manual_desc')}</p>
          <ul className="onboarding-mode-features">
            <li>
              <Check size={12} /> {t('onboarding.mode_manual_feat_1')}
            </li>
            <li>
              <Check size={12} /> {t('onboarding.mode_manual_feat_2')}
            </li>
            <li>
              <Check size={12} /> {t('onboarding.mode_manual_feat_3')}
            </li>
          </ul>
        </button>
      </div>
    </div>
  );
}

function AiSetupStep({
  t,
  status,
  pullProgress,
  pullStarting,
  pullError,
  onInstallOllama,
  onStartPull,
  onRefresh,
}: StepProps & {
  status: GemmaStatus | null;
  pullProgress: GemmaPullProgress | null;
  pullStarting: boolean;
  pullError: string | null;
  onInstallOllama: () => void;
  onStartPull: () => void;
  onRefresh: () => void;
}) {
  const ollamaReady = status?.ollamaReachable ?? false;
  const modelReady = status?.modelInstalled ?? false;
  const allReady = status?.ready ?? false;
  const pulling = pullProgress?.inProgress ?? false;
  const pullPercent =
    pullProgress && pullProgress.totalBytes > 0
      ? Math.min(100, (pullProgress.completedBytes / pullProgress.totalBytes) * 100)
      : 0;

  return (
    <div className="onboarding-step">
      <h2 className="onboarding-step-title">{t('onboarding.ai_title')}</h2>
      <p className="onboarding-step-subtitle">{t('onboarding.ai_subtitle')}</p>

      <div className="onboarding-checklist">
        {/* Step 1 — Ollama */}
        <div className={`onboarding-check-row ${ollamaReady ? 'is-done' : 'is-active'}`}>
          <div className="onboarding-check-icon">
            {ollamaReady ? <Check size={16} /> : <span>1</span>}
          </div>
          <div className="onboarding-check-body">
            <div className="onboarding-check-title">{t('onboarding.ai_step1_title')}</div>
            <div className="onboarding-check-desc">{t('onboarding.ai_step1_desc')}</div>
            {!ollamaReady && (
              <div className="onboarding-check-actions">
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={onInstallOllama}
                  data-testid="onboarding-install-ollama"
                >
                  <ExternalLink size={14} />
                  {t('onboarding.ai_step1_button')}
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={onRefresh}
                  title={t('onboarding.ai_refresh')}
                >
                  <RefreshCw size={14} />
                </button>
              </div>
            )}
          </div>
        </div>

        {/* Step 2 — Gemma model */}
        <div
          className={`onboarding-check-row ${
            modelReady ? 'is-done' : ollamaReady ? 'is-active' : 'is-locked'
          }`}
        >
          <div className="onboarding-check-icon">
            {modelReady ? <Check size={16} /> : <span>2</span>}
          </div>
          <div className="onboarding-check-body">
            <div className="onboarding-check-title">{t('onboarding.ai_step2_title')}</div>
            <div className="onboarding-check-desc">{t('onboarding.ai_step2_desc')}</div>

            {ollamaReady && !modelReady && !pulling && (
              <div className="onboarding-check-actions">
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  onClick={onStartPull}
                  disabled={pullStarting}
                  data-testid="onboarding-pull-gemma"
                >
                  <Download size={14} />
                  {pullStarting
                    ? t('onboarding.ai_pull_starting')
                    : t('onboarding.ai_step2_button')}
                </button>
              </div>
            )}

            {pulling && (
              <div className="onboarding-pull-progress">
                <div className="onboarding-pull-status">
                  {pullProgress?.status || t('onboarding.ai_pull_starting')}
                </div>
                <div className="onboarding-pull-bar">
                  <div
                    className="onboarding-pull-bar-fill"
                    style={{ width: `${pullPercent.toFixed(1)}%` }}
                  />
                </div>
                {pullProgress && pullProgress.totalBytes > 0 && (
                  <div className="onboarding-pull-bytes">
                    {(pullProgress.completedBytes / 1024 / 1024).toFixed(0)} /{' '}
                    {(pullProgress.totalBytes / 1024 / 1024).toFixed(0)} MB
                    {' · '}
                    {pullPercent.toFixed(0)}%
                  </div>
                )}
              </div>
            )}

            {pullError && <div className="onboarding-pull-error">{pullError}</div>}
          </div>
        </div>
      </div>

      {allReady && (
        <div className="onboarding-success">
          <Check size={16} /> {t('onboarding.ai_ready')}
        </div>
      )}
    </div>
  );
}

function ReadyStep({ t, chosenMode }: StepProps & { chosenMode: 'ai-guided' | 'manual' | null }) {
  return (
    <div className="onboarding-step onboarding-step-ready">
      <div className="onboarding-success-orb">
        <Check size={48} color="white" />
      </div>
      <h2 className="onboarding-title">{t('onboarding.ready_title')}</h2>
      <p className="onboarding-tagline">
        {chosenMode === 'manual'
          ? t('onboarding.ready_manual_desc')
          : t('onboarding.ready_ai_desc')}
      </p>

      <div className="onboarding-ready-checklist">
        <div className="onboarding-ready-item">
          <Check size={14} /> {t('onboarding.ready_check_1')}
        </div>
        <div className="onboarding-ready-item">
          <Check size={14} /> {t('onboarding.ready_check_2')}
        </div>
        <div className="onboarding-ready-item">
          <Check size={14} /> {t('onboarding.ready_check_3')}
        </div>
      </div>
    </div>
  );
}
