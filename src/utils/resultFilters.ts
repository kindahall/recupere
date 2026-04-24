import type {
  CompressionKind,
  FileIntegrity,
  RecoveredFile,
  RecoveryComplexity,
  RecoverySourceView,
} from '../types';

export type ResultsTypeFilter = 'all' | 'deleted' | 'visible' | 'carved' | 'previewable';
export type ResultsDateField = 'modifiedAt' | 'createdAt';
export type ResultsSortKey = 'name' | 'path' | 'size' | 'score' | 'modifiedAt';
export type ResultsSortDirection = 'asc' | 'desc';
export type ResultsSourceViewFilter = 'all' | RecoverySourceView;
export type ResultsCompressionFilter = 'all' | 'none' | CompressionKind;
export type ResultsComplexityFilter = 'all' | RecoveryComplexity;

export interface ResultsFilterState {
  query?: string;
  integrity?: FileIntegrity | 'all';
  type?: ResultsTypeFilter;
  sourceView?: ResultsSourceViewFilter;
  compressionKind?: ResultsCompressionFilter;
  recoveryComplexity?: ResultsComplexityFilter;
  extension?: string;
  minSizeBytes?: number;
  maxSizeBytes?: number;
  minRecoveryScore?: number;
  dateField?: ResultsDateField;
  dateFrom?: string;
  dateTo?: string;
}

function normalizeExtension(extension: string | undefined): string {
  return (extension ?? '').trim().toLowerCase().replace(/^\./, '');
}

function parseTimestamp(value: string | undefined): number | null {
  if (!value) {
    return null;
  }

  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp) ? null : timestamp;
}

function parseBoundaryDate(value: string | undefined, endOfDay = false): number | null {
  if (!value) {
    return null;
  }

  const timestamp = Date.parse(`${value}T${endOfDay ? '23:59:59.999' : '00:00:00.000'}`);
  return Number.isNaN(timestamp) ? null : timestamp;
}

function getDateValue(file: RecoveredFile, field: ResultsDateField): number | null {
  return field === 'createdAt' ? parseTimestamp(file.createdAt) : parseTimestamp(file.modifiedAt);
}

export function isApfsCatalogPreviewFirstCandidate(file: RecoveredFile): boolean {
  return (
    Boolean(file.isDeleted) &&
    file.sourceView === 'live-catalog' &&
    (file.validatorStatus === 'unsupported' || file.validatorStatus === 'partial-unvalidated')
  );
}

export function isApfsCatalogReassembledCandidate(file: RecoveredFile): boolean {
  return (
    Boolean(file.isDeleted) &&
    file.sourceView === 'live-catalog' &&
    file.validatorStatus === 'reassembled'
  );
}

export function buildDefaultExportBatch(files: RecoveredFile[]): {
  files: RecoveredFile[];
  excludedPreviewFirstCount: number;
} {
  const previewFirstIds = new Set(
    files.filter(isApfsCatalogPreviewFirstCandidate).map((file) => file.id),
  );

  if (previewFirstIds.size === 0 || previewFirstIds.size === files.length) {
    return {
      files,
      excludedPreviewFirstCount: 0,
    };
  }

  return {
    files: files.filter((file) => !previewFirstIds.has(file.id)),
    excludedPreviewFirstCount: previewFirstIds.size,
  };
}

function matchesType(file: RecoveredFile, type: ResultsTypeFilter): boolean {
  switch (type) {
    case 'deleted':
      return Boolean(file.isDeleted);
    case 'visible':
      return !file.isDeleted;
    case 'carved':
      return file.recoveryMethod === 'carving';
    case 'previewable':
      return file.previewAvailable;
    default:
      return true;
  }
}

function matchesSourceView(file: RecoveredFile, sourceView: ResultsSourceViewFilter): boolean {
  if (sourceView === 'all') {
    return true;
  }

  if (sourceView === 'journal') {
    return file.sourceView === 'journal' || Boolean(file.journalDerived);
  }

  return file.sourceView === sourceView;
}

function matchesCompression(
  file: RecoveredFile,
  compressionKind: ResultsCompressionFilter,
): boolean {
  if (compressionKind === 'all') {
    return true;
  }

  if (compressionKind === 'none') {
    return !file.compressionKind;
  }

  return file.compressionKind === compressionKind;
}

