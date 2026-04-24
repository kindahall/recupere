import type {
  DetectedDevice,
  FileIntegrity,
  ImportedRecoverySourceStatus,
  RecoveredFile,
  RecoveryContextHint,
  RecoveryContextLookupInput,
} from '../types';

export function normalizeComparableFilesystemPath(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) {
    return '';
  }

  const slashNormalized = trimmed.replace(/\\/g, '/').replace(/\/+/g, '/');
  const withoutTrailing =
    slashNormalized.length > 1 ? slashNormalized.replace(/\/+$/, '') : slashNormalized;

  if (/^[A-Za-z]:/.test(withoutTrailing)) {
    return `${withoutTrailing[0].toLowerCase()}${withoutTrailing.slice(1)}`;
  }

  return withoutTrailing;
}

export function pathsAppearRelated(left: string, right: string): boolean {
  const normalizedLeft = normalizeComparableFilesystemPath(left);
  const normalizedRight = normalizeComparableFilesystemPath(right);

  if (!normalizedLeft || !normalizedRight) {
    return false;
  }

  return (
    normalizedLeft === normalizedRight ||
    normalizedLeft.startsWith(`${normalizedRight}/`) ||
    normalizedRight.startsWith(`${normalizedLeft}/`)
  );
}

export function collectFilesystemContextRoots(
  device: DetectedDevice | null | undefined,
  importedSourceStatus: ImportedRecoverySourceStatus | null | undefined,
): string[] {
  const candidates = [
    ...(device?.partitions.flatMap((partition) =>
      partition.mountPath ? [partition.mountPath] : [],
    ) ?? []),
    importedSourceStatus?.analysisPath,
    importedSourceStatus?.sourcePath,
  ];

  return Array.from(
    new Set(
      candidates
        .filter(
          (candidate): candidate is string =>
            typeof candidate === 'string' && candidate.trim().length > 0,
        )
        .map((candidate) => normalizeComparableFilesystemPath(candidate)),
    ),
  );
}

export function buildRecoveryContextLookupInputs(
  files: RecoveredFile[],
): RecoveryContextLookupInput[] {
  return files.map((file) => ({
    fileId: file.id,
    name: file.name,
    path: file.path,
    sizeBytes: file.sizeBytes,
  }));
}

function baseFilesystemMemoryScoreAdjustment(hint: RecoveryContextHint): number {
  if (hint.matchedBy === 'path') {
    switch (hint.confidence) {
      case 'high':
        return 8;
      case 'medium':
        return 6;
      case 'low':
        return 3;
    }
  }

  switch (hint.confidence) {
    case 'high':
      return 4;
    case 'medium':
      return 2;
    case 'low':
      return 1;
  }
}

function capFilesystemMemoryAdjustmentForIntegrity(
  integrity: FileIntegrity,
  adjustment: number,
): number {
  switch (integrity) {
    case 'uncertain':
    case 'corrupt':
      return 0;
    case 'partial':
    case 'fragmented':
      return Math.min(adjustment, 4);
    default:
      return adjustment;
  }
}

function computeFilesystemMemoryScoreAdjustment(
  file: RecoveredFile,
  hint: RecoveryContextHint | undefined,
): number {
  if (!hint || !file.isDeleted) {
    return 0;
  }

  return capFilesystemMemoryAdjustmentForIntegrity(
    file.integrity,
    baseFilesystemMemoryScoreAdjustment(hint),
  );
}

export function applyRecoveryContextHints(
  files: RecoveredFile[],
  hints: RecoveryContextHint[],
): RecoveredFile[] {
  const hintsByFileId = new Map(hints.map((hint) => [hint.fileId, hint]));

  return files.map((file) => {
    const hint = hintsByFileId.get(file.id);
    const baseRecoveryScore = file.baseRecoveryScore ?? file.recoveryScore;
    const adjustment = computeFilesystemMemoryScoreAdjustment(file, hint);
    const recoveryScore =
      adjustment > 0 && baseRecoveryScore < 96
        ? Math.min(96, baseRecoveryScore + adjustment)
        : baseRecoveryScore;

    return {
      ...file,
      recoveryScore,
      baseRecoveryScore: recoveryScore > baseRecoveryScore ? baseRecoveryScore : undefined,
      recoveryScoreAdjustment:
        recoveryScore > baseRecoveryScore ? recoveryScore - baseRecoveryScore : undefined,
      filesystemMemoryContext: hint,
    };
  });
}

export function countFilesystemMemoryReprioritizedFiles(files: RecoveredFile[]): number {
  return files.filter((file) => (file.recoveryScoreAdjustment ?? 0) > 0).length;
}
