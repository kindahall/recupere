import { describe, expect, it } from 'vitest';
import type { RecoveredFile } from '../types';
import {
  buildDefaultExportBatch,
  collectRecoveryFileExtensions,
  filterRecoveryFiles,
  isApfsCatalogPreviewFirstCandidate,
  isApfsCatalogReassembledCandidate,
  sortRecoveryFiles,
} from './resultFilters';

function makeFile(
  overrides: Partial<RecoveredFile> & Pick<RecoveredFile, 'id' | 'name'>,
): RecoveredFile {
  const { id, name, ...rest } = overrides;

  return {
    id,
    name,
    path: rest.path ?? '/',
    extension: rest.extension ?? 'txt',
    sizeBytes: rest.sizeBytes ?? 10,
    integrity: rest.integrity ?? 'intact',
    recoveryScore: rest.recoveryScore ?? 95,
    recoveryMethod: rest.recoveryMethod ?? 'filesystem',
    previewAvailable: rest.previewAvailable ?? true,
    ...rest,
  };
}

describe('collectRecoveryFileExtensions', () => {
  it('collects normalized unique extensions in alphabetical order', () => {
    const files = [
      makeFile({ id: 'file-1', name: 'report.TXT', extension: '.TXT' }),
      makeFile({ id: 'file-2', name: 'photo.jpg', extension: 'jpg' }),
      makeFile({ id: 'file-3', name: 'notes.txt', extension: 'txt' }),
      makeFile({ id: 'file-4', name: 'README', extension: '' }),
    ];

    expect(collectRecoveryFileExtensions(files)).toEqual(['jpg', 'txt']);
  });
});

describe('filterRecoveryFiles', () => {
  const files = [
    makeFile({
      id: 'file-1',
      name: 'budget.xlsx',
      path: '/finance',
      extension: 'xlsx',
      sizeBytes: 4 * 1024 * 1024,
      recoveryScore: 88,
      integrity: 'intact',
      modifiedAt: '2026-03-10T09:30:00Z',
    }),
    makeFile({
      id: 'file-2',
      name: 'deleted-report.docx',
      path: '/finance/archive',
      extension: 'docx',
      sizeBytes: 800 * 1024,
      recoveryScore: 54,
      integrity: 'partial',
      isDeleted: true,
      modifiedAt: '2026-02-10T09:30:00Z',
      createdAt: '2026-02-01T09:30:00Z',
    }),
    makeFile({
      id: 'file-3',
      name: 'carved-photo.jpg',
      path: '/carved',
      extension: 'jpg',
      sizeBytes: 12 * 1024 * 1024,
      recoveryScore: 71,
      integrity: 'fragmented',
      recoveryMethod: 'carving',
      previewAvailable: true,
      modifiedAt: '2026-01-15T15:00:00Z',
    }),
    makeFile({
      id: 'file-4',
      name: 'raw.bin',
      path: '/dump',
      extension: 'bin',
      sizeBytes: 24 * 1024 * 1024,
      recoveryScore: 32,
      integrity: 'corrupt',
      previewAvailable: false,
    }),
    makeFile({
      id: 'file-5',
      name: 'unknown-run.dat',
      path: '/lab',
      extension: 'dat',
      recoveryScore: 45,
      integrity: 'uncertain',
      previewAvailable: false,
    }),
  ];

  it('filters by query, type and extension together', () => {
    expect(
      filterRecoveryFiles(files, {
        query: 'finance',
        type: 'deleted',
        extension: 'docx',
      }).map((file) => file.id),
    ).toEqual(['file-2']);
  });

  it('filters by size range and minimum recovery score', () => {
    expect(
      filterRecoveryFiles(files, {
        minSizeBytes: 1024 * 1024,
        maxSizeBytes: 16 * 1024 * 1024,
        minRecoveryScore: 70,
      }).map((file) => file.id),
    ).toEqual(['file-1', 'file-3']);
  });

  it('filters explicitly by uncertain integrity', () => {
    expect(
      filterRecoveryFiles(files, {
        integrity: 'uncertain',
      }).map((file) => file.id),
    ).toEqual(['file-5']);
  });

  it('filters by date field and inclusive range', () => {
    expect(
      filterRecoveryFiles(files, {
        dateField: 'createdAt',
        dateFrom: '2026-02-01',
        dateTo: '2026-02-01',
      }).map((file) => file.id),
    ).toEqual(['file-2']);
  });

  it('rejects files without the requested date field when date filtering is active', () => {
    expect(
      filterRecoveryFiles(files, {
        dateField: 'modifiedAt',
        dateFrom: '2026-03-01',
      }).map((file) => file.id),
    ).toEqual(['file-1']);
  });

  it('treats journal-derived files as journal provenance in the source-view filter', () => {
    expect(
      filterRecoveryFiles(
        [
          ...files,
          makeFile({
            id: 'file-5',
            name: 'journal-note.txt',
            sourceView: 'live-catalog',
            journalDerived: true,
          }),
        ],
        {
          sourceView: 'journal',
        },
      ).map((file) => file.id),
    ).toEqual(['file-5']);
  });
});

