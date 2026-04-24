// "Remote agents" section for the Devices page.
//
// V1 surface:
//   - Form to register a new agent (label, base URL, bearer token).
//     `add_remote_agent` probes `/v1/health` and fails fast on bad URL/token.
//     Plain `http://` against a non-loopback host is rejected backend-side by
//     `validate_remote_base_url` (see src-tauri/src/remote/commands.rs).
//   - TLS warning banner + "I understand the risk" checkbox whenever the Base
//     URL field is not HTTPS and not a loopback address.
//   - List of registered agents.
//   - Per-agent device list with a "Start scan" launcher that picks a scan
//     type, fires `remote_start_scan`, stores the resulting scan id and the
//     remote agent id in the global app store, then navigates to `/scan` so
//     the existing Scan/Results pages take over the live progress + restore
//     UX through the dispatcher functions in `hooks/ipc/remote.ts`.

import { isTauri } from '@tauri-apps/api/core';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import {
  type RemoteAgentSummary,
  addRemoteAgent,
  listRemoteAgents,
  remoteGetDevices,
  remoteStartScan,
  removeRemoteAgent,
} from '../../hooks/useIpc';
import { useAppStore } from '../../stores/appStore';
import type { DetectedDevice, ScanType } from '../../types';
import { WarningBanner } from '../common/WarningBanner';

interface FormState {
  label: string;
  baseUrl: string;
  token: string;
  riskAcknowledged: boolean;
}

const EMPTY_FORM: FormState = {
  label: '',
  baseUrl: 'http://127.0.0.1:7878',
  token: '',
  riskAcknowledged: false,
};

