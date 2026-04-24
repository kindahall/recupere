import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ExportProgress, RecoveredFile } from '../../types';
import type { ScanLogEntry } from '../../types/scan';
import { ExportStepConfirm } from './ExportStepConfirm';
import { ExportStepOptions } from './ExportStepOptions';
import { ExportStepProgress } from './ExportStepProgress';
import { ExportStepSelection } from './ExportStepSelection';

export interface ExportWizardProps {
  files: RecoveredFile[];
  implicitPreviewFirstExcludedCount: number;
  requiredBytes: number;
  resourceForkFilesCount: number;
  resourceForkBytes: number;
  alternateDataStreamsCount: number;
  alternateDataStreamsBytes: number;
  apfsExportOperatorVisible: boolean;
  apfsCurrentCatalogFilesCount: number;
  apfsReassembledFilesCount: number;
  snapshotFilesCount: number;
  journalFilesCount: number;
  compressedFilesCount: number;
  highComplexityFilesCount: number;
  filesystemMemoryContextFilesCount: number;
  filesystemMemoryAdjustedFilesCount: number;
  deletedRecoveryExport: boolean;
  carvedExport: boolean;
  lostVolumeMode: boolean;
  recoveredFilesystem: string | undefined;
  scanConfigLabel: string | undefined;
  raidImportedSource: boolean;
  raidAnalysisPath: string | undefined;
  // Options state
  destination: string;
  setDestination: (d: string) => void;
  conflictStrategy: 'rename' | 'skip' | 'overwrite';
  setConflictStrategy: (s: 'rename' | 'skip' | 'overwrite') => void;
  preserveStructure: boolean;
  setPreserveStructure: (v: boolean) => void;
  verifyIntegrity: boolean;
  setVerifyIntegrity: (v: boolean) => void;
  // Validation
  isSafe: boolean | null;
  validationMsg: string | null;
  availableSpace: number;
  onValidateDestination: () => void;
  validating?: boolean;
  exportValidationAvailable: boolean;
  exportEngineAvailable: boolean;
  // Export state
  exportRunning: boolean;
  exportProgress: ExportProgress | null;
  exportLogs: ScanLogEntry[];
  exportPercent: number;
  activeExportId: string | null;
  logsError: string | null;
  error: string | null;
  onStartExport: () => void;
}

const STEPS = ['selection', 'options', 'confirm', 'progress'] as const;

export function ExportWizard(props: ExportWizardProps) {
  const { t } = useTranslation();
  const [currentStep, setCurrentStep] = useState(0);

  // Auto-advance to progress step when export starts
  if (props.activeExportId && currentStep < 3) {
    setCurrentStep(3);
  }

  const canNext = () => {
    if (currentStep === 0) return true;
    if (currentStep === 1) return props.isSafe === true && !!props.destination;
    return false;
  };

  return (
    <div>
      <div className="wizard-steps">
        {STEPS.map((step, index) => (
          <div
            key={step}
            className={`wizard-step ${index === currentStep ? 'wizard-step-active' : ''} ${index < currentStep ? 'wizard-step-completed' : ''}`}
          >
            <span className="wizard-step-number">{index + 1}</span>
            <span className="wizard-step-label">{t(`export.wizard.step_${step}`)}</span>
          </div>
        ))}
      </div>

      <div className="mt-6">
        {currentStep === 0 && (
          <ExportStepSelection
            files={props.files}
            implicitPreviewFirstExcludedCount={props.implicitPreviewFirstExcludedCount}
            requiredBytes={props.requiredBytes}
            resourceForkFilesCount={props.resourceForkFilesCount}
            resourceForkBytes={props.resourceForkBytes}
            alternateDataStreamsCount={props.alternateDataStreamsCount}
            alternateDataStreamsBytes={props.alternateDataStreamsBytes}
            apfsExportOperatorVisible={props.apfsExportOperatorVisible}
            apfsCurrentCatalogFilesCount={props.apfsCurrentCatalogFilesCount}
            apfsReassembledFilesCount={props.apfsReassembledFilesCount}
            snapshotFilesCount={props.snapshotFilesCount}
            journalFilesCount={props.journalFilesCount}
            compressedFilesCount={props.compressedFilesCount}
            highComplexityFilesCount={props.highComplexityFilesCount}
            filesystemMemoryContextFilesCount={props.filesystemMemoryContextFilesCount}
            filesystemMemoryAdjustedFilesCount={props.filesystemMemoryAdjustedFilesCount}
            deletedRecoveryExport={props.deletedRecoveryExport}
            carvedExport={props.carvedExport}
            raidImportedSource={props.raidImportedSource}
            raidAnalysisPath={props.raidAnalysisPath}
          />
        )}

        {currentStep === 1 && (
          <ExportStepOptions
            destination={props.destination}
            setDestination={props.setDestination}
            conflictStrategy={props.conflictStrategy}
            setConflictStrategy={props.setConflictStrategy}
            preserveStructure={props.preserveStructure}
            setPreserveStructure={props.setPreserveStructure}
            verifyIntegrity={props.verifyIntegrity}
            setVerifyIntegrity={props.setVerifyIntegrity}
            isSafe={props.isSafe}
            validationMsg={props.validationMsg}
            availableSpace={props.availableSpace}
            onValidateDestination={props.onValidateDestination}
            validating={props.validating}
            exportValidationAvailable={props.exportValidationAvailable}
            requiredBytes={props.requiredBytes}
            fileCount={props.files.length}
          />
        )}

        {currentStep === 2 && (
          <ExportStepConfirm
            files={props.files}
            implicitPreviewFirstExcludedCount={props.implicitPreviewFirstExcludedCount}
            destination={props.destination}
            conflictStrategy={props.conflictStrategy}
            preserveStructure={props.preserveStructure}
            verifyIntegrity={props.verifyIntegrity}
            requiredBytes={props.requiredBytes}
            apfsExportOperatorVisible={props.apfsExportOperatorVisible}
            apfsCurrentCatalogFilesCount={props.apfsCurrentCatalogFilesCount}
            apfsReassembledFilesCount={props.apfsReassembledFilesCount}
            snapshotFilesCount={props.snapshotFilesCount}
            journalFilesCount={props.journalFilesCount}
            filesystemMemoryContextFilesCount={props.filesystemMemoryContextFilesCount}
            filesystemMemoryAdjustedFilesCount={props.filesystemMemoryAdjustedFilesCount}
            raidImportedSource={props.raidImportedSource}
            raidAnalysisPath={props.raidAnalysisPath}
            onStartExport={props.onStartExport}
            exportEngineAvailable={props.exportEngineAvailable}
            error={props.error}
          />
        )}

        {currentStep === 3 && (
          <ExportStepProgress
            exportProgress={props.exportProgress}
            exportLogs={props.exportLogs}
            exportPercent={props.exportPercent}
            exportRunning={props.exportRunning}
            activeExportId={props.activeExportId}
            logsError={props.logsError}
          />
        )}
      </div>

      {currentStep < 3 && (
        <div className="flex justify-between mt-6">
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => setCurrentStep(Math.max(0, currentStep - 1))}
            disabled={currentStep === 0}
          >
            {t('export.wizard.back')}
          </button>
          {currentStep < 2 ? (
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => setCurrentStep(currentStep + 1)}
              disabled={!canNext()}
            >
              {t('export.wizard.next')}
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-primary btn-lg"
              onClick={props.onStartExport}
              disabled={!props.exportEngineAvailable}
            >
              {t('export.wizard.start_export')}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
