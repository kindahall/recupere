import { Brain, Cloud, FileSearch, FileText, Filter, Sparkles, Target, Wrench } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import {
  type FileClassification,
  type FileSearchHit,
  type GemmaStatus,
  type NarrativeReport,
  type ReconstructionStrategy,
  type RecoveryPrediction,
  classifyScanFiles,
  fetchGemmaAnalysisProgress,
  fetchGemmaStatus,
  generateNarrativeReport,
  predictScanRecovery,
  searchFileByName,
  smartSelectByCategory,
  startGemmaAnalysis,
  suggestFileReconstruction,
} from '../../hooks/useIpc';
import { useAppStore } from '../../stores/appStore';
import type { RecoveredFile } from '../../types';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';

interface AiAnalysisPanelProps {
  scanId: string;
  focusFile: RecoveredFile | null;
  onSelectFiles: (fileIds: string[]) => void;
}

const CATEGORY_BUTTONS = [
  { key: 'photos', icon: '📷' },
  { key: 'documents', icon: '📄' },
  { key: 'videos', icon: '🎬' },
  { key: 'music', icon: '🎵' },
  { key: 'code', icon: '💻' },
  { key: 'email', icon: '📧' },
  { key: 'all', icon: '📁' },
];

export function AiAnalysisPanel({ scanId, focusFile, onSelectFiles }: AiAnalysisPanelProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const licenseStatus = useAppStore((s) => s.licenseStatus);
  const isPro = licenseStatus === 'pro';
  const [selectionNotice, setSelectionNotice] = useState<{
    count: number;
    category: string;
  } | null>(null);
  const [classifications, setClassifications] = useState<FileClassification[] | null>(null);
  const [predictions, setPredictions] = useState<RecoveryPrediction[] | null>(null);
  const [narrative, setNarrative] = useState<NarrativeReport | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<FileSearchHit[] | null>(null);
  const [reconstruction, setReconstruction] = useState<ReconstructionStrategy | null>(null);
  const [cloudResponse, setCloudResponse] = useState<string | null>(null);
  const [streamingOutput, setStreamingOutput] = useState('');
  const [streamingTokens, setStreamingTokens] = useState(0);
  const [localElapsed, setLocalElapsed] = useState(0);
  const [loadingPhraseIndex, setLoadingPhraseIndex] = useState(0);
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [gemmaStatus, setGemmaStatus] = useState<GemmaStatus | null>(null);
  const analysisPollRef = useRef<number | null>(null);
  const localTickRef = useRef<number | null>(null);
  const phraseTickRef = useRef<number | null>(null);
  const categoryLabel = (categoryKey: string) => t(`results.ai_category_${categoryKey}`);
  const focusFileId = focusFile?.id ?? null;

  useEffect(() => {
    setReconstruction((current) => (current?.fileId === focusFileId ? current : null));
  }, [focusFileId]);

  const LOADING_PHRASE_KEYS = [
    'results.ai_gemma_phrase_1',
    'results.ai_gemma_phrase_2',
    'results.ai_gemma_phrase_3',
    'results.ai_gemma_phrase_4',
    'results.ai_gemma_phrase_5',
    'results.ai_gemma_phrase_6',
    'results.ai_gemma_phrase_7',
  ];

  const stopAllTimers = useCallback(() => {
    if (analysisPollRef.current !== null) {
      window.clearInterval(analysisPollRef.current);
      analysisPollRef.current = null;
    }
    if (localTickRef.current !== null) {
      window.clearInterval(localTickRef.current);
      localTickRef.current = null;
    }
    if (phraseTickRef.current !== null) {
      window.clearInterval(phraseTickRef.current);
      phraseTickRef.current = null;
    }
  }, []);

  useEffect(() => () => stopAllTimers(), [stopAllTimers]);

  useEffect(() => {
    let cancelled = false;
    fetchGemmaStatus()
      .then((s) => {
        if (!cancelled) setGemmaStatus(s);
      })
      .catch(() => {
        if (!cancelled) setGemmaStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const gemmaReady = gemmaStatus?.ready ?? false;

  const runClassification = async () => {
    setLoading('classify');
    setError(null);
    try {
      const result = await classifyScanFiles(scanId);
      setClassifications(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Classification failed');
    } finally {
      setLoading(null);
    }
  };

  const runPrediction = async () => {
    setLoading('predict');
    setError(null);
    try {
      const result = await predictScanRecovery(scanId);
      setPredictions(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Prediction failed');
    } finally {
      setLoading(null);
    }
  };

  const runNarrative = async () => {
    setLoading('narrative');
    setError(null);
    try {
      const result = await generateNarrativeReport(scanId);
      setNarrative(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Report generation failed');
    } finally {
      setLoading(null);
    }
  };

  const runSearch = async () => {
    const normalizedQuery = searchQuery.trim();
    if (normalizedQuery.length === 0) {
      setSearchResults([]);
      return;
    }
    setLoading('search');
    setError(null);
    try {
      const results = await searchFileByName(scanId, normalizedQuery);
      setSearchResults(results);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Search failed');
    } finally {
      setLoading(null);
    }
  };

  const runReconstruction = async () => {
    if (!focusFile) {
      return;
    }
    setLoading('reconstruct');
    setError(null);
    try {
      const result = await suggestFileReconstruction(scanId, focusFile.id);
      setReconstruction(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Reconstruction suggestion failed');
    } finally {
      setLoading(null);
    }
  };

  const runCloudAnalysis = async () => {
    setLoading('cloud');
    setError(null);
    setCloudResponse(null);
    setStreamingOutput('');
    setStreamingTokens(0);
    setLocalElapsed(0);
    setLoadingPhraseIndex(0);
    try {
      await startGemmaAnalysis(scanId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setLoading(null);
      return;
    }
    stopAllTimers();
    // Local 1s tick — independent of backend polling, so the elapsed counter
    // moves even while the model is loading and no tokens are coming back yet.
    const startedAt = Date.now();
    localTickRef.current = window.setInterval(() => {
      setLocalElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);
    // Rotate reassuring phrases every 5s during the loading phase.
    phraseTickRef.current = window.setInterval(() => {
      setLoadingPhraseIndex((i) => (i + 1) % LOADING_PHRASE_KEYS.length);
    }, 5000);
    analysisPollRef.current = window.setInterval(async () => {
      try {
        const p = await fetchGemmaAnalysisProgress();
        setStreamingOutput(p.output);
        setStreamingTokens(p.tokensGenerated);
        if (p.finished) {
          stopAllTimers();
          if (p.error) {
            setError(p.error);
          } else {
            setCloudResponse(p.output);
          }
          setLoading(null);
        }
      } catch {
        // transient poll error: keep polling
      }
    }, 500);
  };

  const selectCategory = async (category: string) => {
    setLoading('select');
    setError(null);
    setSelectionNotice(null);
    try {
      const fileIds = await smartSelectByCategory(scanId, category);
      onSelectFiles(fileIds);
      setSelectionNotice({ count: fileIds.length, category });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Selection failed');
    } finally {
      setLoading(null);
    }
  };

  // Compute category stats from classifications
  const categoryStats = classifications
    ? Object.entries(
        classifications.reduce<Record<string, number>>((acc, c) => {
          acc[c.category] = (acc[c.category] || 0) + 1;
          return acc;
        }, {}),
      ).sort((a, b) => b[1] - a[1])
    : null;

  const importanceStats = classifications
    ? {
        critical: classifications.filter((c) => c.importance === 'critical').length,
        high: classifications.filter((c) => c.importance === 'high').length,
        medium: classifications.filter((c) => c.importance === 'medium').length,
        low: classifications.filter((c) => c.importance === 'low').length,
      }
    : null;

  return (
    <div className="flex flex-col gap-4">
      {!gemmaReady && gemmaStatus && (
        <WarningBanner variant="warning">
          {t('settings.gemma_unavailable_banner')} {gemmaStatus.message}
        </WarningBanner>
      )}
      {/* Smart Selection Bar */}
      <SectionCard
        title={t('results.ai_smart_select')}
        action={
          <div className="flex items-center gap-1">
            <Filter size={14} />
            <span className="text-xs text-secondary">{t('results.ai_select_category')}</span>
          </div>
        }
      >
        <div className="flex flex-wrap gap-2">
          {CATEGORY_BUTTONS.map((cat) => {
            const isActive = selectionNotice?.category === cat.key;
            const isLoading = loading === 'select';
            return (
              <button
                type="button"
                key={cat.key}
                className={`btn btn-sm ${isActive ? 'btn-primary' : 'btn-secondary'}`}
                onClick={() => selectCategory(cat.key)}
                disabled={isLoading}
                style={
                  isActive
                    ? {
                        boxShadow:
                          '0 0 0 2px rgba(99, 102, 241, 0.45), 0 8px 24px -8px rgba(99, 102, 241, 0.55)',
                      }
                    : undefined
                }
              >
                <span>{cat.icon}</span>
                {categoryLabel(cat.key)}
                {isActive && selectionNotice && (
                  <span
                    style={{
                      marginLeft: 'var(--space-1)',
                      padding: '0 6px',
                      borderRadius: 'var(--radius-full)',
                      background: 'rgba(255,255,255,0.2)',
                      fontSize: 'var(--font-size-xs)',
                      fontWeight: 600,
                    }}
                  >
                    {selectionNotice.count}
                  </span>
                )}
              </button>
            );
          })}
        </div>
        {selectionNotice && (
          <div style={{ marginTop: 'var(--space-3)' }}>
            {selectionNotice.count === 0 ? (
              <WarningBanner variant="warning">
                {t('results.ai_smart_select_empty', {
                  category: categoryLabel(selectionNotice.category),
                })}
              </WarningBanner>
            ) : isPro ? (
              <WarningBanner variant="success">
                {t('results.ai_smart_select_done_pro', { count: selectionNotice.count })}{' '}
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  style={{ marginLeft: 'var(--space-2)' }}
                  onClick={() => navigate('/export')}
                >
                  {t('results.ai_smart_select_go_export')}
                </button>
              </WarningBanner>
            ) : (
              <WarningBanner variant="info">
                {t('results.ai_smart_select_done_free', { count: selectionNotice.count })}{' '}
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  style={{ marginLeft: 'var(--space-2)' }}
                  onClick={() => navigate('/export')}
                >
                  {t('results.ai_smart_select_unlock')}
                </button>
              </WarningBanner>
            )}
          </div>
        )}
      </SectionCard>

      <SectionCard
        title={t('results.ai_investigate_files_title')}
        subtitle={t('results.ai_investigate_files_subtitle')}
      >
        <div className="flex flex-col gap-3">
          <div className="flex gap-2">
            <input
              type="text"
              className="input input-sm"
              style={{ width: '100%' }}
              placeholder={t('results.ai_search_placeholder')}
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void runSearch();
                }
              }}
            />
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={runSearch}
              disabled={loading !== null}
            >
              <FileSearch size={14} />
              {loading === 'search' ? t('common.loading') : t('results.ai_search_action')}
            </button>
          </div>

          {searchResults !== null && (
            <div className="flex flex-col gap-2">
              {searchResults.length === 0 ? (
                <div className="text-sm text-secondary">{t('results.ai_search_empty')}</div>
              ) : (
                searchResults.slice(0, 8).map((hit) => (
                  <button
                    type="button"
                    key={hit.id}
                    className="stat-card"
                    style={{ textAlign: 'left', cursor: 'pointer' }}
                    onClick={() => onSelectFiles([hit.id])}
                  >
                    <span className="stat-card-label">
                      {hit.name}
                      {hit.isDeleted ? ` · ${t('results.deleted_badge')}` : ''}
                    </span>
                    <span
                      className="text-sm text-secondary"
                      style={{ marginTop: 'var(--space-1)' }}
                    >
                      {hit.path}
                    </span>
                    <span
                      className="text-xs text-secondary"
                      style={{ marginTop: 'var(--space-1)' }}
                    >
                      {t('results.ai_search_hit_meta', {
                        size: hit.sizeBytes.toLocaleString(),
                        score: hit.recoveryScore,
                        integrity: hit.integrity,
                      })}
                    </span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>
      </SectionCard>

      <SectionCard
        title={t('results.ai_reconstruction_title')}
        subtitle={
          focusFile
            ? t('results.ai_reconstruction_subtitle', { name: focusFile.name })
            : t('results.ai_reconstruction_no_file')
        }
      >
        <div className="flex flex-col gap-3">
          {focusFile && (
            <div className="text-sm text-secondary">
              <strong>{focusFile.name}</strong>
              <div>{focusFile.path}</div>
            </div>
          )}
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            onClick={runReconstruction}
            disabled={loading !== null || focusFile === null}
          >
            <Wrench size={14} />
            {loading === 'reconstruct'
              ? t('common.loading')
              : t('results.ai_reconstruction_action')}
          </button>

          {reconstruction && (
            <div
              style={{
                border: '1px solid var(--color-border)',
                borderRadius: 'var(--radius-md)',
                padding: 'var(--space-3)',
                background: 'var(--color-bg-secondary)',
              }}
            >
              <div className="text-sm font-medium" style={{ marginBottom: 'var(--space-2)' }}>
                {reconstruction.strategy}
              </div>
              <div className="text-xs text-secondary" style={{ marginBottom: 'var(--space-3)' }}>
                {t('results.ai_reconstruction_success_estimate', {
                  percent: Math.round(reconstruction.estimatedSuccess * 100),
                })}
              </div>
              <div className="flex flex-col gap-2">
                {reconstruction.steps.map((step, index) => (
                  <div
                    key={`${reconstruction.fileId}-step-${index}`}
                    className="text-sm text-secondary"
                  >
                    {index + 1}. {step}
                  </div>
                ))}
              </div>
              {reconstruction.alternativeStrategies.length > 0 && (
                <div style={{ marginTop: 'var(--space-3)' }}>
                  <div className="text-sm font-medium" style={{ marginBottom: 'var(--space-2)' }}>
                    {t('results.ai_reconstruction_alternatives')}
                  </div>
                  <div className="flex flex-col gap-2">
                    {reconstruction.alternativeStrategies.map((strategy, index) => (
                      <div
                        key={`${reconstruction.fileId}-alternative-${index}`}
                        className="text-sm text-secondary"
                      >
                        {strategy}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </SectionCard>

      {/* AI Analysis Actions */}
      <div className="flex gap-2">
        <button
          type="button"
          className="btn btn-primary btn-sm"
          onClick={runClassification}
          disabled={loading !== null}
        >
          <Brain size={14} />
          {loading === 'classify' ? t('common.loading') : t('results.ai_classify')}
        </button>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={runPrediction}
          disabled={loading !== null}
        >
          <Target size={14} />
          {loading === 'predict' ? t('common.loading') : t('results.ai_predict')}
        </button>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={runNarrative}
          disabled={loading !== null}
        >
          <FileText size={14} />
          {loading === 'narrative' ? t('common.loading') : t('results.ai_narrative')}
        </button>
        <button
          type="button"
          className="btn btn-primary btn-sm"
          onClick={runCloudAnalysis}
          disabled={loading !== null || !gemmaReady}
          title={!gemmaReady ? t('settings.gemma_unavailable_banner') : undefined}
        >
          <Cloud size={14} />
          {loading === 'cloud'
            ? streamingTokens === 0
              ? t('results.ai_gemma_loading', { seconds: localElapsed })
              : t('results.ai_gemma_generating', { tokens: streamingTokens, seconds: localElapsed })
            : t('results.ai_gemma_button')}
        </button>
      </div>

      {loading === 'cloud' && (
        <SectionCard
          title={
            streamingTokens === 0
              ? t('results.ai_gemma_loading_title')
              : t('results.ai_gemma_thinking_title')
          }
        >
          <style>{`
            @keyframes recupereGemmaPulse {
              0%, 100% { opacity: 0.4; transform: scale(1); }
              50%      { opacity: 1;   transform: scale(1.15); }
            }
            @keyframes recupereGemmaShimmer {
              0%   { background-position: -200% 0; }
              100% { background-position: 200% 0; }
            }
          `}</style>

          <div className="flex items-center gap-3 mb-3">
            <div
              style={{
                width: 12,
                height: 12,
                borderRadius: '50%',
                background: streamingTokens === 0 ? 'var(--color-warning)' : 'var(--color-success)',
                animation: 'recupereGemmaPulse 1.2s ease-in-out infinite',
              }}
            />
            <div className="text-sm font-medium">
              {streamingTokens === 0
                ? t(LOADING_PHRASE_KEYS[loadingPhraseIndex])
                : t('results.ai_gemma_tokens_count', { tokens: streamingTokens })}
            </div>
            <div className="text-xs text-secondary" style={{ marginLeft: 'auto' }}>
              {t('results.ai_gemma_elapsed', { seconds: localElapsed })}
            </div>
          </div>

          {streamingTokens === 0 && (
            <div
              style={{
                height: 4,
                borderRadius: 2,
                background:
                  'linear-gradient(90deg, var(--color-bg-secondary) 0%, var(--color-accent) 50%, var(--color-bg-secondary) 100%)',
                backgroundSize: '200% 100%',
                animation: 'recupereGemmaShimmer 1.8s linear infinite',
                marginBottom: 'var(--space-3)',
              }}
            />
          )}

          {streamingOutput && (
            <pre
              style={{
                whiteSpace: 'pre-wrap',
                fontFamily: 'var(--font-mono)',
                fontSize: 'var(--font-size-sm)',
                maxHeight: 400,
                overflowY: 'auto',
                padding: 'var(--space-3)',
                background: 'var(--color-bg-secondary)',
                borderRadius: 'var(--radius-md)',
                margin: 0,
              }}
            >
              {streamingOutput}
            </pre>
          )}
        </SectionCard>
      )}

      {error && <WarningBanner variant="danger">{error}</WarningBanner>}

      {/* Classification Results */}
      {categoryStats && (
        <SectionCard title={t('results.ai_classification_results')}>
          <div className="grid grid-3 gap-3">
            {categoryStats.slice(0, 9).map(([cat, count]) => (
              <button
                type="button"
                key={cat}
                className="stat-card"
                style={{ cursor: 'pointer' }}
                onClick={() => selectCategory(cat)}
              >
                <span className="stat-card-label">{cat.replace(/_/g, ' ')}</span>
                <span className="stat-card-value">{count}</span>
              </button>
            ))}
          </div>
          {importanceStats && (
            <div className="flex gap-4 mt-4">
              <div className="flex items-center gap-2">
                <span className="health-badge-dot" style={{ background: 'var(--color-danger)' }} />
                <span className="text-sm">
                  {t('results.ai_critical')}: {importanceStats.critical}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="health-badge-dot" style={{ background: 'var(--color-warning)' }} />
                <span className="text-sm">
                  {t('results.ai_high')}: {importanceStats.high}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="health-badge-dot" style={{ background: 'var(--color-info)' }} />
                <span className="text-sm">
                  {t('results.ai_medium')}: {importanceStats.medium}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span
                  className="health-badge-dot"
                  style={{ background: 'var(--color-text-secondary)' }}
                />
                <span className="text-sm">
                  {t('results.ai_low')}: {importanceStats.low}
                </span>
              </div>
            </div>
          )}
        </SectionCard>
      )}

      {/* Prediction Results */}
      {predictions && (
        <SectionCard title={t('results.ai_prediction_results')}>
          <div className="grid grid-4 gap-3">
            <div className="stat-card">
              <span className="stat-card-label">{t('results.ai_pred_perfect')}</span>
              <span className="stat-card-value text-success">
                {predictions.filter((p) => p.estimatedQuality === 'perfect').length}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('results.ai_pred_good')}</span>
              <span className="stat-card-value" style={{ color: 'var(--color-info)' }}>
                {predictions.filter((p) => p.estimatedQuality === 'good').length}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('results.ai_pred_partial')}</span>
              <span className="stat-card-value text-warning">
                {predictions.filter((p) => p.estimatedQuality === 'partial').length}
              </span>
            </div>
            <div className="stat-card">
              <span className="stat-card-label">{t('results.ai_pred_poor')}</span>
              <span className="stat-card-value text-danger">
                {predictions.filter((p) => p.estimatedQuality === 'poor').length}
              </span>
            </div>
          </div>
          <div className="text-sm text-secondary mt-3">
            {t('results.ai_avg_success')}:{' '}
            {(
              (predictions.reduce((sum, p) => sum + p.successProbability, 0) / predictions.length) *
              100
            ).toFixed(0)}
            %
          </div>
        </SectionCard>
      )}

      {/* Narrative Report */}
      {narrative && (
        <SectionCard title={narrative.title}>
          <div className="flex flex-col gap-4">
            <div className="text-md font-medium" style={{ color: 'var(--color-accent)' }}>
              {narrative.summary}
            </div>

            <div>
              <div className="text-sm font-medium mb-2">{t('results.ai_situation')}</div>
              <p className="text-sm text-secondary" style={{ margin: 0 }}>
                {narrative.situationAnalysis}
              </p>
            </div>

            <div>
              <div className="text-sm font-medium mb-2">{t('results.ai_findings')}</div>
              {narrative.keyFindings.map((f, i) => (
                <div key={i} className="text-sm text-secondary flex items-start gap-2">
                  <Sparkles
                    size={12}
                    className="text-accent"
                    style={{ marginTop: 3, flexShrink: 0 }}
                  />
                  {f}
                </div>
              ))}
            </div>

            <div>
              <div className="text-sm font-medium mb-2">{t('results.ai_strategy')}</div>
              <p className="text-sm text-secondary" style={{ margin: 0 }}>
                {narrative.recoveryStrategy}
              </p>
            </div>

            {narrative.priorityFiles.length > 0 && (
              <div>
                <div className="text-sm font-medium mb-2">{t('results.ai_priority_files')}</div>
                {narrative.priorityFiles.map((f, i) => (
                  <div key={i} className="text-sm text-secondary">
                    {i + 1}. {f}
                  </div>
                ))}
              </div>
            )}

            {narrative.warnings.length > 0 && (
              <WarningBanner variant="warning">{narrative.warnings.join(' ')}</WarningBanner>
            )}

            <div className="text-sm text-secondary" style={{ fontStyle: 'italic' }}>
              {narrative.conclusion}
            </div>
          </div>
        </SectionCard>
      )}

      {/* Cloud AI Response */}
      {cloudResponse && (
        <SectionCard title={t('results.ai_cloud_title')}>
          <pre
            className="text-sm text-secondary"
            style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word', margin: 0 }}
          >
            {cloudResponse}
          </pre>
        </SectionCard>
      )}
    </div>
  );
}
