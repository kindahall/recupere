import { useTranslation } from 'react-i18next';
import type { DiagnosticData } from '../../hooks/useIpc';
import type { PotentialVolume } from '../../types';
import { isPotentialVolumeRecoverySupported } from '../../utils/potentialVolumeRecovery';
import { ErrorState } from '../common/ErrorState';
import { SectionCard } from '../common/SectionCard';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

export interface ExpertPotentialVolumesPanelProps {
  diagnostic: DiagnosticData | null;
  diagnosticError: string | null;
  orderedPotentialVolumes: PotentialVolume[];
  recommendedPotentialVolumeId: string | null | undefined;
  startingPotentialVolumeId: string | null;
  onAnalyzeCandidate: (volume: PotentialVolume) => void;
}

export function ExpertPotentialVolumesPanel({
  diagnostic,
  diagnosticError,
  orderedPotentialVolumes,
  recommendedPotentialVolumeId,
  startingPotentialVolumeId,
  onAnalyzeCandidate,
}: ExpertPotentialVolumesPanelProps) {
  const { t } = useTranslation();

  return (
    <details>
      <summary className="text-sm font-semibold mb-2" style={{ cursor: 'pointer' }}>
        {t('expert.potential_volumes')}
      </summary>
      <SectionCard title={t('expert.potential_volumes')}>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--font-size-xs)' }}>
          {diagnosticError ? (
            <ErrorState title={t('common.error')} description={diagnosticError} />
          ) : diagnostic?.potentialVolumes.length ? (
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ borderBottom: '1px solid var(--color-border)' }}>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.pv_label')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.partition_fs')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'left',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.detection_method')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.pv_offset')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.detected_size')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.confidence')}
                  </th>
                  <th
                    style={{
                      padding: '4px 8px',
                      textAlign: 'right',
                      color: 'var(--color-text-muted)',
                    }}
                  >
                    {t('expert.action')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {orderedPotentialVolumes.map((volume) => {
                  const supportedFilesystem = isPotentialVolumeRecoverySupported(volume.filesystem);
                  const recommendedCandidate = volume.id === recommendedPotentialVolumeId;

                  return (
                    <tr
                      key={volume.id}
                      style={{
                        borderBottom: '1px solid var(--color-border-light)',
                        verticalAlign: 'top',
                      }}
                    >
                      <td style={{ padding: '6px 8px' }}>
                        <div className="flex items-center gap-2">
                          <div className="font-medium">{volume.label}</div>
                          {recommendedCandidate && (
                            <span className="badge badge-risk-low">
                              {t('diagnostic.recommended')}
                            </span>
                          )}
                        </div>
                        {volume.notes.length > 0 && (
                          <div className="text-secondary mt-1" style={{ lineHeight: 1.6 }}>
                            {volume.notes.join(' ')}
                          </div>
                        )}
                      </td>
                      <td style={{ padding: '6px 8px' }}>
                        <span className="text-accent">{volume.filesystem.toUpperCase()}</span>
                      </td>
                      <td style={{ padding: '6px 8px' }}>{volume.detectionMethod}</td>
                      <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                        0x{volume.startOffset.toString(16).toUpperCase()}
                      </td>
                      <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                        {volume.sizeBytes ? formatBytes(volume.sizeBytes) : t('common.unknown')}
                      </td>
                      <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                        {volume.confidenceScore}%
                      </td>
                      <td style={{ padding: '6px 8px', textAlign: 'right' }}>
                        <button
                          type="button"
                          className="btn btn-secondary btn-sm"
                          disabled={!supportedFilesystem || startingPotentialVolumeId === volume.id}
                          onClick={() => {
                            if (!supportedFilesystem) return;
                            onAnalyzeCandidate(volume);
                          }}
                        >
                          {supportedFilesystem
                            ? startingPotentialVolumeId === volume.id
                              ? t('expert.analyzing_candidate')
                              : t('expert.analyze_candidate')
                            : t('expert.candidate_unsupported')}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          ) : (
            <div className="flex flex-col gap-3">
              <span className="text-muted">
                {diagnostic?.potentialVolumesNotice ??
                  (diagnostic?.potentialVolumesInspected
                    ? t('expert.no_potential_volumes')
                    : t('expert.potential_volumes_unavailable'))}
              </span>
            </div>
          )}
        </div>
      </SectionCard>
    </details>
  );
}
