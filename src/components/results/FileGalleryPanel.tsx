import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
import { CheckSquare, Eye, FileText, Film, Image as ImageIcon, Music, Square } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchFileMediaAsset, fetchFilePreview } from '../../hooks/useIpc';
import type { FilePreview, RecoveredFile } from '../../types';
import { formatBytes } from './resultsFormat';

const IMAGE_EXT = new Set([
  'jpg',
  'jpeg',
  'png',
  'gif',
  'bmp',
  'webp',
  'tiff',
  'tif',
  'heic',
  'heif',
  'svg',
  'ico',
  'avif',
]);
const VIDEO_EXT = new Set([
  'mp4',
  'mov',
  'mkv',
  'avi',
  'webm',
  'wmv',
  'flv',
  'm4v',
  'mpg',
  'mpeg',
  '3gp',
]);
const AUDIO_EXT = new Set([
  'mp3',
  'wav',
  'flac',
  'aac',
  'ogg',
  'oga',
  'm4a',
  'opus',
  'wma',
  'aiff',
  'alac',
]);

type MediaKind = 'image' | 'video' | 'audio';

function classifyExt(ext: string): MediaKind | null {
  const e = ext.toLowerCase();
  if (IMAGE_EXT.has(e)) return 'image';
  if (VIDEO_EXT.has(e)) return 'video';
  if (AUDIO_EXT.has(e)) return 'audio';
  return null;
}

interface FileGalleryPanelProps {
  scanId: string;
  files: RecoveredFile[];
  selectedFileIds: Set<string>;
  toggleFileSelection: (id: string) => void;
  onPreview: (file: RecoveredFile) => void;
}

export function FileGalleryPanel({
  scanId,
  files,
  selectedFileIds,
  toggleFileSelection,
  onPreview,
}: FileGalleryPanelProps) {
  const { t } = useTranslation();

  const previewable = files
    .map((f) => ({ file: f, kind: classifyExt(f.extension || '') }))
    .filter((entry): entry is { file: RecoveredFile; kind: MediaKind } => entry.kind !== null);

  if (previewable.length === 0) {
    return (
      <div className="text-sm text-secondary" style={{ padding: 'var(--space-4)' }}>
        {t('results.gallery_empty')}
      </div>
    );
  }

  return (
    <div className="file-gallery-grid">
      {previewable.map(({ file, kind }) => (
        <GalleryTile
          key={file.id}
          scanId={scanId}
          file={file}
          mediaKind={kind}
          isSelected={selectedFileIds.has(file.id)}
          onToggle={() => toggleFileSelection(file.id)}
          onPreview={() => onPreview(file)}
        />
      ))}
    </div>
  );
}

interface GalleryTileProps {
  scanId: string;
  file: RecoveredFile;
  mediaKind: MediaKind;
  isSelected: boolean;
  onToggle: () => void;
  onPreview: () => void;
}

function GalleryTile({
  scanId,
  file,
  mediaKind,
  isSelected,
  onToggle,
  onPreview,
}: GalleryTileProps) {
  const { t } = useTranslation();
  const tileRef = useRef<HTMLDivElement | null>(null);
  const [visible, setVisible] = useState(false);
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [mediaAssetUrl, setMediaAssetUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!tileRef.current || visible) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(tileRef.current);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    if (!visible || loading || error) return;
    if (mediaKind === 'image' && preview) return;
    if ((mediaKind === 'video' || mediaKind === 'audio') && mediaAssetUrl) return;
    let cancelled = false;
    setLoading(true);
    const task =
      mediaKind === 'image'
        ? fetchFilePreview(scanId, file.id).then((p) => {
            if (!cancelled) setPreview(p);
          })
        : fetchFileMediaAsset(scanId, file.id).then((path) => {
            if (cancelled) return;
            const url = isTauri() ? convertFileSrc(path) : path;
            setMediaAssetUrl(url);
          });
    task
      .catch(() => {
        if (!cancelled) setError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [visible, scanId, file.id, mediaKind, preview, mediaAssetUrl, loading, error]);

  const assetUrl =
    preview?.kind === 'image' && preview.assetPath
      ? isTauri()
        ? convertFileSrc(preview.assetPath)
        : preview.assetPath
      : null;

  const renderMedia = () => {
    if (mediaKind === 'image') {
      if (assetUrl) return <img src={assetUrl} alt={file.name} loading="lazy" />;
      if (loading) return <div className="file-gallery-placeholder shimmer" />;
      if (error || preview?.kind === 'unavailable') {
        return (
          <div className="file-gallery-placeholder">
            <FileText size={28} />
          </div>
        );
      }
      return (
        <div className="file-gallery-placeholder">
          <ImageIcon size={28} />
        </div>
      );
    }
    if (mediaKind === 'video') {
      if (mediaAssetUrl) {
        return (
          <video
            src={mediaAssetUrl}
            preload="metadata"
            muted
            playsInline
            onMouseEnter={(e) => {
              void (e.currentTarget as HTMLVideoElement).play().catch(() => {});
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLVideoElement).pause();
              (e.currentTarget as HTMLVideoElement).currentTime = 0;
            }}
          />
        );
      }
      return (
        <div className={`file-gallery-placeholder kind-video${loading ? ' shimmer' : ''}`}>
          <Film size={32} />
        </div>
      );
    }
    // audio
    if (mediaAssetUrl) {
      return (
        <div className="file-gallery-placeholder kind-audio">
          <Music size={32} />
          <span className="text-xs text-secondary" style={{ marginTop: 'var(--space-2)' }}>
            {t('results.preview')}
          </span>
        </div>
      );
    }
    return (
      <div className={`file-gallery-placeholder kind-audio${loading ? ' shimmer' : ''}`}>
        <Music size={32} />
      </div>
    );
  };

  return (
    <div
      ref={tileRef}
      className={`file-gallery-tile kind-${mediaKind}${isSelected ? ' selected' : ''}`}
    >
      <div className="file-gallery-thumb">
        {renderMedia()}
        <span className={`file-gallery-badge kind-${mediaKind}`}>
          {mediaKind === 'image' ? 'IMG' : mediaKind === 'video' ? 'VID' : 'AUD'}
        </span>
        <button
          type="button"
          className="file-gallery-checkbox"
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
          aria-label={isSelected ? t('results.tree_deselect') : t('results.tree_select')}
        >
          {isSelected ? <CheckSquare size={16} /> : <Square size={16} />}
        </button>
        <button
          type="button"
          className="file-gallery-eye"
          onClick={(e) => {
            e.stopPropagation();
            onPreview();
          }}
          aria-label={t('results.preview')}
        >
          <Eye size={14} />
        </button>
      </div>
      <div className="file-gallery-meta">
        <span className="file-gallery-name" title={file.name}>
          {file.name}
        </span>
        <span className="file-gallery-size">{formatBytes(file.sizeBytes)}</span>
        {file.filesystemMemoryContext && (
          <span className="text-xs text-secondary">{t('results.filesystem_memory_badge')}</span>
        )}
      </div>
    </div>
  );
}