describe('sortRecoveryFiles', () => {
  const files = [
    makeFile({
      id: 'file-1',
      name: 'report-2.txt',
      path: '/finance',
      sizeBytes: 200,
      recoveryScore: 85,
      modifiedAt: '2026-03-01T10:00:00Z',
    }),
    makeFile({
      id: 'file-2',
      name: 'report-10.txt',
      path: '/archive',
      sizeBytes: 400,
      recoveryScore: 65,
      modifiedAt: '2026-01-01T10:00:00Z',
    }),
    makeFile({
      id: 'file-3',
      name: 'report-1.txt',
      path: '/finance',
      sizeBytes: 100,
      recoveryScore: 95,
      modifiedAt: '2026-02-01T10:00:00Z',
    }),
  ];

  it('sorts strings with numeric awareness', () => {
    expect(sortRecoveryFiles(files, 'name', 'asc').map((file) => file.id)).toEqual([
      'file-3',
      'file-1',
      'file-2',
    ]);
  });

  it('sorts numeric fields in descending order', () => {
    expect(sortRecoveryFiles(files, 'score', 'desc').map((file) => file.id)).toEqual([
      'file-3',
      'file-1',
      'file-2',
    ]);
  });

  it('demotes unsupported APFS catalog candidates behind simpler score-sorted files', () => {
    const apfsCatalog = makeFile({
      id: 'file-4',
      name: 'apfs-note.txt',
      recoveryScore: 97,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'unsupported',
    });

    expect(
      sortRecoveryFiles([...files, apfsCatalog], 'score', 'desc').map((file) => file.id),
    ).toEqual(['file-3', 'file-1', 'file-2', 'file-4']);
    expect(isApfsCatalogPreviewFirstCandidate(apfsCatalog)).toBe(true);
  });

  it('sorts dates with a fallback to deterministic ids', () => {
    expect(sortRecoveryFiles(files, 'modifiedAt', 'asc').map((file) => file.id)).toEqual([
      'file-2',
      'file-3',
      'file-1',
    ]);
  });
});

describe('buildDefaultExportBatch', () => {
  it('excludes APFS preview-first candidates from the implicit export batch when safer files exist', () => {
    const strong = makeFile({
      id: 'file-1',
      name: 'strong.txt',
      recoveryScore: 90,
    });
    const apfsCatalog = makeFile({
      id: 'file-2',
      name: 'apfs-risk.txt',
      recoveryScore: 96,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'unsupported',
    });

    expect(buildDefaultExportBatch([strong, apfsCatalog])).toEqual({
      files: [strong],
      excludedPreviewFirstCount: 1,
    });
  });

  it('keeps the full batch when every file is APFS preview-first', () => {
    const apfsCatalog = makeFile({
      id: 'file-1',
      name: 'apfs-risk.txt',
      recoveryScore: 96,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'unsupported',
    });

    expect(buildDefaultExportBatch([apfsCatalog])).toEqual({
      files: [apfsCatalog],
      excludedPreviewFirstCount: 0,
    });
  });

  it('treats partial-unvalidated APFS catalog candidates as preview-first too', () => {
    const strong = makeFile({
      id: 'file-1',
      name: 'strong.txt',
      recoveryScore: 90,
    });
    const apfsCatalog = makeFile({
      id: 'file-2',
      name: 'apfs-partial.txt',
      recoveryScore: 82,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'partial-unvalidated',
    });

    expect(isApfsCatalogPreviewFirstCandidate(apfsCatalog)).toBe(true);
    expect(buildDefaultExportBatch([strong, apfsCatalog])).toEqual({
      files: [strong],
      excludedPreviewFirstCount: 1,
    });
  });

  it('does not treat reassembled APFS catalog candidates as preview-first', () => {
    const apfsCatalog = makeFile({
      id: 'file-1',
      name: 'apfs-reassembled.txt',
      recoveryScore: 82,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'reassembled',
    });

    expect(isApfsCatalogPreviewFirstCandidate(apfsCatalog)).toBe(false);
  });

  it('recognizes reassembled APFS catalog candidates separately', () => {
    const apfsCatalog = makeFile({
      id: 'file-1',
      name: 'apfs-reassembled.txt',
      recoveryScore: 82,
      isDeleted: true,
      sourceView: 'live-catalog',
      validatorStatus: 'reassembled',
    });

    expect(isApfsCatalogReassembledCandidate(apfsCatalog)).toBe(true);
  });
});
