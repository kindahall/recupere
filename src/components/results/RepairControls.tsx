import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { AlertTriangle, CheckCircle2, Download, LoaderCircle, Wrench } from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { type RepairCommandResult, repairFile, saveRepairedFile } from '../../hooks/ipc/preview';

const REPAIRABLE_EXT = new Set([
  'jpg',
  'jpeg',
  'png',
  'pdf',
  'zip',
  'docx',
  'xlsx',
  'pptx',
  'odt',
  'ods',
  'odp',
  'epub',
  'jar',
  'apk',
  'mp4',
  'mov',
  'm4v',
  'm4a',
]);
const PARTIAL_REPAIR_EXT = new Set(['mp4', 'mov', 'm4v', 'm4a']);

interface RepairControlsProps {
  scanId: string;
  fileId: string;
  fileName: string;
  extension: string;
  onRepairedAsset?: (url: string) => void;
}

export function RepairControls({
  scanId,
  fileId,
  fileName,
  extension,
  onRepairedAsset,
}: RepairControlsProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<RepairCommandResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ext = extension.toLowerCase();
  const repairSupported = REPAIRABLE_EXT.has(ext);
  const repairCapability = repairSupported
    ? PARTIAL_REPAIR_EXT.has(ext)
      ? 'partial'
      : 'repairable'
    : 'none';

  const runRepair = async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await repairFile(scanId, fileId);
      setResult(r);
      if (r.asset_path && onRepairedAsset) {
        const url = isTauri() ? convertFileSrc(r.asset_path) : r.asset_path;
        onRepairedAsset(url);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const saveRepaired = async () => {
    if (!result?.asset_path) return;
    try {
      const dest = await saveDialog({
        defaultPath: `repaired-${fileName}`,
        title: t('results.repair_save_title', 'Enregistrer le fichier réparé'),
      });
      if (!dest) return;
      await saveRepairedFile(result.asset_path, dest as string);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const confidence = result?.report.confidence;
  const confidenceLabel: Record<string, string> = {
    intact: t('results.repair_intact', 'Déjà intact'),
    high: t('results.repair_high', 'Forte'),
    medium: t('results.repair_medium', 'Moyenne'),
    none: t('results.repair_none', 'Non réparable'),
  };
  const confidenceColor: Record<string, string> = {
    intact: 'var(--color-success)',
    high: 'var(--color-success)',
    medium: 'var(--color-warning)',
    none: 'var(--color-danger)',
  };
  const capabilityLabel = {
    repairable: t('results.repair_capability_repairable'),
    partial: t('results.repair_capability_partial'),
    none: t('results.repair_capability_none'),
  }[repairCapability];

  return (
    <div className="repair-controls">
      <div className="flex items-center gap-2">
        <span className="badge-device">{capabilityLabel}</span>
        {repairSupported && (
          <button
            type="button"
            className="btn btn-primary btn-sm"
            onClick={runRepair}
            disabled={loading}
          >
            {loading ? <LoaderCircle size={14} className="animate-spin" /> : <Wrench size={14} />}
            {loading
              ? t('results.repair_running', 'Réparation…')
              : t('results.repair_action', 'Tenter une réparation')}
          </button>
        )}
        {result?.asset_path && confidence !== 'none' && (
          <button type="button" className="btn btn-secondary btn-sm" onClick={saveRepaired}>
            <Download size={14} />
            {t('results.repair_save', 'Enregistrer la version réparée')}
          </button>
        )}
      </div>

      <div className="text-xs text-secondary" style={{ marginTop: 'var(--space-2)' }}>
        {t('results.repair_honest_note')}
      </div>

      {error && (
        <div className="repair-status error">
          <AlertTriangle size={14} /> {error}
        </div>
      )}

      {result && !error && (
        <div className="repair-report">
          <div className="repair-report-header">
            <CheckCircle2 size={14} style={{ color: confidenceColor[confidence ?? 'none'] }} />
            <span className="repair-format">{result.report.format.toUpperCase()}</span>
            <span
              className="repair-confidence"
              style={{
                background: `${confidenceColor[confidence ?? 'none']}22`,
                color: confidenceColor[confidence ?? 'none'],
              }}
            >
              {confidenceLabel[confidence ?? 'none']}
            </span>
            <span className="repair-sizes">
              {result.report.original_size.toLocaleString()} →{' '}
              {result.report.repaired_size.toLocaleString()} octets
            </span>
          </div>
          {result.report.actions.length > 0 && (
            <ul className="repair-list">
              {result.report.actions.map((a, i) => (
                <li key={`a-${i}`}>{a}</li>
              ))}
            </ul>
          )}
          {result.report.notes.length > 0 && (
            <ul className="repair-list muted">
              {result.report.notes.map((n, i) => (
                <li key={`n-${i}`}>{n}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
