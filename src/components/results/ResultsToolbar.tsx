import {
  Activity,
  Calendar,
  ChevronDown,
  Filter,
  HardDrive,
  HardDriveDownload,
  Search,
  SlidersHorizontal,
  X,
  XCircle,
} from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { FileIntegrity } from '../../types';
import type {
  ResultsComplexityFilter,
  ResultsCompressionFilter,
  ResultsDateField,
  ResultsSortDirection,
  ResultsSortKey,
  ResultsSourceViewFilter,
  ResultsTypeFilter,
} from '../../utils/resultFilters';
import { CustomSelect } from '../common/CustomSelect';

export interface ResultsToolbarProps {
  resultsQuery: string;
  setResultsQuery: (v: string) => void;
  filteredCount: number;
  totalCount: number;
  hasCustomView: boolean;
  resetViewFilters: () => void;
  filterIntegrity: FileIntegrity | 'all';
  setFilterIntegrity: (v: FileIntegrity | 'all') => void;

  typeFilter: ResultsTypeFilter;
  setTypeFilter: (v: ResultsTypeFilter) => void;
  sourceViewFilter: ResultsSourceViewFilter;
  setSourceViewFilter: (v: ResultsSourceViewFilter) => void;
  compressionFilter: ResultsCompressionFilter;
  setCompressionFilter: (v: ResultsCompressionFilter) => void;
  complexityFilter: ResultsComplexityFilter;
  setComplexityFilter: (v: ResultsComplexityFilter) => void;
  extensionFilter: string;
  setExtensionFilter: (v: string) => void;
  extensionOptions: string[];
  minSizeMb: string;
  setMinSizeMb: (v: string) => void;
  maxSizeMb: string;
  setMaxSizeMb: (v: string) => void;
  minScore: string;
  setMinScore: (v: string) => void;
  dateField: ResultsDateField;
  setDateField: (v: ResultsDateField) => void;
  dateFrom: string;
  setDateFrom: (v: string) => void;
  dateTo: string;
  setDateTo: (v: string) => void;
  sortKey: ResultsSortKey;
  setSortKey: (v: ResultsSortKey) => void;
  sortDirection: ResultsSortDirection;
  setSortDirection: (v: ResultsSortDirection) => void;
}

const inputStyle = {
  width: '100%',
  padding: 'var(--space-2) var(--space-3)',
  borderRadius: 'var(--radius-md)',
  border: '1px solid var(--color-border)',
  background: 'rgba(0, 0, 0, 0.2)',
  color: 'var(--color-text-primary)',
  fontSize: 'var(--font-size-sm)',
  outline: 'none',
  transition: 'all var(--transition-fast)',
  boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.1)',
} as const;

const inputFocusStyle = `
  .filter-input:focus {
    border-color: var(--color-accent) !important;
    box-shadow: 0 0 0 2px var(--color-accent-subtle), inset 0 1px 2px rgba(0,0,0,0.1) !important;
    background: rgba(0, 0, 0, 0.4) !important;
  }
  .filter-input:hover:not(:focus) {
    border-color: var(--color-border-focus) !important;
    background: rgba(0, 0, 0, 0.3) !important;
  }
`;

const labelStyle = {
  display: 'flex',
  alignItems: 'center',
  gap: 'var(--space-1)',
  fontSize: '12px',
  fontWeight: 'var(--font-weight-medium)',
  color: 'var(--color-text-secondary)',
  marginBottom: 'var(--space-2)',
  textTransform: 'uppercase' as const,
  letterSpacing: '0.05em',
} as const;

