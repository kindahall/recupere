import type { FileTreeNode, RecoveredFile } from '../types';

export type FileTreeFilterMode = 'all' | 'deleted' | 'previewable' | 'selected';

export interface FileTreeFilterOptions {
  query?: string;
  mode?: FileTreeFilterMode;
  selectedFileIds?: ReadonlySet<string>;
}

export interface FlattenedFileTreeRow {
  id: string;
  node: FileTreeNode;
  depth: number;
  fileCount?: number;
}

export interface VirtualTreeWindow {
  startIndex: number;
  endIndex: number;
  offsetTop: number;
  totalHeight: number;
}

interface MutableTreeNode {
  id: string;
  name: string;
  isDirectory: boolean;
  file?: RecoveredFile;
  children: MutableTreeNode[];
  directories: Map<string, MutableTreeNode>;
}

function createDirectoryNode(id: string, name: string): MutableTreeNode {
  return {
    id,
    name,
    isDirectory: true,
    children: [],
    directories: new Map(),
  };
}

function normalizePathSegments(path: string): string[] {
  if (!path || path === '/') {
    return [];
  }

  return path
    .split('/')
    .map((segment) => segment.trim())
    .filter(Boolean);
}

function sortNodes(nodes: MutableTreeNode[]): MutableTreeNode[] {
  return nodes
    .map((node) => ({
      ...node,
      children: node.isDirectory ? sortNodes(node.children) : node.children,
    }))
    .sort((left, right) => {
      if (left.isDirectory !== right.isDirectory) {
        return left.isDirectory ? -1 : 1;
      }

      return left.name.localeCompare(right.name, undefined, {
        numeric: true,
        sensitivity: 'base',
      });
    });
}

function finalizeNode(node: MutableTreeNode): FileTreeNode {
  return {
    id: node.id,
    name: node.name,
    isDirectory: node.isDirectory,
    file: node.file,
    children: node.isDirectory ? sortNodes(node.children).map(finalizeNode) : undefined,
  };
}

export function countTreeFiles(node: FileTreeNode): number {
  if (!node.isDirectory) {
    return 1;
  }

  return (node.children ?? []).reduce((sum, child) => sum + countTreeFiles(child), 0);
}

export function buildFileTree(files: RecoveredFile[]): FileTreeNode {
  const root = createDirectoryNode('root', '/');

  for (const file of files) {
    const segments = normalizePathSegments(file.path);
    let current = root;
    let currentId = 'root';

    for (const segment of segments) {
      const nextId = `${currentId}/${segment}`;
      let next = current.directories.get(segment);
      if (!next) {
        next = createDirectoryNode(nextId, segment);
        current.directories.set(segment, next);
        current.children.push(next);
      }
      current = next;
      currentId = nextId;
    }

    current.children.push({
      id: file.id,
      name: file.name,
      isDirectory: false,
      file,
      children: [],
      directories: new Map(),
    });
  }

  return finalizeNode(root);
}

export function filterFilesForTree(
  files: RecoveredFile[],
  { query = '', mode = 'all', selectedFileIds }: FileTreeFilterOptions = {},
): RecoveredFile[] {
  const normalizedQuery = query.trim().toLowerCase();

  return files.filter((file) => {
    if (mode === 'deleted' && !file.isDeleted) {
      return false;
    }

    if (mode === 'previewable' && !file.previewAvailable) {
      return false;
    }

    if (mode === 'selected' && !selectedFileIds?.has(file.id)) {
      return false;
    }

    if (!normalizedQuery) {
      return true;
    }

    return [file.name, file.path, file.extension, file.recoveryMethod, file.mimeType ?? '']
      .join(' ')
      .toLowerCase()
      .includes(normalizedQuery);
  });
}

export function flattenVisibleFileTree(
  root: FileTreeNode,
  expandedIds: ReadonlySet<string>,
): FlattenedFileTreeRow[] {
  const rows: FlattenedFileTreeRow[] = [];

  const visit = (node: FileTreeNode, depth: number) => {
    if (node.isDirectory) {
      rows.push({
        id: node.id,
        node,
        depth,
        fileCount: countTreeFiles(node),
      });

      if (!expandedIds.has(node.id)) {
        return;
      }

      for (const child of node.children ?? []) {
        visit(child, depth + 1);
      }

      return;
    }

    rows.push({
      id: node.id,
      node,
      depth,
    });
  };

  for (const child of root.children ?? []) {
    visit(child, 0);
  }

  return rows;
}

export function computeVirtualTreeWindow(
  itemCount: number,
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  overscan = 6,
): VirtualTreeWindow {
  if (itemCount <= 0 || viewportHeight <= 0 || rowHeight <= 0) {
    return {
      startIndex: 0,
      endIndex: 0,
      offsetTop: 0,
      totalHeight: 0,
    };
  }

  const visibleCount = Math.max(1, Math.ceil(viewportHeight / rowHeight));
  const startIndex = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const endIndex = Math.min(itemCount, startIndex + visibleCount + overscan * 2);

  return {
    startIndex,
    endIndex,
    offsetTop: startIndex * rowHeight,
    totalHeight: itemCount * rowHeight,
  };
}
