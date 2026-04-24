import type { DetectedDevice } from '../types/device';
import type { ImagingProfile } from '../types/scan';

export function recommendedImagingProfileForDevice(
  device: Pick<DetectedDevice, 'status' | 'riskLevel'>,
): ImagingProfile {
  if (
    device.status === 'degraded' ||
    device.status === 'failing' ||
    device.status === 'unresponsive' ||
    device.riskLevel === 'high' ||
    device.riskLevel === 'critical'
  ) {
    return 'cautious';
  }

  return 'standard';
}

export function recommendedImagingProfileReasonKeyForDevice(
  device: Pick<DetectedDevice, 'status' | 'riskLevel'>,
): string {
  if (device.status === 'unresponsive' || device.status === 'failing') {
    return 'imaging.profile_reason_failing';
  }

  if (device.status === 'degraded') {
    return 'imaging.profile_reason_degraded';
  }

  if (device.riskLevel === 'critical' || device.riskLevel === 'high') {
    return 'imaging.profile_reason_risk';
  }

  return 'imaging.profile_reason_standard';
}
