import { invoke } from '@tauri-apps/api/core';

export async function startImaging(
  deviceId: string,
  destinationPath: string,
  rescueMapPath?: string,
): Promise<string> {
  return invoke<string>('start_imaging', { deviceId, destinationPath, rescueMapPath });
}
