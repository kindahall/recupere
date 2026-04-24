import { FileText, FileWarning, Image as ImageIcon, LoaderCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useAppStore } from '../../stores/appStore';
import type { FilePreview, RecoveredFile } from '../../types';
import { WarningBanner } from '../common/WarningBanner';
import { OfficePreview } from './OfficePreview';
import { RepairControls } from './RepairControls';

interface FilePreviewPanelProps {
  file: (Pick<RecoveredFile, 'name'> & Partial<Pick<RecoveredFile, 'id' | 'extension'>>) | null;
  scanId?: string | null;
  preview: FilePreview | null;
  loading: boolean;
  error: string | null;
  assetUrl?: string;
}

const OFFICE_EXTENSIONS = new Set(['docx', 'xlsx', 'pptx']);

export function FilePreviewPanel({
  file,
  scanId,
  preview,
  loading,
  error,
  assetUrl,
}: FilePreviewPanelProps) {
  const { t } = useTranslation();
  const isExpert = useAppStore((state) => state.userMode === 'expert');

  const extensionLower = (file?.extension ?? '').toLowerCase();
  const isOffice = OFFICE_EXTENSIONS.has(extensionLower);

  if (!file) {
    return <div className="text-sm text-secondary">{t('results.preview_empty')}</div>;
  }

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-sm text-secondary">
        <LoaderCircle size={16} className="animate-spin" />
        <span>{t('results.preview_loading')}</span>
      </div>
    );
  }

  if (error) {
    return <WarningBanner variant="warning">{error}</WarningBanner>;
  }

  if (!preview || preview.kind === 'unavailable') {
    return (
      <WarningBanner variant="info">{preview?.message ?? t('results.no_preview')}</WarningBanner>
    );
  }

  return (
    <div className="file-preview-panel flex flex-col gap-4" style={{ padding: 'var(--space-4)' }}>
      <div className="flex items-center gap-2">
        {preview.kind === 'text' ? (
          <FileText size={16} className="text-accent" />
        ) : preview.kind === 'image' ? (
          <ImageIcon size={16} className="text-accent" />
        ) : (
          <FileWarning size={16} className="text-accent" />
        )}
        <span className="font-medium">{file.name}</span>
      </div>

      {scanId && file.id && file.extension && (
        <RepairControls
          scanId={scanId}
          fileId={file.id}
          fileName={file.name}
          extension={file.extension}
        />
      )}

      {preview.truncated && (
        <WarningBanner variant="info">{t('results.preview_truncated')}</WarningBanner>
      )}

      {preview.kind === 'text' && isOffice && preview.textContent ? (
        <OfficePreview
          extension={extensionLower}
          textContent={preview.textContent}
          expert={isExpert}
        />
      ) : null}

      {preview.kind === 'text' && !isOffice && (
        <pre
          style={{
            margin: 0,
            maxHeight: 360,
            overflow: 'auto',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            padding: 'var(--space-4)',
            borderRadius: 'var(--radius-lg)',
            background: 'var(--color-bg-secondary)',
            border: '1px solid var(--color-border)',
            fontFamily: 'var(--font-mono)',
            fontSize: 'var(--font-size-sm)',
          }}
        >
          {preview.textContent}
        </pre>
      )}

      {preview.kind === 'image' && assetUrl && (
        <div
          className="file-preview-image-wrapper"
          style={{
            padding: 'var(--space-3)',
          }}
        >
          <img
            src={assetUrl}
            alt={file.name}
            style={{
              display: 'block',
              maxWidth: '100%',
              maxHeight: 420,
              margin: '0 auto',
              borderRadius: 'var(--radius-md)',
            }}
          />
        </div>
      )}

      {preview.kind === 'pdf' && assetUrl && (
        <iframe
          title={file.name}
          src={assetUrl}
          style={{
            width: '100%',
            height: 480,
            border: '1px solid var(--color-border)',
            borderRadius: 'var(--radius-lg)',
            background: 'white',
          }}
        />
      )}
    </div>
  );
}
