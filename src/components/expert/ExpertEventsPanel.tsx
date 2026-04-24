import { Download } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { RuntimeCapabilities } from '../../types';
import type {
  TechnicalTimelineEntry,
  TechnicalTimelineSeverityFilter,
  TechnicalTimelineSourceFilter,
} from '../../utils/technicalTimeline';
import { ErrorState } from '../common/ErrorState';
import { SectionCard } from '../common/SectionCard';
import { WarningBanner } from '../common/WarningBanner';
import { TechnicalTimelinePanel } from '../scan/TechnicalTimelinePanel';

export interface ExpertEventsPanelProps {
  runtimeCapabilities: RuntimeCapabilities | null;
  hasActiveTechnicalSession: boolean;
  activeScanId: string | null;
  activeExportId: string | null;
  scanLogsError: string | null;
  exportLogsError: string | null;
  searchQuery: string;
  setSearchQuery: (v: string) => void;
  sourceFilter: TechnicalTimelineSourceFilter;
  setSourceFilter: (v: TechnicalTimelineSourceFilter) => void;
  severityFilter: TechnicalTimelineSeverityFilter;
  setSeverityFilter: (v: TechnicalTimelineSeverityFilter) => void;
  combinedTimeline: TechnicalTimelineEntry[];
  filteredTimeline: TechnicalTimelineEntry[];
  timelineExporting: boolean;
  timelineExportNotice: { variant: 'success' | 'warning' | 'danger'; message: string } | null;
  onExportFilteredTimeline: () => void;
}

export function ExpertEventsPanel(props: ExpertEventsPanelProps) {
  const { t } = useTranslation();
  const {
    runtimeCapabilities,
    hasActiveTechnicalSession,
    activeScanId,
    activeExportId,
    scanLogsError,
    exportLogsError,
    searchQuery,
    setSearchQuery,
    sourceFilter,
    setSourceFilter,
    severityFilter,
    setSeverityFilter,
    combinedTimeline,
    filteredTimeline,
    timelineExporting,
    timelineExportNotice,
    onExportFilteredTimeline,
  } = props;

  return (
    <details>
      <summary className="text-sm font-semibold mb-2" style={{ cursor: 'pointer' }}>
        {t('expert.events')}
      </summary>
      <SectionCard title={t('expert.events')}>
        {runtimeCapabilities?.technicalLogs ? (
          hasActiveTechnicalSession ? (
            <div className="flex flex-col gap-4">
              {timelineExportNotice && (
                <WarningBanner variant={timelineExportNotice.variant}>
                  {timelineExportNotice.message}
                </WarningBanner>
              )}

              <div className="flex flex-wrap gap-2">
                {activeScanId && (
                  <span className="badge-device">
                    {t('expert.active_scan')}: {activeScanId}
                  </span>
                )}
                {activeExportId && (
                  <span className="badge-device">
                    {t('expert.active_export')}: {activeExportId}
                  </span>
                )}
              </div>

              {(scanLogsError || exportLogsError) && (
                <div className="flex flex-col gap-3">
                  {scanLogsError && (
                    <ErrorState title={t('common.error')} description={scanLogsError} />
                  )}
                  {exportLogsError && (
                    <ErrorState title={t('common.error')} description={exportLogsError} />
                  )}
                </div>
              )}

              <div className="flex flex-col gap-3">
                <input
                  type="text"
                  className="btn btn-secondary w-full"
                  style={{
                    justifyContent: 'flex-start',
                    textAlign: 'left',
                    fontFamily: 'var(--font-mono)',
                    fontSize: 'var(--font-size-sm)',
                  }}
                  placeholder={t('expert.search_placeholder')}
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                />

                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className={`btn ${sourceFilter === 'all' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSourceFilter('all')}
                  >
                    {t('expert.filter_all')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${sourceFilter === 'scan' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSourceFilter('scan')}
                  >
                    {t('expert.source_scan')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${sourceFilter === 'export' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSourceFilter('export')}
                  >
                    {t('expert.source_export')}
                  </button>
                </div>

                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className={`btn ${severityFilter === 'all' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSeverityFilter('all')}
                  >
                    {t('expert.filter_all')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${severityFilter === 'info' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSeverityFilter('info')}
                  >
                    {t('expert.filter_info')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${severityFilter === 'warning' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSeverityFilter('warning')}
                  >
                    {t('expert.filter_warning')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${severityFilter === 'error' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSeverityFilter('error')}
                  >
                    {t('expert.filter_errors')}
                  </button>
                  <button
                    type="button"
                    className={`btn ${severityFilter === 'debug' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                    onClick={() => setSeverityFilter('debug')}
                  >
                    {t('expert.filter_debug')}
                  </button>
                </div>
              </div>

              <div className="flex items-center justify-between gap-3">
                <div className="text-sm text-secondary">
                  {t('expert.timeline_count', {
                    visible: filteredTimeline.length,
                    total: combinedTimeline.length,
                  })}
                </div>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={onExportFilteredTimeline}
                  disabled={filteredTimeline.length === 0 || timelineExporting}
                >
                  <Download size={14} />
                  {timelineExporting
                    ? t('expert.export_filtered_log_saving')
                    : t('expert.export_filtered_log')}
                </button>
              </div>

              <div className="text-sm font-medium">{t('expert.events_timeline')}</div>
              <TechnicalTimelinePanel
                entries={filteredTimeline}
                emptyMessage={
                  combinedTimeline.length === 0
                    ? t('expert.events_empty')
                    : t('expert.events_filtered_empty')
                }
                sourceLabel={(source) =>
                  source === 'scan' ? t('expert.source_scan') : t('expert.source_export')
                }
              />
            </div>
          ) : (
            <TechnicalTimelinePanel
              entries={[]}
              emptyMessage={t('expert.events_empty')}
              sourceLabel={(source) =>
                source === 'scan' ? t('expert.source_scan') : t('expert.source_export')
              }
            />
          )
        ) : (
          <div className="scan-log-panel">
            <div
              className="text-muted"
              style={{
                textAlign: 'center',
                padding: 'var(--space-4) 0',
                fontFamily: 'var(--font-mono)',
                fontSize: 'var(--font-size-xs)',
              }}
            >
              {t('expert.events_unavailable')}
            </div>
          </div>
        )}
      </SectionCard>
    </details>
  );
}