function matchesRecoveryComplexity(
  file: RecoveredFile,
  recoveryComplexity: ResultsComplexityFilter,
): boolean {
  if (recoveryComplexity === 'all') {
    return true;
  }

  return file.recoveryComplexity === recoveryComplexity;
}

export function collectRecoveryFileExtensions(files: RecoveredFile[]): string[] {
  return Array.from(
    new Set(files.map((file) => normalizeExtension(file.extension)).filter(Boolean)),
  ).sort((left, right) =>
    left.localeCompare(right, undefined, {
      numeric: true,
      sensitivity: 'base',
    }),
  );
}

export function filterRecoveryFiles(
  files: RecoveredFile[],
  {
    query = '',
    integrity = 'all',
    type = 'all',
    sourceView = 'all',
    compressionKind = 'all',
    recoveryComplexity = 'all',
    extension = '',
    minSizeBytes,
    maxSizeBytes,
    minRecoveryScore,
    dateField = 'modifiedAt',
    dateFrom,
    dateTo,
  }: ResultsFilterState = {},
): RecoveredFile[] {
  const normalizedQuery = query.trim().toLowerCase();
  const normalizedExtension = normalizeExtension(extension);
  const minDate = parseBoundaryDate(dateFrom);
  const maxDate = parseBoundaryDate(dateTo, true);

  return files.filter((file) => {
    if (integrity !== 'all' && file.integrity !== integrity) {
      return false;
    }

    if (!matchesType(file, type)) {
      return false;
    }

    if (!matchesSourceView(file, sourceView)) {
      return false;
    }

    if (!matchesCompression(file, compressionKind)) {
      return false;
    }

    if (!matchesRecoveryComplexity(file, recoveryComplexity)) {
      return false;
    }

    if (normalizedExtension && normalizeExtension(file.extension) !== normalizedExtension) {
      return false;
    }

    if (typeof minSizeBytes === 'number' && file.sizeBytes < minSizeBytes) {
      return false;
    }

    if (typeof maxSizeBytes === 'number' && file.sizeBytes > maxSizeBytes) {
      return false;
    }

    if (typeof minRecoveryScore === 'number' && file.recoveryScore < minRecoveryScore) {
      return false;
    }

    if (typeof minDate === 'number' || typeof maxDate === 'number') {
      const fileDate = getDateValue(file, dateField);

      if (fileDate === null) {
        return false;
      }

      if (typeof minDate === 'number' && fileDate < minDate) {
        return false;
      }

      if (typeof maxDate === 'number' && fileDate > maxDate) {
        return false;
      }
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

function compareStrings(left: string, right: string): number {
  return left.localeCompare(right, undefined, {
    numeric: true,
    sensitivity: 'base',
  });
}

function getSortNumber(file: RecoveredFile, key: ResultsSortKey): number {
  switch (key) {
    case 'size':
      return file.sizeBytes;
    case 'score':
      return file.recoveryScore;
    case 'modifiedAt':
      return parseTimestamp(file.modifiedAt) ?? parseTimestamp(file.createdAt) ?? 0;
    default:
      return 0;
  }
}

export function sortRecoveryFiles(
  files: RecoveredFile[],
  sortKey: ResultsSortKey,
  sortDirection: ResultsSortDirection,
): RecoveredFile[] {
  const direction = sortDirection === 'asc' ? 1 : -1;

  return [...files].sort((left, right) => {
    if (sortKey === 'score' && sortDirection === 'desc') {
      const leftPreviewFirst = isApfsCatalogPreviewFirstCandidate(left);
      const rightPreviewFirst = isApfsCatalogPreviewFirstCandidate(right);

      if (leftPreviewFirst !== rightPreviewFirst) {
        return leftPreviewFirst ? 1 : -1;
      }
    }

    let comparison = 0;

    if (sortKey === 'name') {
      comparison = compareStrings(left.name, right.name);
    } else if (sortKey === 'path') {
      comparison = compareStrings(`${left.path}/${left.name}`, `${right.path}/${right.name}`);
    } else {
      comparison = getSortNumber(left, sortKey) - getSortNumber(right, sortKey);
    }

    if (comparison !== 0) {
      return comparison * direction;
    }

    return compareStrings(left.id, right.id);
  });
}