export function ResultsToolbar(props: ResultsToolbarProps) {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);

  const {
    resultsQuery,
    setResultsQuery,
    filteredCount,
    totalCount,
    hasCustomView,
    resetViewFilters,
    filterIntegrity,
    setFilterIntegrity,
    typeFilter,
    setTypeFilter,
    sourceViewFilter,
    setSourceViewFilter,
    compressionFilter,
    setCompressionFilter,
    complexityFilter,
    setComplexityFilter,
    extensionFilter,
    setExtensionFilter,
    extensionOptions,
    minSizeMb,
    setMinSizeMb,
    maxSizeMb,
    setMaxSizeMb,
    minScore,
    setMinScore,
    dateField,
    setDateField,
    dateFrom,
    setDateFrom,
    dateTo,
    setDateTo,
    sortKey,
    setSortKey,
    sortDirection,
    setSortDirection,
  } = props;

  return (
    <div
      style={{
        background: 'var(--color-bg-elevated)',
        border: '1px solid var(--color-border)',
        borderRadius: 'var(--radius-xl)',
        padding: 'var(--space-4)',
        marginBottom: 'var(--space-6)',
        boxShadow: 'var(--shadow-lg)',
        position: 'relative',
        overflow: 'visible',
      }}
    >
      <style>{inputFocusStyle}</style>

      {/* Search Bar & Primary Actions */}
      <div
        style={{
          display: 'flex',
          gap: 'var(--space-3)',
          alignItems: 'center',
          position: 'relative',
          zIndex: 2,
        }}
      >
        <div style={{ position: 'relative', flex: 1 }}>
          <Search
            size={18}
            style={{
              position: 'absolute',
              left: '16px',
              top: '50%',
              transform: 'translateY(-50%)',
              color: 'var(--color-text-muted)',
            }}
          />
          <input
            type="search"
            value={resultsQuery}
            onChange={(e) => setResultsQuery(e.target.value)}
            placeholder={t('results.search_placeholder')}
            style={{
              width: '100%',
              padding: '12px 16px 12px 44px',
              borderRadius: 'var(--radius-lg)',
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: 'rgba(0, 0, 0, 0.3)',
              color: 'var(--color-text-primary)',
              fontSize: 'var(--font-size-md)',
              outline: 'none',
              transition: 'all 0.3s ease',
              boxShadow: 'inset 0 2px 4px rgba(0,0,0,0.2)',
            }}
            onFocus={(e) => {
              e.target.style.borderColor = 'var(--color-accent)';
              e.target.style.boxShadow =
                '0 0 0 2px var(--color-accent-subtle), inset 0 2px 4px rgba(0,0,0,0.2)';
              e.target.style.background = 'rgba(0, 0, 0, 0.5)';
            }}
            onBlur={(e) => {
              e.target.style.borderColor = 'rgba(255, 255, 255, 0.1)';
              e.target.style.boxShadow = 'inset 0 2px 4px rgba(0,0,0,0.2)';
              e.target.style.background = 'rgba(0, 0, 0, 0.3)';
            }}
          />
          {resultsQuery && (
            <button
              type="button"
              onClick={() => setResultsQuery('')}
              style={{
                position: 'absolute',
                right: '12px',
                top: '50%',
                transform: 'translateY(-50%)',
                color: 'var(--color-text-muted)',
                background: 'transparent',
                border: 'none',
                cursor: 'pointer',
                display: 'flex',
                padding: '4px',
                borderRadius: '50%',
              }}
              onMouseOver={(e) => {
                e.currentTarget.style.color = 'var(--color-text-primary)';
              }}
              onFocus={(e) => {
                e.currentTarget.style.color = 'var(--color-text-primary)';
              }}
              onMouseOut={(e) => {
                e.currentTarget.style.color = 'var(--color-text-muted)';
              }}
              onBlur={(e) => {
                e.currentTarget.style.color = 'var(--color-text-muted)';
              }}
            >
              <X size={16} />
            </button>
          )}
        </div>

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 'var(--space-4)',
            padding: '0 var(--space-2)',
          }}
        >
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'flex-end',
              justifyContent: 'center',
            }}
          >
            <span
              style={{
                fontSize: 'var(--font-size-lg)',
                fontWeight: 'var(--font-weight-bold)',
                color: 'var(--color-text-primary)',
                lineHeight: 1,
              }}
            >
              {filteredCount}
            </span>
            <span
              style={{
                fontSize: '10px',
                color: 'var(--color-text-muted)',
                textTransform: 'uppercase',
                letterSpacing: '0.05em',
                marginTop: '4px',
              }}
            >
              / {totalCount} {t('results.total_files')}
            </span>
          </div>

          <div style={{ width: '1px', height: '32px', background: 'var(--color-border)' }} />

          {hasCustomView && (
            <button
              type="button"
              className="btn"
              onClick={resetViewFilters}
              style={{
                background: 'rgba(239, 68, 68, 0.1)',
                color: 'var(--color-danger)',
                border: '1px solid rgba(239, 68, 68, 0.2)',
                borderRadius: 'var(--radius-lg)',
              }}
              onMouseOver={(e) => {
                e.currentTarget.style.background = 'rgba(239, 68, 68, 0.2)';
              }}
              onFocus={(e) => {
                e.currentTarget.style.background = 'rgba(239, 68, 68, 0.2)';
              }}
              onMouseOut={(e) => {
                e.currentTarget.style.background = 'rgba(239, 68, 68, 0.1)';
              }}
              onBlur={(e) => {
                e.currentTarget.style.background = 'rgba(239, 68, 68, 0.1)';
              }}
            >
              <XCircle size={16} />
              <span style={{ fontSize: 'var(--font-size-sm)' }}>{t('results.reset_filters')}</span>
            </button>
          )}

          <button
            type="button"
            onClick={() => setIsExpanded(!isExpanded)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 'var(--space-2)',
              padding: '10px 16px',
              borderRadius: 'var(--radius-lg)',
              background: isExpanded ? 'var(--color-accent-subtle)' : 'transparent',
              color: isExpanded ? 'var(--color-accent)' : 'var(--color-text-secondary)',
              border: `1px solid ${isExpanded ? 'var(--color-accent)' : 'var(--color-border)'}`,
              cursor: 'pointer',
              transition: 'all 0.2s',
              fontWeight: 'var(--font-weight-medium)',
            }}
            onMouseOver={(e) => {
              if (!isExpanded) {
                e.currentTarget.style.background = 'rgba(255, 255, 255, 0.05)';
              }
            }}
            onFocus={(e) => {
              if (!isExpanded) {
                e.currentTarget.style.background = 'rgba(255, 255, 255, 0.05)';
              }
            }}
            onMouseOut={(e) => {
              if (!isExpanded) {
                e.currentTarget.style.background = 'transparent';
              }
            }}
            onBlur={(e) => {
              if (!isExpanded) {
                e.currentTarget.style.background = 'transparent';
              }
            }}
          >
            <SlidersHorizontal size={16} />
            <span style={{ fontSize: 'var(--font-size-sm)' }}>{t('results.advanced_filters')}</span>
            <ChevronDown
              size={16}
              style={{
                transform: isExpanded ? 'rotate(180deg)' : 'rotate(0deg)',
                transition: 'transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
              }}
            />
          </button>
        </div>
      </div>

      {/* Advanced Filters Panel */}
      <div
        style={{
          maxHeight: isExpanded ? '1000px' : '0',
          opacity: isExpanded ? 1 : 0,
          overflow: isExpanded ? 'visible' : 'hidden', // important for custom select dropdown positioning
          transition: 'all 0.4s cubic-bezier(0.4, 0, 0.2, 1)',
          marginTop: isExpanded ? 'var(--space-4)' : '0',
          borderTop: isExpanded ? '1px solid var(--color-border)' : 'none',
          paddingTop: isExpanded ? 'var(--space-4)' : '0',
        }}
      >
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(4, 1fr)',
            gap: 'var(--space-5)',
          }}
        >
          {/* Main Attributes */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-4)' }}>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-type" style={labelStyle}>
                <Filter size={14} />
                {t('results.filter_type')}
              </label>
              <CustomSelect
                id="results-type"
                value={typeFilter}
                onChange={(val) => setTypeFilter(val as ResultsTypeFilter)}
                options={[
                  { value: 'all', label: t('results.filter_type_all') },
                  { value: 'deleted', label: t('results.filter_type_deleted') },
                  { value: 'visible', label: t('results.filter_type_visible') },
                  { value: 'carved', label: t('results.filter_type_carved') },
                  { value: 'previewable', label: t('results.filter_type_previewable') },
                ]}
              />
            </div>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-extension" style={labelStyle}>
                <Search size={14} />
                {t('results.filter_extension')}
              </label>
              <CustomSelect
                id="results-extension"
                value={extensionFilter}
                onChange={setExtensionFilter}
                options={[
                  { value: '', label: t('results.filter_extension_all') },
                  ...extensionOptions.map((ext) => ({ value: ext, label: `.${ext}` })),
                ]}
              />
            </div>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-integrity" style={labelStyle}>
                <Activity size={14} />
                {t('results.filter_integrity')}
              </label>
              <CustomSelect
                id="results-integrity"
                value={filterIntegrity}
                onChange={(val) => setFilterIntegrity(val as FileIntegrity | 'all')}
                options={[
                  { value: 'all', label: t('results.filter_integrity_all') },
                  { value: 'intact', label: t('integrity.intact') },
                  { value: 'partial', label: t('integrity.partial') },
                  { value: 'fragmented', label: t('integrity.fragmented') },
                  { value: 'uncertain', label: t('integrity.uncertain') },
                  { value: 'corrupt', label: t('integrity.corrupt') },
                ]}
              />
            </div>
          </div>

          {/* Source & Recovery */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-4)' }}>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-source-view" style={labelStyle}>
                <HardDrive size={14} />
                {t('results.filter_source_view')}
              </label>
              <CustomSelect
                id="results-source-view"
                value={sourceViewFilter}
                onChange={(val) => setSourceViewFilter(val as ResultsSourceViewFilter)}
                options={[
                  { value: 'all', label: t('results.filter_source_view_all') },
                  { value: 'mounted-volume', label: t('results.source_view_mounted_volume') },
                  { value: 'recovery-image', label: t('results.source_view_recovery_image') },
                  { value: 'live-catalog', label: t('results.source_view_live_catalog') },
                  { value: 'snapshot', label: t('results.source_view_snapshot') },
                  { value: 'journal', label: t('results.source_view_journal') },
                ]}
              />
            </div>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-complexity" style={labelStyle}>
                <Activity size={14} />
                {t('results.filter_complexity')}
              </label>
              <CustomSelect
                id="results-complexity"
                value={complexityFilter}
                onChange={(val) => setComplexityFilter(val as ResultsComplexityFilter)}
                options={[
                  { value: 'all', label: t('results.filter_complexity_all') },
                  { value: 'low', label: t('results.complexity_low') },
                  { value: 'medium', label: t('results.complexity_medium') },
                  { value: 'high', label: t('results.complexity_high') },
                ]}
              />
            </div>
            <div style={{ position: 'relative' }}>
              <label htmlFor="results-compression" style={labelStyle}>
                <HardDriveDownload size={14} />
                {t('results.filter_compression')}
              </label>
              <CustomSelect
                id="results-compression"
                value={compressionFilter}
                onChange={(val) => setCompressionFilter(val as ResultsCompressionFilter)}
                options={[
                  { value: 'all', label: t('results.filter_compression_all') },
                  { value: 'none', label: t('results.filter_compression_none') },
                  { value: 'lznt1', label: t('results.filter_compression_lznt1') },
                ]}
              />
            </div>
          </div>

          {/* Size & Score */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-4)' }}>
            <div>
              <div style={labelStyle}>{t('results.sort_size')} (Mo)</div>
              <div style={{ display: 'flex', gap: 'var(--space-2)' }}>
                <input
                  id="results-size-min"
                  className="filter-input"
                  type="number"
                  min="0"
                  step="0.1"
                  inputMode="decimal"
                  value={minSizeMb}
                  onChange={(e) => setMinSizeMb(e.target.value)}
                  placeholder={t('results.filter_size_min_placeholder')}
                  style={inputStyle}
                />
                <input
                  id="results-size-max"
                  className="filter-input"
                  type="number"
                  min="0"
                  step="0.1"
                  inputMode="decimal"
                  value={maxSizeMb}
                  onChange={(e) => setMaxSizeMb(e.target.value)}
                  placeholder={t('results.filter_size_max_placeholder')}
                  style={inputStyle}
                />
              </div>
            </div>
            <div>
              <label htmlFor="results-score-min" style={labelStyle}>
                {t('results.filter_score_min')}
              </label>
              <input
                id="results-score-min"
                className="filter-input"
                type="number"
                min="0"
                max="100"
                step="1"
                inputMode="numeric"
                value={minScore}
                onChange={(e) => setMinScore(e.target.value)}
                placeholder={t('results.filter_score_min_placeholder')}
                style={inputStyle}
              />
            </div>
          </div>

          {/* Dates & Sort */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-4)' }}>
            <div>
              <label htmlFor="results-date-field" style={labelStyle}>
                <Calendar size={14} />
                {t('results.filter_date')}
              </label>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-2)' }}>
                <div style={{ position: 'relative', zIndex: 10 }}>
                  <CustomSelect
                    id="results-date-field"
                    value={dateField}
                    onChange={(val) => setDateField(val as ResultsDateField)}
                    options={[
                      { value: 'modifiedAt', label: t('results.filter_date_modified') },
                      { value: 'createdAt', label: t('results.filter_date_created') },
                    ]}
                  />
                </div>
                <div style={{ display: 'flex', gap: 'var(--space-2)' }}>
                  <input
                    id="results-date-from"
                    className="filter-input"
                    type="date"
                    value={dateFrom}
                    onChange={(e) => setDateFrom(e.target.value)}
                    style={inputStyle}
                  />
                  <input
                    id="results-date-to"
                    className="filter-input"
                    type="date"
                    value={dateTo}
                    onChange={(e) => setDateTo(e.target.value)}
                    style={inputStyle}
                  />
                </div>
              </div>
            </div>
            <div>
              <label htmlFor="results-sort-key" style={labelStyle}>
                {t('results.sort_label')}
              </label>
              <div style={{ display: 'flex', gap: 'var(--space-2)' }}>
                <div style={{ flex: 2, position: 'relative' }}>
                  <CustomSelect
                    id="results-sort-key"
                    value={sortKey}
                    onChange={(val) => setSortKey(val as ResultsSortKey)}
                    options={[
                      { value: 'score', label: t('results.sort_score') },
                      { value: 'name', label: t('results.sort_name') },
                      { value: 'path', label: t('results.sort_path') },
                      { value: 'size', label: t('results.sort_size') },
                      { value: 'modifiedAt', label: t('results.sort_modified') },
                    ]}
                  />
                </div>
                <div style={{ flex: 1, position: 'relative' }}>
                  <CustomSelect
                    id="results-sort-direction"
                    value={sortDirection}
                    onChange={(val) => setSortDirection(val as ResultsSortDirection)}
                    options={[
                      { value: 'desc', label: t('results.sort_desc') },
                      { value: 'asc', label: t('results.sort_asc') },
                    ]}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
