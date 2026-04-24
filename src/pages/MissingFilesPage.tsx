import { Camera, Clock, FileSearch, Folder, RefreshCcw } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { EmptyState } from '../components/common/EmptyState';
import { LoadingState } from '../components/common/LoadingState';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import {
  computeFilesystemMemoryDiff,
  createFilesystemMemorySnapshot,
  fetchMissingFiles,
  listFilesystemMemorySnapshots,
} from '../hooks/ipc/filesystemMemory';
import { useAppStore } from '../stores/appStore';
import type {
  Confidence,
  DiffChange,
  DiffChangeKind,
  FilesystemSnapshot,
  MissingFileInsight,
  SnapshotDiff,
} from '../types';

function formatBytes(size: number): string {
  if (size <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const exp = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
  const value = size / 1024 ** exp;
  return `${value.toFixed(exp === 0 ? 0 : 1)} ${units[exp]}`;
}

function formatDate(ms: number | null | undefined, locale: string): string {
  if (!ms) return '—';
  return new Date(ms).toLocaleString(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

function confidenceBadgeVariant(confidence: Confidence): 'success' | 'warning' | 'info' {
  switch (confidence) {
    case 'high':
      return 'success';
    case 'medium':
      return 'info';
    default:
      return 'warning';
  }
}

function changeKindBadgeVariant(kind: DiffChangeKind): 'success' | 'warning' | 'info' {
  switch (kind) {
    case 'new':
    case 'moved':
    case 'renamed':
      return 'success';
    case 'missing':
    case 'ambiguous':
      return 'warning';
    default:
      return 'info';
  }
}

function changePathLabel(change: DiffChange): string {
  const from = change.from?.absolutePath ?? '—';
  const to = change.to?.absolutePath ?? '—';
  if (change.kind === 'missing') return from;
  if (change.kind === 'new') return to;
  return `${from} → ${to}`;
}

export function MissingFilesPage() {
  const { t, i18n } = useTranslation();
  const isExpert = useAppStore((state) => state.userMode === 'expert');
  const setFilesystemMemoryComparison = useAppStore((state) => state.setFilesystemMemoryComparison);

  const [snapshots, setSnapshots] = useState<FilesystemSnapshot[]>([]);
  const [listLoading, setListLoading] = useState(true);
  const [listError, setListError] = useState<string | null>(null);
  const [newPath, setNewPath] = useState('');
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [baselineId, setBaselineId] = useState<string>('');
  const [headId, setHeadId] = useState<string>('');
  const [insights, setInsights] = useState<MissingFileInsight[] | null>(null);
  const [diff, setDiff] = useState<SnapshotDiff | null>(null);
  const [insightsLoading, setInsightsLoading] = useState(false);
  const [insightsError, setInsightsError] = useState<string | null>(null);

  const reloadSnapshots = useCallback(async () => {
    setListLoading(true);
    setListError(null);
    try {
      const loaded = await listFilesystemMemorySnapshots();
      const sorted = [...loaded].sort((a, b) => b.capturedAtMs - a.capturedAtMs);
      setSnapshots(sorted);
    } catch (error) {
      setListError(error instanceof Error ? error.message : String(error));
    } finally {
      setListLoading(false);
    }
  }, []);

  useEffect(() => {
    void reloadSnapshots();
  }, [reloadSnapshots]);

  const uniqueTargets = useMemo(() => {
    const set = new Map<string, FilesystemSnapshot>();
    for (const snapshot of snapshots) {
      if (!set.has(snapshot.targetPath)) {
        set.set(snapshot.targetPath, snapshot);
      }
    }
    return Array.from(set.keys());
  }, [snapshots]);

  useEffect(() => {
    if (!selectedTarget && uniqueTargets.length > 0) {
      setSelectedTarget(uniqueTargets[0] ?? null);
    }
  }, [selectedTarget, uniqueTargets]);

  const snapshotsForTarget = useMemo(() => {
    if (!selectedTarget) return [];
    return snapshots
      .filter((snapshot) => snapshot.targetPath === selectedTarget)
      .sort((a, b) => a.capturedAtMs - b.capturedAtMs);
  }, [selectedTarget, snapshots]);

  useEffect(() => {
    if (snapshotsForTarget.length >= 2) {
      setBaselineId((current) =>
        current && snapshotsForTarget.some((s) => s.id === current)
          ? current
          : snapshotsForTarget[0].id,
      );
      setHeadId((current) =>
        current && snapshotsForTarget.some((s) => s.id === current)
          ? current
          : snapshotsForTarget[snapshotsForTarget.length - 1].id,
      );
    } else {
      setBaselineId('');
      setHeadId('');
      setInsights(null);
      setDiff(null);
    }
  }, [snapshotsForTarget]);

  const baselineSnapshot = useMemo(
    () => snapshotsForTarget.find((snapshot) => snapshot.id === baselineId) ?? null,
    [baselineId, snapshotsForTarget],
  );
  const headSnapshot = useMemo(
    () => snapshotsForTarget.find((snapshot) => snapshot.id === headId) ?? null,
    [headId, snapshotsForTarget],
  );

  const handleCreateSnapshot = useCallback(async () => {
    const trimmed = newPath.trim();
    if (!trimmed) {
      setCreateError(t('missingFiles.errors.empty_path'));
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      const snapshot = await createFilesystemMemorySnapshot(trimmed);
      setNewPath('');
      setSelectedTarget(snapshot.targetPath);
      await reloadSnapshots();
    } catch (error) {
      setCreateError(error instanceof Error ? error.message : String(error));
    } finally {
      setCreating(false);
    }
  }, [newPath, reloadSnapshots, t]);

  const handleCompareSnapshots = useCallback(async () => {
    if (!baselineId || !headId || baselineId === headId) {
      setInsightsError(t('missingFiles.errors.pick_two_snapshots'));
      return;
    }
    if (
      baselineSnapshot &&
      headSnapshot &&
      baselineSnapshot.capturedAtMs >= headSnapshot.capturedAtMs
    ) {
      setInsightsError(t('missingFiles.errors.pick_ordered_snapshots'));
      return;
    }
    setInsightsLoading(true);
    setInsightsError(null);
    try {
      const [computedDiff, missing] = await Promise.all([
        computeFilesystemMemoryDiff(baselineId, headId),
        fetchMissingFiles(baselineId, headId),
      ]);
      setDiff(computedDiff);
      setInsights(missing);
      setFilesystemMemoryComparison({
        baselineId: computedDiff.baselineId,
        headId: computedDiff.headId,
        targetPath: computedDiff.targetPath,
        comparedAtMs: computedDiff.computedAtMs,
      });
    } catch (error) {
      setInsightsError(error instanceof Error ? error.message : String(error));
      setInsights(null);
      setDiff(null);
    } finally {
      setInsightsLoading(false);
    }
  }, [baselineId, baselineSnapshot, headId, headSnapshot, setFilesystemMemoryComparison, t]);

  const locale = i18n.language === 'fr' ? 'fr-FR' : 'en-US';
  const changeCounts = useMemo(() => {
    const counts: Record<DiffChangeKind, number> = {
      new: 0,
      missing: 0,
      moved: 0,
      renamed: 0,
      modified: 0,
      ambiguous: 0,
    };
    for (const change of diff?.changes ?? []) {
      counts[change.kind] += 1;
    }
    return counts;
  }, [diff]);

  return (
    <div
      className="page missing-files-page"
      style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-5)' }}
    >
      <header>
        <h1 style={{ margin: 0, marginBottom: 'var(--space-2)' }}>{t('missingFiles.title')}</h1>
        <p className="text-sm text-secondary" style={{ margin: 0 }}>
          {t('missingFiles.subtitle')}
        </p>
      </header>

      <WarningBanner variant="info">{t('missingFiles.read_only_reminder')}</WarningBanner>

      <SectionCard
        title={t('missingFiles.capture_title')}
        subtitle={t('missingFiles.capture_subtitle')}
      >
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 'var(--space-3)',
            alignItems: 'flex-end',
          }}
        >
          <label
            style={{
              flex: 1,
              minWidth: 280,
              display: 'flex',
              flexDirection: 'column',
              gap: 'var(--space-1)',
            }}
          >
            <span className="text-xs text-secondary">{t('missingFiles.capture_path_label')}</span>
            <input
              type="text"
              className="input"
              placeholder={t('missingFiles.capture_path_placeholder')}
              value={newPath}
              onChange={(event) => setNewPath(event.target.value)}
              disabled={creating}
            />
          </label>
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleCreateSnapshot}
            disabled={creating || newPath.trim().length === 0}
          >
            <Camera size={14} />
            {creating ? t('missingFiles.capture_in_progress') : t('missingFiles.capture_action')}
          </button>
        </div>
        {createError && (
          <WarningBanner variant="danger">
            {t('missingFiles.errors.capture_failed', { message: createError })}
          </WarningBanner>
        )}
      </SectionCard>

      <SectionCard
        title={t('missingFiles.history_title')}
        subtitle={t('missingFiles.history_subtitle')}
        action={
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => void reloadSnapshots()}
            disabled={listLoading}
          >
            <RefreshCcw size={14} />
            {t('missingFiles.refresh')}
          </button>
        }
      >
        {listLoading ? (
          <LoadingState message={t('missingFiles.history_loading')} />
        ) : listError ? (
          <WarningBanner variant="danger">{listError}</WarningBanner>
        ) : snapshots.length === 0 ? (
          <EmptyState
            icon={<Folder size={24} />}
            title={t('missingFiles.history_empty_title')}
            description={t('missingFiles.history_empty_description')}
          />
        ) : (
          <>
            <label
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 'var(--space-1)',
                marginBottom: 'var(--space-3)',
              }}
            >
              <span className="text-xs text-secondary">{t('missingFiles.target_label')}</span>
              <select
                className="input"
                value={selectedTarget ?? ''}
                onChange={(event) => setSelectedTarget(event.target.value || null)}
              >
                {uniqueTargets.map((target) => (
                  <option key={target} value={target}>
                    {target}
                  </option>
                ))}
              </select>
            </label>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
                gap: 'var(--space-3)',
                marginBottom: 'var(--space-3)',
              }}
            >
              <label style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-1)' }}>
                <span className="text-xs text-secondary">{t('missingFiles.baseline_label')}</span>
                <select
                  className="input"
                  value={baselineId}
                  onChange={(event) => setBaselineId(event.target.value)}
                  disabled={snapshotsForTarget.length < 2}
                >
                  {snapshotsForTarget.map((snapshot) => (
                    <option key={`baseline-${snapshot.id}`} value={snapshot.id}>
                      {formatDate(snapshot.capturedAtMs, locale)} — {snapshot.filesIndexed}{' '}
                      {t('missingFiles.files_count')}
                    </option>
                  ))}
                </select>
              </label>
              <label style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-1)' }}>
                <span className="text-xs text-secondary">{t('missingFiles.head_label')}</span>
                <select
                  className="input"
                  value={headId}
                  onChange={(event) => setHeadId(event.target.value)}
                  disabled={snapshotsForTarget.length < 2}
                >
                  {snapshotsForTarget.map((snapshot) => (
                    <option key={`head-${snapshot.id}`} value={snapshot.id}>
                      {formatDate(snapshot.capturedAtMs, locale)} — {snapshot.filesIndexed}{' '}
                      {t('missingFiles.files_count')}
                    </option>
                  ))}
                </select>
              </label>
            </div>

            <button
              type="button"
              className="btn btn-primary"
              onClick={handleCompareSnapshots}
              disabled={insightsLoading || !baselineId || !headId || baselineId === headId}
            >
              <FileSearch size={14} />
              {insightsLoading
                ? t('missingFiles.compare_in_progress')
                : t('missingFiles.compare_action')}
            </button>
          </>
        )}
      </SectionCard>

      {insightsError && <WarningBanner variant="danger">{insightsError}</WarningBanner>}

      {baselineSnapshot?.status === 'partial' && (
        <WarningBanner variant="warning">
          {t('missingFiles.baseline_partial_warning', {
            count: baselineSnapshot.errors.length,
          })}
        </WarningBanner>
      )}

      {headSnapshot?.status === 'partial' && (
        <WarningBanner variant="warning">
          {t('missingFiles.head_partial_warning', {
            count: headSnapshot.errors.length,
          })}
        </WarningBanner>
      )}

      {diff && (
        <SectionCard
          title={t('missingFiles.changes_title')}
          subtitle={t('missingFiles.changes_subtitle', { count: diff.changes.length })}
        >
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))',
              gap: 'var(--space-3)',
              marginBottom: isExpert ? 'var(--space-4)' : 0,
            }}
          >
            {(
              [
                ['new', changeCounts.new],
                ['missing', changeCounts.missing],
                ['moved', changeCounts.moved],
                ['renamed', changeCounts.renamed],
                ['modified', changeCounts.modified],
                ['ambiguous', changeCounts.ambiguous],
              ] as Array<[DiffChangeKind, number]>
            ).map(([kind, count]) => (
              <div
                key={kind}
                style={{
                  padding: 'var(--space-3)',
                  borderRadius: 'var(--radius-lg)',
                  background: 'var(--color-bg-secondary)',
                  border: '1px solid var(--color-border)',
                }}
              >
                <div className="text-xs text-secondary">
                  {t(`missingFiles.change_kind.${kind}`)}
                </div>
                <div className="font-medium" style={{ fontSize: '1.1rem' }}>
                  {count}
                </div>
              </div>
            ))}
          </div>

          {isExpert && diff.changes.length > 0 && (
            <ul
              style={{
                listStyle: 'none',
                padding: 0,
                margin: 0,
                display: 'flex',
                flexDirection: 'column',
                gap: 'var(--space-3)',
              }}
            >
              {diff.changes.map((change, index) => (
                <li
                  key={`${change.kind}-${change.from?.absolutePath ?? 'from'}-${change.to?.absolutePath ?? 'to'}-${index}`}
                  style={{
                    padding: 'var(--space-4)',
                    borderRadius: 'var(--radius-lg)',
                    background: 'var(--color-bg-secondary)',
                    border: '1px solid var(--color-border)',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 'var(--space-2)',
                      flexWrap: 'wrap',
                    }}
                  >
                    <span className={`badge-${changeKindBadgeVariant(change.kind)}`}>
                      {t(`missingFiles.change_kind.${change.kind}`)}
                    </span>
                    <span className={`badge-${confidenceBadgeVariant(change.confidence)}`}>
                      {t(`missingFiles.confidence.${change.confidence}`)}
                    </span>
                  </div>
                  <div className="text-sm" style={{ marginTop: 'var(--space-2)' }}>
                    {changePathLabel(change)}
                  </div>
                  <div className="text-xs text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                    {change.reason}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      )}

      {insights !== null && (
        <SectionCard
          title={t('missingFiles.results_title')}
          subtitle={t('missingFiles.results_subtitle', { count: insights.length })}
        >
          {insights.length === 0 ? (
            <EmptyState
              icon={<Clock size={24} />}
              title={t('missingFiles.results_empty_title')}
              description={t('missingFiles.results_empty_description')}
            />
          ) : (
            <ul
              style={{
                listStyle: 'none',
                padding: 0,
                margin: 0,
                display: 'flex',
                flexDirection: 'column',
                gap: 'var(--space-3)',
              }}
            >
              {insights.map((insight) => (
                <li
                  key={`${insight.lastKnownPath}-${insight.firstMissingObservedAtMs}`}
                  style={{
                    padding: 'var(--space-4)',
                    borderRadius: 'var(--radius-lg)',
                    background: 'var(--color-bg-secondary)',
                    border: '1px solid var(--color-border)',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 'var(--space-2)',
                      flexWrap: 'wrap',
                    }}
                  >
                    <span className="font-medium">{insight.name}</span>
                    <span className={`badge-${confidenceBadgeVariant(insight.confidence)}`}>
                      {t(`missingFiles.confidence.${insight.confidence}`)}
                    </span>
                    <span className="text-xs text-secondary">{formatBytes(insight.sizeBytes)}</span>
                  </div>
                  <div className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                    {t('missingFiles.last_known_label')}: {insight.lastKnownPath}
                  </div>
                  <div className="text-sm text-secondary">
                    {t('missingFiles.last_observed_label')}:{' '}
                    {formatDate(insight.lastObservedAtMs, locale)}
                  </div>
                  <div className="text-sm text-secondary">
                    {t('missingFiles.first_missing_label')}:{' '}
                    {formatDate(insight.firstMissingObservedAtMs, locale)}
                  </div>
                  {insight.fileModifiedAtMs !== null && (
                    <div className="text-sm text-secondary">
                      {t('missingFiles.file_modified_label')}:{' '}
                      {formatDate(insight.fileModifiedAtMs, locale)}
                    </div>
                  )}
                  {isExpert && (
                    <div className="text-xs text-secondary" style={{ marginTop: 'var(--space-2)' }}>
                      {insight.recoveryHint}
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      )}

      {isExpert && (baselineSnapshot || headSnapshot) && (
        <SectionCard
          title={t('missingFiles.expert_snapshot_title')}
          subtitle={t('missingFiles.expert_snapshot_subtitle')}
        >
          <div
            className="text-sm text-secondary"
            style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-2)' }}
          >
            {baselineSnapshot && (
              <span>
                {t('missingFiles.baseline_label')}: {baselineSnapshot.id} ·{' '}
                {t('missingFiles.volume_fingerprint_label')}:{' '}
                {baselineSnapshot.volumeFingerprint ?? '—'}
              </span>
            )}
            {headSnapshot && (
              <span>
                {t('missingFiles.head_label')}: {headSnapshot.id} ·{' '}
                {t('missingFiles.volume_fingerprint_label')}:{' '}
                {headSnapshot.volumeFingerprint ?? '—'}
              </span>
            )}
          </div>
        </SectionCard>
      )}
    </div>
  );
}
