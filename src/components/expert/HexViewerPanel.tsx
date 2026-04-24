import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { FileHexPreview, RecoveredFile } from '../../types';

type HexInspectableFile = Pick<
  RecoveredFile,
  | 'id'
  | 'name'
  | 'path'
  | 'sizeBytes'
  | 'startOffset'
  | 'isDeleted'
  | 'recoveryMethod'
  | 'resourceFork'
  | 'alternateDataStreams'
>;

interface HexTargetOption {
  key: string;
  label: string;
  sizeBytes: number;
  sourceOffset?: number;
}

interface HexViewerPanelProps {
  files: HexInspectableFile[];
  selectedFileId: string | null;
  onSelectFile: (fileId: string) => void;
  targets: HexTargetOption[];
  selectedTargetKey: string | null;
  onSelectTarget: (targetKey: string) => void;
  offsetInput: string;
  onOffsetInputChange: (value: string) => void;
  pageSize: number;
  onPageSizeChange: (value: number) => void;
  onLoad: () => void;
  onPrevious: () => void;
  onNext: () => void;
  loading: boolean;
  error: string | null;
  preview: FileHexPreview | null;
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

function formatHexOffset(offset: number): string {
  return `0x${offset.toString(16).toUpperCase().padStart(8, '0')}`;
}

export function HexViewerPanel({
  files,
  selectedFileId,
  onSelectFile,
  targets,
  selectedTargetKey,
  onSelectTarget,
  offsetInput,
  onOffsetInputChange,
  pageSize,
  onPageSizeChange,
  onLoad,
  onPrevious,
  onNext,
  loading,
  error,
  preview,
}: HexViewerPanelProps) {
  const { t } = useTranslation();
  const fileSelectId = useId();
  const targetSelectId = useId();
  const offsetInputId = useId();
  const windowSelectId = useId();
  const selectedFile = files.find((file) => file.id === selectedFileId) ?? null;
  const selectedTarget = targets.find((target) => target.key === selectedTargetKey) ?? null;

  if (files.length === 0) {
    return (
      <div
        className={error ? 'text-danger' : 'text-muted'}
        style={{ textAlign: 'center', padding: 'var(--space-4) 0' }}
      >
        {error ?? t('expert.hex_no_results')}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
          gap: 'var(--space-3)',
        }}
      >
        <div>
          <label
            htmlFor={fileSelectId}
            className="text-xs text-secondary mb-1"
            style={{ display: 'block' }}
          >
            {t('expert.hex_file_label')}
          </label>
          <select
            id={fileSelectId}
            value={selectedFileId ?? ''}
            onChange={(event) => onSelectFile(event.target.value)}
            style={{
              width: '100%',
              padding: 'var(--space-2) var(--space-3)',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
              color: 'var(--color-text-primary)',
            }}
          >
            {files.map((file) => (
              <option key={file.id} value={file.id}>
                {file.name} — {file.path || '/'}
              </option>
            ))}
          </select>
        </div>

        <div>
          <label
            htmlFor={targetSelectId}
            className="text-xs text-secondary mb-1"
            style={{ display: 'block' }}
          >
            {t('expert.hex_target_label')}
          </label>
          <select
            id={targetSelectId}
            value={selectedTargetKey ?? ''}
            onChange={(event) => onSelectTarget(event.target.value)}
            style={{
              width: '100%',
              padding: 'var(--space-2) var(--space-3)',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
              color: 'var(--color-text-primary)',
            }}
          >
            {targets.map((target) => (
              <option key={target.key} value={target.key}>
                {target.label}
              </option>
            ))}
          </select>
        </div>

        <div>
          <label
            htmlFor={offsetInputId}
            className="text-xs text-secondary mb-1"
            style={{ display: 'block' }}
          >
            {t('expert.hex_offset_label')}
          </label>
          <input
            id={offsetInputId}
            type="number"
            min="0"
            step="1"
            inputMode="numeric"
            value={offsetInput}
            onChange={(event) => onOffsetInputChange(event.target.value)}
            style={{
              width: '100%',
              padding: 'var(--space-2) var(--space-3)',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
              color: 'var(--color-text-primary)',
            }}
          />
        </div>

        <div>
          <label
            htmlFor={windowSelectId}
            className="text-xs text-secondary mb-1"
            style={{ display: 'block' }}
          >
            {t('expert.hex_window_label')}
          </label>
          <select
            id={windowSelectId}
            value={pageSize}
            onChange={(event) => onPageSizeChange(Number(event.target.value))}
            style={{
              width: '100%',
              padding: 'var(--space-2) var(--space-3)',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--color-border)',
              background: 'var(--color-bg-secondary)',
              color: 'var(--color-text-primary)',
            }}
          >
            {[128, 256, 512, 1024].map((size) => (
              <option key={size} value={size}>
                {size} B
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={onLoad}
          disabled={loading}
        >
          {loading ? t('expert.hex_loading') : t('expert.hex_load')}
        </button>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={onPrevious}
          disabled={loading || !preview?.hasMoreBefore}
        >
          <ChevronLeft size={14} />
          {t('expert.hex_previous')}
        </button>
        <button
          type="button"
          className="btn btn-secondary btn-sm"
          onClick={onNext}
          disabled={loading || !preview?.hasMoreAfter}
        >
          {t('expert.hex_next')}
          <ChevronRight size={14} />
        </button>
      </div>

      {selectedFile && (
        <div
          className="text-xs text-secondary"
          style={{
            fontFamily: 'var(--font-mono)',
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
            gap: 'var(--space-2)',
          }}
        >
          <span>
            {t('expert.hex_selected_size')}:{' '}
            {formatBytes(selectedTarget?.sizeBytes ?? selectedFile.sizeBytes)}
          </span>
          <span>
            {t('expert.hex_current_offset')}: {formatHexOffset(Number(offsetInput || '0'))}
          </span>
          {typeof selectedTarget?.sourceOffset === 'number' && (
            <span>
              {t('expert.hex_source_offset')}: {formatHexOffset(selectedTarget.sourceOffset)}
            </span>
          )}
        </div>
      )}

      {error && <div className="text-sm text-danger">{error}</div>}

      {!error && preview && (
        <div className="text-sm text-secondary">
          {t('expert.hex_range', {
            read: preview.bytesRead,
            start: formatHexOffset(preview.startOffset),
            end: formatHexOffset(
              preview.bytesRead > 0
                ? preview.startOffset + preview.bytesRead - 1
                : preview.startOffset,
            ),
            total: formatBytes(preview.totalSizeBytes),
          })}
        </div>
      )}

      <div
        style={{
          fontFamily: 'var(--font-mono)',
          fontSize: 'var(--font-size-xs)',
          background: 'var(--color-bg-secondary)',
          padding: '12px',
          borderRadius: '6px',
          lineHeight: 1.8,
          minHeight: 320,
          overflow: 'auto',
        }}
      >
        {loading ? (
          <div className="text-muted" style={{ textAlign: 'center', padding: 'var(--space-4) 0' }}>
            {t('expert.hex_loading')}
          </div>
        ) : preview ? (
          preview.lines.length > 0 ? (
            <div className="flex flex-col gap-1">
              <div
                className="text-secondary"
                style={{
                  display: 'grid',
                  gridTemplateColumns: '110px minmax(0, 1fr) 180px',
                  gap: 'var(--space-3)',
                  paddingBottom: 'var(--space-2)',
                  borderBottom: '1px solid var(--color-border)',
                  marginBottom: 'var(--space-2)',
                }}
              >
                <span>{t('expert.hex_column_offset')}</span>
                <span>{t('expert.hex_column_hex')}</span>
                <span>{t('expert.hex_column_ascii')}</span>
              </div>
              {preview.lines.map((line) => (
                <div
                  key={line.offset}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '110px minmax(0, 1fr) 180px',
                    gap: 'var(--space-3)',
                  }}
                >
                  <span className="text-secondary">{formatHexOffset(line.offset)}</span>
                  <span style={{ whiteSpace: 'pre' }}>{line.hex}</span>
                  <span style={{ whiteSpace: 'pre' }}>{line.ascii}</span>
                </div>
              ))}
            </div>
          ) : (
            <div
              className="text-muted"
              style={{ textAlign: 'center', padding: 'var(--space-4) 0' }}
            >
              {t('expert.hex_empty_window')}
            </div>
          )
        ) : (
          <div className="text-muted" style={{ textAlign: 'center', padding: 'var(--space-4) 0' }}>
            {t('expert.hex_no_file')}
          </div>
        )}
      </div>
    </div>
  );
}
