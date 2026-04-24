import { describe, expect, it } from 'vitest';
import type { RecoveredFile, RecoveryContextHint } from '../types';
import {
  applyRecoveryContextHints,
  countFilesystemMemoryReprioritizedFiles,
} from './filesystemMemoryContext';

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
    recoveryScore: rest.recoveryScore ?? 72,
    recoveryMethod: rest.recoveryMethod ?? 'filesystem',
    previewAvailable: rest.previewAvailable ?? true,
    ...rest,
  };
}

function makeHint(
  overrides: Partial<RecoveryContextHint> & Pick<RecoveryContextHint, 'fileId' | 'fileName'>,
): RecoveryContextHint {
  const { fileId, fileName, ...rest } = overrides;

  return {
    fileId,
    fileName,
    lastKnownPath: rest.lastKnownPath ?? '/cases/client-a',
    lastObservedAtMs: rest.lastObservedAtMs ?? 1_711_000_000_000,
    firstMissingObservedAtMs: rest.firstMissingObservedAtMs ?? 1_711_086_400_000,
    fileModifiedAtMs: rest.fileModifiedAtMs ?? 1_710_900_000_000,
    confidence: rest.confidence ?? 'high',
    matchedBy: rest.matchedBy ?? 'path',
  };
}

describe('applyRecoveryContextHints', () => {
  it('adds a bounded triage bonus to deleted intact files', () => {
    const files = [
      makeFile({
        id: 'file-1',
        name: 'contract.pdf',
        extension: 'pdf',
        isDeleted: true,
        recoveryScore: 74,
      }),
    ];
    const hints = [makeHint({ fileId: 'file-1', fileName: 'contract.pdf' })];

    const [file] = applyRecoveryContextHints(files, hints);

    expect(file.filesystemMemoryContext?.fileId).toBe('file-1');
    expect(file.baseRecoveryScore).toBe(74);
    expect(file.recoveryScoreAdjustment).toBe(8);
    expect(file.recoveryScore).toBe(82);
  });

  it('never boosts non-deleted, uncertain, or corrupt files', () => {
    const files = [
      makeFile({
        id: 'file-1',
        name: 'visible.txt',
        recoveryScore: 78,
      }),
      makeFile({
        id: 'file-2',
        name: 'broken.zip',
        extension: 'zip',
        integrity: 'corrupt',
        isDeleted: true,
        recoveryScore: 41,
      }),
      makeFile({
        id: 'file-3',
        name: 'unknown.dat',
        extension: 'dat',
        integrity: 'uncertain',
        isDeleted: true,
        recoveryScore: 44,
      }),
    ];
    const hints = [
      makeHint({ fileId: 'file-1', fileName: 'visible.txt' }),
      makeHint({ fileId: 'file-2', fileName: 'broken.zip' }),
      makeHint({ fileId: 'file-3', fileName: 'unknown.dat' }),
    ];

    const enriched = applyRecoveryContextHints(files, hints);

    expect(enriched[0].recoveryScore).toBe(78);
    expect(enriched[0].baseRecoveryScore).toBeUndefined();
    expect(enriched[0].recoveryScoreAdjustment).toBeUndefined();
    expect(enriched[1].recoveryScore).toBe(41);
    expect(enriched[1].baseRecoveryScore).toBeUndefined();
    expect(enriched[1].recoveryScoreAdjustment).toBeUndefined();
    expect(enriched[2].recoveryScore).toBe(44);
    expect(enriched[2].baseRecoveryScore).toBeUndefined();
    expect(enriched[2].recoveryScoreAdjustment).toBeUndefined();
  });

  it('caps the bonus on fragmented files and counts reprioritized items', () => {
    const files = [
      makeFile({
        id: 'file-1',
        name: 'fragmented.mov',
        extension: 'mov',
        integrity: 'fragmented',
        isDeleted: true,
        recoveryScore: 58,
      }),
      makeFile({
        id: 'file-2',
        name: 'notes.txt',
        isDeleted: true,
        recoveryScore: 70,
      }),
    ];
    const hints = [
      makeHint({ fileId: 'file-1', fileName: 'fragmented.mov' }),
      makeHint({
        fileId: 'file-2',
        fileName: 'notes.txt',
        confidence: 'medium',
        matchedBy: 'name_size',
      }),
    ];

    const enriched = applyRecoveryContextHints(files, hints);

    expect(enriched[0].recoveryScore).toBe(62);
    expect(enriched[0].recoveryScoreAdjustment).toBe(4);
    expect(enriched[1].recoveryScore).toBe(72);
    expect(enriched[1].recoveryScoreAdjustment).toBe(2);
    expect(countFilesystemMemoryReprioritizedFiles(enriched)).toBe(2);
  });
});
