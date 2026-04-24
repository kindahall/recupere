import { invoke } from '@tauri-apps/api/core';

/**
 * Model variants exposed by the Rust backend. The kebab-case values match the
 * `#[serde(rename_all = "kebab-case")]` enum in `cloud_ai/mod.rs`. Keep this
 * list in lockstep with `GemmaModelVariant::all()` so settings never get a
 * value the frontend can't render.
 */
export type GemmaModelVariant = 'gemma4-e4b' | 'gemma3-4b' | 'gemma2-2b';

export const GEMMA_MODEL_VARIANTS: readonly GemmaModelVariant[] = [
  'gemma4-e4b',
  'gemma3-4b',
  'gemma2-2b',
];

export interface GemmaSettings {
  enabled: boolean;
  /** Optional override for the Ollama endpoint. Empty/undefined uses default. */
  endpoint?: string;
  variant: GemmaModelVariant;
}

export interface GemmaStatus {
  ollamaReachable: boolean;
  modelInstalled: boolean;
  ready: boolean;
  endpoint: string;
  model: string;
  variant: GemmaModelVariant;
  lowMemoryWarning?: string;
  message: string;
}

export interface GemmaRegistryProbe {
  variant: GemmaModelVariant;
  model: string;
  reachable: boolean;
  httpStatus?: number;
  message: string;
}

export interface GemmaPullProgress {
  inProgress: boolean;
  status: string;
  completedBytes: number;
  totalBytes: number;
  error?: string;
  finished: boolean;
}

export async function fetchGemmaSettings(): Promise<GemmaSettings> {
  const s = await invoke<{
    enabled: boolean;
    endpoint: string | null;
    variant: GemmaModelVariant;
  }>('get_gemma_settings');
  return {
    enabled: s.enabled,
    endpoint: s.endpoint ?? undefined,
    variant: s.variant,
  };
}

export async function saveGemmaSettings(
  enabled: boolean,
  endpoint?: string,
  variant?: GemmaModelVariant,
): Promise<void> {
  await invoke('save_gemma_settings', {
    enabled,
    endpoint: endpoint && endpoint.trim() !== '' ? endpoint.trim() : null,
    variant: variant ?? null,
  });
}

export async function probeGemmaRegistry(variant: GemmaModelVariant): Promise<GemmaRegistryProbe> {
  const r = await invoke<{
    variant: GemmaModelVariant;
    model: string;
    reachable: boolean;
    http_status: number | null;
    message: string;
  }>('probe_gemma_registry', { variant });
  return {
    variant: r.variant,
    model: r.model,
    reachable: r.reachable,
    httpStatus: r.http_status ?? undefined,
    message: r.message,
  };
}

export async function fetchGemmaMemoryAdvisory(
  variant: GemmaModelVariant,
): Promise<string | undefined> {
  const result = await invoke<string | null>('get_gemma_memory_advisory', { variant });
  return result ?? undefined;
}

export async function startGemmaPull(): Promise<void> {
  await invoke('start_gemma_pull');
}

export async function fetchGemmaPullProgress(): Promise<GemmaPullProgress> {
  const r = await invoke<{
    in_progress: boolean;
    status: string;
    completed_bytes: number;
    total_bytes: number;
    error: string | null;
    finished: boolean;
  }>('get_gemma_pull_progress');
  return {
    inProgress: r.in_progress,
    status: r.status,
    completedBytes: r.completed_bytes,
    totalBytes: r.total_bytes,
    error: r.error ?? undefined,
    finished: r.finished,
  };
}

export async function fetchGemmaStatus(): Promise<GemmaStatus> {
  const s = await invoke<{
    ollama_reachable: boolean;
    model_installed: boolean;
    ready: boolean;
    endpoint: string;
    model: string;
    variant: GemmaModelVariant;
    low_memory_warning: string | null;
    message: string;
  }>('get_gemma_status');
  return {
    ollamaReachable: s.ollama_reachable,
    modelInstalled: s.model_installed,
    ready: s.ready,
    endpoint: s.endpoint,
    model: s.model,
    variant: s.variant,
    lowMemoryWarning: s.low_memory_warning ?? undefined,
    message: s.message,
  };
}
