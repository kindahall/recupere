import { useCallback, useEffect, useState } from 'react';
import type { DetectedDevice, ImportedRecoverySourceStatus } from '../types';
import { fetchImportedRecoverySourceStatus, prepareImportedRecoverySource } from './useIpc';

interface UseSelectedImportedSourceStatusResult {
  status: ImportedRecoverySourceStatus | null;
  loading: boolean;
  preparing: boolean;
  error: string | null;
  refresh: () => Promise<ImportedRecoverySourceStatus | null>;
  prepare: () => Promise<ImportedRecoverySourceStatus | null>;
  isImportedSource: boolean;
  isBlocked: boolean;
}

export function useSelectedImportedSourceStatus(
  device: DetectedDevice | null | undefined,
): UseSelectedImportedSourceStatusResult {
  const isImportedSource = device?.type === 'image';
  const [status, setStatus] = useState<ImportedRecoverySourceStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!device || device.type !== 'image') {
      setStatus(null);
      setError(null);
      setLoading(false);
      return null;
    }

    setLoading(true);
    setError(null);
    try {
      const nextStatus = await fetchImportedRecoverySourceStatus(device.id);
      setStatus(nextStatus);
      return nextStatus;
    } catch (err) {
      setStatus(null);
      setError(err instanceof Error ? err.message : String(err));
      return null;
    } finally {
      setLoading(false);
    }
  }, [device]);

  const prepare = useCallback(async () => {
    if (!device || device.type !== 'image') {
      return null;
    }

    setPreparing(true);
    setError(null);
    try {
      const nextStatus = await prepareImportedRecoverySource(device.id);
      setStatus(nextStatus);
      return nextStatus;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return null;
    } finally {
      setPreparing(false);
    }
  }, [device]);

  useEffect(() => {
    if (!device || device.type !== 'image') {
      setStatus(null);
      setError(null);
      setLoading(false);
      setPreparing(false);
      return;
    }

    void refresh();
  }, [device, refresh]);

  return {
    status,
    loading,
    preparing,
    error,
    refresh,
    prepare,
    isImportedSource,
    isBlocked:
      isImportedSource &&
      (loading ||
        status?.supportTier === 'unsupported' ||
        !status?.sourceAvailable ||
        ((status?.requiresPreparation ?? false) && !status?.prepared)),
  };
}
