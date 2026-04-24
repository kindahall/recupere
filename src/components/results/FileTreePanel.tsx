import {
  CheckSquare,
  ChevronDown,
  ChevronRight,
  Eye,
  File as FileIcon,
  Folder,
  FolderOpen,
  Square,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { FileTreeNode, RecoveredFile } from '../../types';
import { computeVirtualTreeWindow, flattenVisibleFileTree } from '../../utils/fileTree';

interface FileTreePanelProps {
  root: FileTreeNode;
  selectedFileIds: Set<string>;
  activePreviewFileId: string | null;
  onToggleFileSelection: (fileId: string) => void;
  onOpenFile: (file: RecoveredFile) => void;
}

const TREE_PANEL_HEIGHT = 420;
const TREE_ROW_HEIGHT = 36;

function collectDirectoryIds(node: FileTreeNode): string[] {
  if (!node.isDirectory) {
    return [];
  }

  return [node.id, ...(node.children ?? []).flatMap((child) => collectDirectoryIds(child))];
}

export function FileTreePanel({
  root,
  selectedFileIds,
  activePreviewFileId,
  onToggleFileSelection,
  onOpenFile,
}: FileTreePanelProps) {
  const { t } = useTranslation();
  const defaultExpandedIds = useMemo(() => collectDirectoryIds(root), [root]);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set(defaultExpandedIds));
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    setExpandedIds(new Set(defaultExpandedIds));
    setScrollTop(0);
  }, [defaultExpandedIds]);

  const toggleDirectory = (nodeId: string) => {
    setExpandedIds((previous) => {
      const next = new Set(previous);
      if (next.has(nodeId)) {
        next.delete(nodeId);
      } else {
        next.add(nodeId);
      }
      return next;
    });
  };

  const rows = useMemo(() => flattenVisibleFileTree(root, expandedIds), [expandedIds, root]);
  const virtualWindow = useMemo(
    () => computeVirtualTreeWindow(rows.length, scrollTop, TREE_PANEL_HEIGHT, TREE_ROW_HEIGHT),
    [rows.length, scrollTop],
  );
  const visibleRows = rows.slice(virtualWindow.startIndex, virtualWindow.endIndex);

  const renderRow = (rowIndex: number): ReactNode => {
    const row = visibleRows[rowIndex];
    const node = row.node;
    const depth = row.depth;

    if (node.isDirectory) {
      const isExpanded = expandedIds.has(node.id);
      const fileCount = row.fileCount ?? 0;

      return (
        <div key={node.id} className="tree-node" style={{ height: TREE_ROW_HEIGHT }}>
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => toggleDirectory(node.id)}
            style={{
              width: '100%',
              height: TREE_ROW_HEIGHT,
              justifyContent: 'flex-start',
              gap: 'var(--space-2)',
              paddingInlineStart: `calc(var(--space-2) + ${depth} * var(--space-4))`,
              color: 'var(--color-text)',
            }}
          >
            {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            {isExpanded ? (
              <FolderOpen size={15} className="text-accent" />
            ) : (
              <Folder size={15} className="text-accent" />
            )}
            <span className="font-medium">{node.name}</span>
            <span className="text-xs text-secondary">({fileCount})</span>
          </button>
        </div>
      );
    }

    const file = node.file;
    if (!file) {
      return null;
    }

    const isSelected = selectedFileIds.has(file.id);
    const isPreviewActive = activePreviewFileId === file.id;

    return (
      <div
        key={node.id}
        className={`tree-node${isPreviewActive ? ' selected' : ''}`}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 'var(--space-2)',
          height: TREE_ROW_HEIGHT,
          padding: 'var(--space-1) var(--space-2)',
          paddingInlineStart: `calc(var(--space-5) + ${depth} * var(--space-4))`,
        }}
      >
        <button
          type="button"
          className="btn btn-ghost btn-icon btn-sm"
          onClick={() => onToggleFileSelection(file.id)}
          title={isSelected ? t('results.tree_deselect') : t('results.tree_select')}
        >
          {isSelected ? <CheckSquare size={15} className="text-accent" /> : <Square size={15} />}
        </button>
        <button
          type="button"
          className="btn btn-ghost"
          onClick={() => onOpenFile(file)}
          style={{
            flex: 1,
            justifyContent: 'flex-start',
            gap: 'var(--space-2)',
            minWidth: 0,
            color: isPreviewActive ? 'var(--color-accent)' : 'var(--color-text)',
          }}
        >
          <FileIcon size={14} />
          <span
            style={{
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {file.name}
          </span>
          {file.isDeleted && <span className="badge-device">{t('results.deleted_badge')}</span>}
          {file.resourceFork && (
            <span className="badge-device">{t('results.resource_fork_badge')}</span>
          )}
          {file.alternateDataStreams && file.alternateDataStreams.length > 0 && (
            <span className="badge-device">{t('results.ads_badge')}</span>
          )}
          {file.filesystemMemoryContext && (
            <span className="badge-device">{t('results.filesystem_memory_badge')}</span>
          )}
        </button>
        {file.previewAvailable && (
          <button
            type="button"
            className="btn btn-ghost btn-icon btn-sm"
            onClick={() => onOpenFile(file)}
            title={t('results.preview')}
          >
            <Eye size={14} />
          </button>
        )}
      </div>
    );
  };

  if (rows.length === 0) {
    return <div className="text-sm text-secondary">{t('results.tree_empty')}</div>;
  }

  return (
    <div
      className="file-tree-panel"
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      style={{
        height: TREE_PANEL_HEIGHT,
        overflow: 'auto',
      }}
    >
      <div
        style={{
          height: virtualWindow.totalHeight,
          position: 'relative',
        }}
      >
        <div
          style={{
            position: 'absolute',
            top: virtualWindow.offsetTop,
            left: 0,
            right: 0,
          }}
        >
          {visibleRows.map((_, rowIndex) => renderRow(rowIndex))}
        </div>
      </div>
    </div>
  );
}
