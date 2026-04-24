import { AlertCircle, FolderOpen } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { SectionCard } from '../common/SectionCard';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '—';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

interface ExportStepOptionsProps {
  destination: string;
  setDestination: (d: string) => void;
  conflictStrategy: 'rename' | 'skip' | 'overwrite';
  setConflictStrategy: (s: 'rename' | 'skip' | 'overwrite') => void;
  preserveStructure: boolean;
  setPreserveStructure: (v: boolean) => void;
  verifyIntegrity: boolean;
  setVerifyIntegrity: (v: boolean) => void;
  isSafe: boolean | null;
  validationMsg: string | null;
  availableSpace: number;
  onValidateDestination: () => void;
  validating?: boolean;
  exportValidationAvailable: boolean;
  requiredBytes: number;
  fileCount: number;
}

export function ExportStepOptions(props: ExportStepOptionsProps) {
  const { t } = useTranslation();

  return (
    <div className="grid grid-2 gap-6">
      <SectionCard title={t('export.destination')}>
        <div className="flex flex-col gap-4">
          <div className="flex gap-2">
            <input
              type="text"
              className="btn btn-secondary w-full"
              style={{
                justifyContent: 'flex-start',
                textAlign: 'left',
                fontFamily: 'var(--font-mono)',
                fontSize: 'var(--font-size-sm)',
              }}
              placeholder={t('devices.export_destination_placeholder')}
              value={props.destination}
              onChange={(e) => {
                props.setDestination(e.target.value);
              }}
            />
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={props.onValidateDestination}
              disabled={!props.destination || !props.exportValidationAvailable || props.validating}
            >
              <FolderOpen size={14} />
              {props.validating ? t('export.validating') : t('export.validate')}
            </button>
          </div>

          {props.validationMsg && (
            <div
              className={`flex items-center gap-2 text-sm ${props.isSafe ? 'text-success' : 'text-danger'}`}
            >
              <AlertCircle size={14} />
              <span>{props.validationMsg}</span>
            </div>
          )}

          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.file_count')}</span>
            <span className="font-medium">{props.fileCount}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.space_required')}</span>
            <span className="font-medium">{formatBytes(props.requiredBytes)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-sm text-secondary">{t('export.space_available')}</span>
            <span className={`font-medium ${props.availableSpace > 0 ? 'text-success' : ''}`}>
              {props.availableSpace > 0 ? formatBytes(props.availableSpace) : '—'}
            </span>
          </div>
        </div>
      </SectionCard>

      <SectionCard title={t('export.options')}>
        <div className="flex flex-col gap-4">
          <div>
            <div className="text-sm font-medium mb-2" style={{ display: 'block' }}>
              {t('export.conflict_strategy')}
            </div>
            <div className="flex gap-2">
              {(['rename', 'skip', 'overwrite'] as const).map((strategy) => (
                <button
                  type="button"
                  key={strategy}
                  className={`btn ${props.conflictStrategy === strategy ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => props.setConflictStrategy(strategy)}
                >
                  {t(`export.conflict_${strategy}`)}
                </button>
              ))}
            </div>
          </div>

          <label className="flex items-center gap-3" style={{ cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={props.preserveStructure}
              onChange={(e) => props.setPreserveStructure(e.target.checked)}
              style={{ width: 16, height: 16, accentColor: 'var(--color-accent)' }}
            />
            <span className="text-md">{t('export.preserve_structure')}</span>
          </label>
          <label className="flex items-center gap-3" style={{ cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={props.verifyIntegrity}
              onChange={(e) => props.setVerifyIntegrity(e.target.checked)}
              style={{ width: 16, height: 16, accentColor: 'var(--color-accent)' }}
            />
            <span className="text-md">{t('export.verify_integrity')}</span>
          </label>
        </div>
      </SectionCard>
    </div>
  );
}