// Mirror of the backend `is_loopback_host` check so we can show the warning
// banner without a round-trip. The authoritative enforcement lives in Rust;
// this is purely a UX hint.
function isLoopbackUrl(url: string): boolean {
  const trimmed = url.trim().toLowerCase();
  if (!trimmed.startsWith('http://') && !trimmed.startsWith('https://')) return true;
  const afterScheme = trimmed.replace(/^https?:\/\//, '');
  const hostEnd = afterScheme.search(/[:/?]/);
  const host = hostEnd >= 0 ? afterScheme.slice(0, hostEnd) : afterScheme;
  const unbracketed = host.replace(/^\[|\]$/g, '');
  if (unbracketed === '127.0.0.1' || unbracketed === '::1' || unbracketed === 'localhost') {
    return true;
  }
  return unbracketed.startsWith('127.') || unbracketed === '0:0:0:0:0:0:0:1';
}

function needsTlsWarning(url: string): boolean {
  const trimmed = url.trim().toLowerCase();
  if (!trimmed.startsWith('http://')) return false;
  return !isLoopbackUrl(trimmed);
}

export function RemoteAgentsSection() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const setActiveScanId = useAppStore((s) => s.setActiveScanId);
  const setActiveRemoteAgentId = useAppStore((s) => s.setActiveRemoteAgentId);
  const trackScan = useAppStore((s) => s.trackScan);
  const setScanConfig = useAppStore((s) => s.setScanConfig);
  const selectDevice = useAppStore((s) => s.selectDevice);
  const setScanProgress = useAppStore((s) => s.setScanProgress);
  const setRecoveryResult = useAppStore((s) => s.setRecoveryResult);
  const clearScanLogs = useAppStore((s) => s.clearScanLogs);

  const [agents, setAgents] = useState<RemoteAgentSummary[]>([]);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [devicesByAgent, setDevicesByAgent] = useState<Record<string, DetectedDevice[] | null>>({});
  const [agentError, setAgentError] = useState<Record<string, string | null>>({});
  const [loadingFor, setLoadingFor] = useState<Record<string, boolean>>({});
  const [scanType, setScanType] = useState<Record<string, ScanType>>({});
  const [startingScan, setStartingScan] = useState<string | null>(null);
  const browserPreviewRemoteUnavailable = __ALLOW_BROWSER_PREVIEW__ && !isTauri();

  const tlsWarning = useMemo(() => needsTlsWarning(form.baseUrl), [form.baseUrl]);
  const submitDisabled =
    browserPreviewRemoteUnavailable || submitting || (tlsWarning && !form.riskAcknowledged);

  const remoteScanTypes: { value: ScanType; labelKey: string }[] = useMemo(
    () => [
      { value: 'quick', labelKey: 'devices.remote_scan_type_quick' },
      { value: 'deep', labelKey: 'devices.remote_scan_type_deep' },
      { value: 'carving', labelKey: 'devices.remote_scan_type_carving' },
      { value: 'signature-carving', labelKey: 'devices.remote_scan_type_signature_carving' },
    ],
    [],
  );

  const refresh = useCallback(async () => {
    try {
      const list = await listRemoteAgents();
      setAgents(list);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    if (browserPreviewRemoteUnavailable) {
      setAgents([]);
      setError(null);
      return;
    }
    void refresh();
  }, [browserPreviewRemoteUnavailable, refresh]);

  const handleAdd = useCallback(
    async (event: React.FormEvent) => {
      event.preventDefault();
      setError(null);
      setSubmitting(true);
      try {
        await addRemoteAgent(form.label, form.baseUrl, form.token);
        setForm(EMPTY_FORM);
        await refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setSubmitting(false);
      }
    },
    [form, refresh],
  );

  const handleRemove = useCallback(
    async (agentId: string) => {
      try {
        await removeRemoteAgent(agentId);
        await refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [refresh],
  );

  const handleListDevices = useCallback(async (agentId: string) => {
    setLoadingFor((prev) => ({ ...prev, [agentId]: true }));
    setAgentError((prev) => ({ ...prev, [agentId]: null }));
    try {
      const devices = await remoteGetDevices(agentId);
      setDevicesByAgent((prev) => ({ ...prev, [agentId]: devices }));
    } catch (err) {
      setAgentError((prev) => ({
        ...prev,
        [agentId]: err instanceof Error ? err.message : String(err),
      }));
    } finally {
      setLoadingFor((prev) => ({ ...prev, [agentId]: false }));
    }
  }, []);

  const handleStartScan = useCallback(
    async (agentId: string, device: DetectedDevice) => {
      const chosen = scanType[`${agentId}:${device.id}`] ?? 'quick';
      setStartingScan(`${agentId}:${device.id}`);
      setAgentError((prev) => ({ ...prev, [agentId]: null }));
      try {
        const scanId = await remoteStartScan(agentId, device.id, chosen);
        selectDevice(null);
        setRecoveryResult(null);
        setScanProgress(null);
        clearScanLogs();
        setScanConfig({
          deviceId: device.id,
          scanType: chosen,
          targetFilesystems: [device.filesystem],
          enableCarving: chosen === 'carving' || chosen === 'signature-carving',
        });
        setActiveScanId(scanId);
        setActiveRemoteAgentId(agentId);
        trackScan({
          id: scanId,
          agentId,
          label: `${device.name} • ${chosen}`,
          scanType: chosen,
          startedAtMs: Date.now(),
        });
        navigate('/scan');
      } catch (err) {
        setAgentError((prev) => ({
          ...prev,
          [agentId]: err instanceof Error ? err.message : String(err),
        }));
      } finally {
        setStartingScan(null);
      }
    },
    [
      scanType,
      selectDevice,
      setActiveRemoteAgentId,
      setActiveScanId,
      setRecoveryResult,
      setScanConfig,
      setScanProgress,
      clearScanLogs,
      trackScan,
      navigate,
    ],
  );

  return (
    <section className="remote-agents-section">
      <h2>{t('devices.remote_agents_title')}</h2>
      <p className="remote-agents-description">{t('devices.remote_agents_description')}</p>

      {browserPreviewRemoteUnavailable && (
        <div style={{ marginBottom: 'var(--space-4)' }}>
          <WarningBanner variant="warning">
            {t('devices.remote_browser_preview_unavailable')}
          </WarningBanner>
        </div>
      )}

      <form onSubmit={handleAdd} className="remote-agents-form">
        <label>
          {t('devices.remote_label_field')}
          <input
            type="text"
            value={form.label}
            disabled={browserPreviewRemoteUnavailable}
            onChange={(event) => setForm({ ...form, label: event.target.value })}
            placeholder={t('devices.remote_label_placeholder')}
            required
          />
        </label>
        <label>
          {t('devices.remote_url_field')}
          <input
            type="url"
            value={form.baseUrl}
            disabled={browserPreviewRemoteUnavailable}
            onChange={(event) =>
              setForm((prev) => ({
                ...prev,
                baseUrl: event.target.value,
                // Re-arm the ack checkbox every time the URL changes so the
                // user can't accidentally keep it ticked while switching hosts.
                riskAcknowledged: false,
              }))
            }
            placeholder={t('devices.remote_url_placeholder')}
            required
          />
        </label>
        <label>
          {t('devices.remote_token_field')}
          <input
            type="password"
            value={form.token}
            disabled={browserPreviewRemoteUnavailable}
            onChange={(event) => setForm({ ...form, token: event.target.value })}
            placeholder={t('devices.remote_token_placeholder')}
            required
          />
        </label>

        {tlsWarning && (
          <div className="remote-agents-tls-banner" role="alert" data-testid="remote-tls-warning">
            <strong>{t('devices.remote_tls_warning_title')}</strong>
            <span>{t('devices.remote_tls_warning_body')}</span>
            <label>
              <input
                type="checkbox"
                checked={form.riskAcknowledged}
                onChange={(event) =>
                  setForm((prev) => ({ ...prev, riskAcknowledged: event.target.checked }))
                }
              />{' '}
              {t('devices.remote_tls_ack_checkbox')}
            </label>
          </div>
        )}

        <button type="submit" disabled={submitDisabled}>
          {submitting ? t('devices.remote_add_probing') : t('devices.remote_add_submit')}
        </button>
        {error && <div className="remote-agents-error">{error}</div>}
      </form>

      <ul className="remote-agents-list">
        {agents.length === 0 && (
          <li className="remote-agents-list-empty">{t('devices.remote_empty')}</li>
        )}
        {agents.map((agent) => {
          const devices = devicesByAgent[agent.id];
          return (
            <li key={agent.id} className="remote-agents-card">
              <div className="remote-agents-card__header">
                <div>
                  <strong>{agent.label}</strong>
                  <div className="remote-agents-card__url">{agent.base_url}</div>
                </div>
                <div className="remote-agents-card__actions">
                  <button
                    type="button"
                    onClick={() => handleListDevices(agent.id)}
                    disabled={loadingFor[agent.id]}
                  >
                    {loadingFor[agent.id]
                      ? t('devices.remote_refresh_loading')
                      : t('devices.remote_refresh_button')}
                  </button>
                  <button type="button" onClick={() => handleRemove(agent.id)}>
                    {t('devices.remote_remove_button')}
                  </button>
                </div>
              </div>
              {agentError[agent.id] && (
                <div className="remote-agents-card__error">{agentError[agent.id]}</div>
              )}
              {devices && devices.length === 0 && (
                <div className="remote-agents-card__empty">{t('devices.remote_no_devices')}</div>
              )}
              {devices && devices.length > 0 && (
                <ul className="remote-agents-device-list">
                  {devices.map((device) => {
                    const key = `${agent.id}:${device.id}`;
                    const chosen = scanType[key] ?? 'quick';
                    const isStarting = startingScan === key;
                    return (
                      <li key={key} className="remote-agents-device-row">
                        <div className="remote-agents-device-row__info">
                          <div className="remote-agents-device-row__name">{device.name}</div>
                          <div className="remote-agents-device-row__meta">
                            {device.devicePath} • {device.filesystem.toUpperCase()} •{' '}
                            {(device.capacityBytes / 1024 / 1024 / 1024).toFixed(1)} GB
                          </div>
                        </div>
                        <div className="remote-agents-device-row__controls">
                          <select
                            value={chosen}
                            onChange={(event) =>
                              setScanType((prev) => ({
                                ...prev,
                                [key]: event.target.value as ScanType,
                              }))
                            }
                          >
                            {remoteScanTypes.map((option) => (
                              <option key={option.value} value={option.value}>
                                {t(option.labelKey)}
                              </option>
                            ))}
                          </select>
                          <button
                            type="button"
                            onClick={() => handleStartScan(agent.id, device)}
                            disabled={isStarting}
                          >
                            {isStarting
                              ? t('devices.remote_start_starting')
                              : t('devices.remote_start_button')}
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
