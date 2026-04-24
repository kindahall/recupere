import { isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';
import { SectionCard } from '../components/common/SectionCard';
import { WarningBanner } from '../components/common/WarningBanner';
import { PageHeader } from '../components/layout/PageHeader';
import {
  GEMMA_MODEL_VARIANTS,
  type GemmaModelVariant,
  type GemmaPullProgress,
  type GemmaRegistryProbe,
  type GemmaStatus,
  activateLicense,
  clearRecentTraces,
  deactivateLicense,
  fetchAppBuildInfo,
  fetchFilesystemMemoryPolicy,
  fetchGemmaMemoryAdvisory,
  fetchGemmaPullProgress,
  fetchGemmaSettings,
  fetchGemmaStatus,
  fetchRecentTraces,
  generateLabBundle,
  getLicenseStatus,
  getMachineFingerprint,
  probeGemmaRegistry,
  saveFilesystemMemoryPolicy,
  saveGemmaSettings,
  saveSupportBundle,
  startGemmaPull,
  verifyAuditTrail,
} from '../hooks/useIpc';
import i18n from '../i18n';
import { useAppStore } from '../stores/appStore';
import type { AppBuildInfo, AuditChainReport, MonitoringPolicy, TraceEntry } from '../types';
import { MIN_MONITORING_INTERVAL_MINUTES } from '../types/filesystemMemory';

export function SettingsPage() {
  const { t } = useTranslation();
  const [searchParams] = useSearchParams();
  // The Expert route guard redirects here with `?from=expert` when a novice
  // tries to reach `/expert`. We use that signal to surface an explanatory
  // banner rather than silently bouncing them.
  const redirectedFromExpert = useMemo(() => searchParams.get('from') === 'expert', [searchParams]);
  const {
    theme,
    setTheme,
    language,
    setLanguage,
    userMode,
    setUserMode,
    devices,
    selectedDeviceId,
    setLicenseStatus,
  } = useAppStore();
  const [buildInfo, setBuildInfo] = useState<AppBuildInfo | null>(null);
  const [, setBuildInfoLoading] = useState(true);
  const [gemmaEnabled, setGemmaEnabled] = useState(true);
  const [gemmaEndpoint, setGemmaEndpoint] = useState('');
  const [gemmaVariant, setGemmaVariant] = useState<GemmaModelVariant>('gemma4-e4b');
  const [gemmaStatus, setGemmaStatus] = useState<GemmaStatus | null>(null);
  const [gemmaSaving, setGemmaSaving] = useState(false);
  const [gemmaNotice, setGemmaNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [gemmaRegistryProbe, setGemmaRegistryProbe] = useState<GemmaRegistryProbe | null>(null);
  const [gemmaProbeLoading, setGemmaProbeLoading] = useState(false);
  const [gemmaMemoryAdvisory, setGemmaMemoryAdvisory] = useState<string | null>(null);
  const [pullProgress, setPullProgress] = useState<GemmaPullProgress | null>(null);
  const pullPollRef = useRef<number | null>(null);
  const [policy, setPolicy] = useState<MonitoringPolicy>({ mode: 'manual' });
  const [policyInterval, setPolicyInterval] = useState<number>(MIN_MONITORING_INTERVAL_MINUTES);
  const [policySaving, setPolicySaving] = useState(false);
  const [policyNotice, setPolicyNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [supportBundleExporting, setSupportBundleExporting] = useState(false);
  const [supportBundleNotice, setSupportBundleNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [labBundleExporting, setLabBundleExporting] = useState(false);
  const [labBundleNotice, setLabBundleNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [licenseInfo, setLicenseInfo] = useState<Awaited<
    ReturnType<typeof getLicenseStatus>
  > | null>(null);
  const [licenseKeyInput, setLicenseKeyInput] = useState('');
  const [licenseNotice, setLicenseNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [licenseLoading, setLicenseLoading] = useState(false);
  const [machineFingerprint, setMachineFingerprint] = useState<string | null>(null);
  const [auditReport, setAuditReport] = useState<AuditChainReport | null>(null);
  const [auditChecking, setAuditChecking] = useState(false);
  const [diagnosticsNotice, setDiagnosticsNotice] = useState<{
    variant: 'success' | 'warning';
    message: string;
  } | null>(null);
  const [recentTraces, setRecentTraces] = useState<TraceEntry[]>([]);
  const [tracesLoading, setTracesLoading] = useState(false);
  const [tracesClearing, setTracesClearing] = useState(false);
  const selectedDevice = devices.find((device) => device.id === selectedDeviceId);
  const fallbackAppVersion = __APP_VERSION__;
  const formatTimestamp = useCallback(
    (timestampMs: number) =>
      new Intl.DateTimeFormat(language === 'fr' ? 'fr-FR' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'medium',
      }).format(new Date(timestampMs)),
    [language],
  );

  useEffect(() => {
    let cancelled = false;

    const loadBuildInfo = async () => {
      setBuildInfoLoading(true);
      try {
        const info = await fetchAppBuildInfo();
        if (!cancelled) {
          setBuildInfo(info);
        }
      } catch {
        if (!cancelled) {
          setBuildInfo(null);
        }
      } finally {
        if (!cancelled) {
          setBuildInfoLoading(false);
        }
      }
    };

    loadBuildInfo();

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadLicense = async () => {
      try {
        const [info, fingerprint] = await Promise.all([
          getLicenseStatus(),
          getMachineFingerprint().catch(() => null),
        ]);
        if (cancelled) {
          return;
        }
        setLicenseInfo(info);
        setMachineFingerprint(fingerprint);
      } catch {
        if (!cancelled) {
          setLicenseInfo(null);
          setMachineFingerprint(null);
        }
      }
    };
    loadLicense();
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshGemmaStatus = useCallback(async () => {
    try {
      const status = await fetchGemmaStatus();
      setGemmaStatus(status);
    } catch {
      setGemmaStatus(null);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadGemma = async () => {
      try {
        const s = await fetchGemmaSettings();
        if (!cancelled) {
          setGemmaEnabled(s.enabled);
          setGemmaEndpoint(s.endpoint ?? '');
          setGemmaVariant(s.variant);
        }
        const status = await fetchGemmaStatus();
        if (!cancelled) {
          setGemmaStatus(status);
        }
      } catch {
        // Backend not reachable yet — keep defaults
      }
    };
    loadGemma();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadMemoryAdvisory = async () => {
      try {
        const advisory = await fetchGemmaMemoryAdvisory(gemmaVariant);
        if (!cancelled) {
          setGemmaMemoryAdvisory(advisory ?? null);
        }
      } catch {
        if (!cancelled) {
          setGemmaMemoryAdvisory(null);
        }
      }
    };
    loadMemoryAdvisory();
    return () => {
      cancelled = true;
    };
  }, [gemmaVariant]);

  useEffect(() => {
    let cancelled = false;
    const loadPolicy = async () => {
      try {
        const persisted = await fetchFilesystemMemoryPolicy();
        if (cancelled) {
          return;
        }
        setPolicy(persisted);
        if (persisted.mode === 'scheduled') {
          setPolicyInterval(persisted.intervalMinutes);
        }
      } catch {
        // Backend not reachable (browser preview) — keep Manual default so the
        // toggle renders cleanly without pretending a policy is active.
      }
    };
    loadPolicy();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSavePolicy = async () => {
    setPolicyNotice(null);
    setPolicySaving(true);
    try {
      const clampedInterval = Math.max(policyInterval, MIN_MONITORING_INTERVAL_MINUTES);
      const nextPolicy: MonitoringPolicy =
        policy.mode === 'scheduled'
          ? { mode: 'scheduled', intervalMinutes: clampedInterval }
          : policy.mode === 'realtime'
            ? { mode: 'realtime' }
            : { mode: 'manual' };
      const saved = await saveFilesystemMemoryPolicy(nextPolicy);
      setPolicy(saved);
      if (saved.mode === 'scheduled') {
        setPolicyInterval(saved.intervalMinutes);
      }
      setPolicyNotice({
        variant: 'success',
        message: t('settings.filesystem_memory_policy_saved'),
      });
    } catch (error) {
      setPolicyNotice({
        variant: 'warning',
        message: t('settings.filesystem_memory_policy_save_failed', {
          error: error instanceof Error ? error.message : String(error),
        }),
      });
    } finally {
      setPolicySaving(false);
    }
  };

  const stopPullPolling = useCallback(() => {
    if (pullPollRef.current !== null) {
      window.clearInterval(pullPollRef.current);
      pullPollRef.current = null;
    }
  }, []);

  useEffect(() => () => stopPullPolling(), [stopPullPolling]);

  const handleOpenOllamaSite = async () => {
    try {
      // Detect platform and route to the right download page so the user
      // doesn't have to pick the right installer themselves.
      const ua = typeof navigator !== 'undefined' ? navigator.userAgent.toLowerCase() : '';
      let target = 'https://ollama.com/download';
      if (ua.includes('mac')) target = 'https://ollama.com/download/mac';
      else if (ua.includes('win')) target = 'https://ollama.com/download/windows';
      else if (ua.includes('linux')) target = 'https://ollama.com/download/linux';
      await openUrl(target);
    } catch {
      // Opener plugin is unavailable in browser preview; nothing to do.
    }
  };

  // Auto-poll Gemma status while the user is on Settings and Ollama is not
  // yet reachable. Stops as soon as Ollama becomes reachable, so we don't
  // hammer the backend once the install is complete.
  useEffect(() => {
    if (!gemmaEnabled) return;
    if (gemmaStatus?.ollamaReachable) return;
    const id = window.setInterval(() => {
      void refreshGemmaStatus();
    }, 3000);
    return () => window.clearInterval(id);
  }, [gemmaEnabled, gemmaStatus?.ollamaReachable, refreshGemmaStatus]);

  const handleStartGemmaPull = async () => {
    setGemmaNotice({ variant: 'success', message: t('settings.gemma_pull_starting') });
    try {
      await startGemmaPull();
      // Begin polling progress every 750ms.
      stopPullPolling();
      pullPollRef.current = window.setInterval(async () => {
        try {
          const p = await fetchGemmaPullProgress();
          setPullProgress(p);
          if (p.finished) {
            stopPullPolling();
            await refreshGemmaStatus();
            if (p.error) {
              setGemmaNotice({
                variant: 'warning',
                message: `${t('settings.gemma_pull_failed')}: ${p.error}`,
              });
            } else {
              setGemmaNotice({
                variant: 'success',
                message: t('settings.gemma_pull_success'),
              });
            }
          }
        } catch {
          // Ignore transient poll errors; keep polling.
        }
      }, 750);
    } catch (err) {
      setGemmaNotice({
        variant: 'warning',
        message: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleProbeGemmaRegistry = async () => {
    setGemmaNotice(null);
    setGemmaProbeLoading(true);
    try {
      const probe = await probeGemmaRegistry(gemmaVariant);
      setGemmaRegistryProbe(probe);
      setGemmaNotice({
        variant: probe.reachable ? 'success' : 'warning',
        message: probe.message,
      });
    } catch (error) {
      setGemmaRegistryProbe(null);
      setGemmaNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.gemma_registry_probe_failed'),
      });
    } finally {
      setGemmaProbeLoading(false);
    }
  };

  const handleSaveGemmaSettings = async () => {
    setGemmaNotice(null);
    setGemmaSaving(true);
    try {
      await saveGemmaSettings(gemmaEnabled, gemmaEndpoint || undefined, gemmaVariant);
      setGemmaNotice({ variant: 'success', message: t('settings.gemma_saved') });
      await refreshGemmaStatus();
    } catch (error) {
      setGemmaNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.gemma_save_failed'),
      });
    } finally {
      setGemmaSaving(false);
    }
  };

  const handleLanguageChange = (lang: 'en' | 'fr') => {
    setLanguage(lang);
    i18n.changeLanguage(lang);
  };

  const refreshLicenseState = useCallback(async () => {
    try {
      const [info, fingerprint] = await Promise.all([
        getLicenseStatus(),
        getMachineFingerprint().catch(() => null),
      ]);
      setLicenseInfo(info);
      setMachineFingerprint(fingerprint);
      setLicenseStatus(info.valid ? 'pro' : 'free');
    } catch (error) {
      setLicenseNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.license_refresh_failed'),
      });
    }
  }, [setLicenseStatus, t]);

  const handleActivateLicense = async () => {
    const trimmed = licenseKeyInput.trim();
    if (trimmed.length === 0) {
      return;
    }
    setLicenseNotice(null);
    setLicenseLoading(true);
    try {
      const info = await activateLicense(trimmed);
      setLicenseInfo(info);
      setLicenseStatus(info.valid ? 'pro' : 'free');
      if (info.valid) {
        setLicenseKeyInput('');
        await refreshLicenseState();
        setLicenseNotice({
          variant: 'success',
          message: t('settings.license_activate_success'),
        });
      } else {
        setLicenseNotice({
          variant: 'warning',
          message: info.message,
        });
      }
    } catch (error) {
      setLicenseNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.license_activate_failed'),
      });
    } finally {
      setLicenseLoading(false);
    }
  };

  const handleDeactivateLicense = async () => {
    setLicenseNotice(null);
    setLicenseLoading(true);
    try {
      await deactivateLicense();
      setLicenseStatus('free');
      await refreshLicenseState();
      setLicenseNotice({
        variant: 'success',
        message: t('settings.license_deactivate_success'),
      });
    } catch (error) {
      setLicenseNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.license_deactivate_failed'),
      });
    } finally {
      setLicenseLoading(false);
    }
  };

  const handleSupportBundleExport = async () => {
    setSupportBundleNotice(null);

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setSupportBundleNotice({
        variant: 'warning',
        message: t('settings.support_bundle_unavailable'),
      });
      return;
    }

    setSupportBundleExporting(true);
    try {
      const destinationPath = await save({
        defaultPath: 'recupere-support-bundle.zip',
        filters: [{ name: 'ZIP archive', extensions: ['zip'] }],
      });

      if (!destinationPath) {
        setSupportBundleNotice({
          variant: 'warning',
          message: t('settings.support_bundle_cancelled'),
        });
        return;
      }

      await saveSupportBundle(destinationPath, selectedDevice?.devicePath);
      setSupportBundleNotice({
        variant: 'success',
        message: t('settings.support_bundle_success'),
      });
    } catch (error) {
      setSupportBundleNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.support_bundle_failed'),
      });
    } finally {
      setSupportBundleExporting(false);
    }
  };

  const handleLabBundleExport = async () => {
    setLabBundleNotice(null);

    if (!selectedDeviceId) {
      setLabBundleNotice({
        variant: 'warning',
        message: t('settings.lab_bundle_requires_device'),
      });
      return;
    }

    if (__ALLOW_BROWSER_PREVIEW__ && !isTauri()) {
      setLabBundleNotice({
        variant: 'warning',
        message: t('settings.lab_bundle_unavailable'),
      });
      return;
    }

    setLabBundleExporting(true);
    try {
      const selectedPath = await open({
        directory: true,
        multiple: false,
      });

      if (!selectedPath || Array.isArray(selectedPath)) {
        setLabBundleNotice({
          variant: 'warning',
          message: t('settings.lab_bundle_cancelled'),
        });
        return;
      }

      const outputPath = await generateLabBundle(selectedDeviceId, selectedPath);
      setLabBundleNotice({
        variant: 'success',
        message: t('settings.lab_bundle_success', { path: outputPath }),
      });
    } catch (error) {
      setLabBundleNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.lab_bundle_failed'),
      });
    } finally {
      setLabBundleExporting(false);
    }
  };

  const refreshDiagnostics = useCallback(async () => {
    setDiagnosticsNotice(null);
    setAuditChecking(true);
    setTracesLoading(true);
    try {
      const [report, traces] = await Promise.all([verifyAuditTrail(), fetchRecentTraces(50)]);
      setAuditReport(report);
      setRecentTraces(traces);
    } catch (error) {
      setDiagnosticsNotice({
        variant: 'warning',
        message: error instanceof Error ? error.message : t('settings.diagnostics_refresh_failed'),
      });
    } finally {
      setAuditChecking(false);
      setTracesLoading(false);
    }
  }, [t]);

  const handleClearRecentTraces = async () => {
    setDiagnosticsNotice(null);
    setTracesClearing(true);
    try {
      await clearRecentTraces();
      setRecentTraces([]);
      setDiagnosticsNotice({
        variant: 'success',
        message: t('settings.diagnostics_traces_cleared'),
      });
    } catch (error) {
      setDiagnosticsNotice({
        variant: 'warning',
        message:
          error instanceof Error ? error.message : t('settings.diagnostics_trace_clear_failed'),
      });
    } finally {
      setTracesClearing(false);
    }
  };

  useEffect(() => {
    void refreshDiagnostics();
  }, [refreshDiagnostics]);

  const auditStatusVariant =
    auditReport && auditReport.firstBrokenId === null && auditReport.total > 0
      ? 'success'
      : auditReport && auditReport.total === 0
        ? 'info'
        : 'warning';
  const lastTrace = recentTraces[0] ?? null;

  return (
    <div>
      <PageHeader title={t('settings.title')} subtitle={t('settings.subtitle')} />
      <div className="flex flex-col gap-6" style={{ maxWidth: 600 }}>
        <SectionCard title={t('settings.appearance')}>
          <div className="flex flex-col gap-4">
            <div>
              <div className="text-sm font-medium mb-2" style={{ display: 'block' }}>
                {t('settings.theme')}
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  data-testid="settings-theme-light"
                  className={`btn ${theme === 'light' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => setTheme('light')}
                >
                  {t('settings.theme_light')}
                </button>
                <button
                  type="button"
                  data-testid="settings-theme-dark"
                  className={`btn ${theme === 'dark' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => setTheme('dark')}
                >
                  {t('settings.theme_dark')}
                </button>
              </div>
            </div>
            <div>
              <div className="text-sm font-medium mb-2" style={{ display: 'block' }}>
                {t('settings.language')}
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  data-testid="settings-language-en"
                  className={`btn ${language === 'en' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => handleLanguageChange('en')}
                >
                  {t('settings.lang_en')}
                </button>
                <button
                  type="button"
                  data-testid="settings-language-fr"
                  className={`btn ${language === 'fr' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => handleLanguageChange('fr')}
                >
                  {t('settings.lang_fr')}
                </button>
              </div>
            </div>
            <div>
              <div className="text-sm font-medium mb-2" style={{ display: 'block' }}>
                {t('settings.mode')}
              </div>
              {redirectedFromExpert && userMode === 'novice' && (
                <div className="mb-3" data-testid="settings-expert-gate-banner">
                  <WarningBanner variant="info">
                    <strong>{t('settings.expert_gate_title')}</strong>
                    <div>{t('settings.expert_gate_desc')}</div>
                  </WarningBanner>
                </div>
              )}
              <div className="flex gap-2">
                <button
                  type="button"
                  data-testid="settings-mode-novice"
                  className={`btn ${userMode === 'novice' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => setUserMode('novice')}
                >
                  {t('mode.novice')}
                </button>
                <button
                  type="button"
                  data-testid="settings-mode-expert"
                  className={`btn ${userMode === 'expert' ? 'btn-primary' : 'btn-secondary'} btn-sm`}
                  onClick={() => setUserMode('expert')}
                >
                  {t('mode.expert')}
                </button>
              </div>
            </div>
          </div>
        </SectionCard>

        <SectionCard title={t('settings.gemma')}>
          <div className="flex flex-col gap-3">
            <p className="text-sm text-secondary" style={{ margin: 0 }}>
              {t('settings.gemma_intro')}
            </p>

            <div className="flex flex-col gap-2">
              <label
                htmlFor="settings-gemma-variant"
                className="text-sm font-medium"
                style={{ display: 'block' }}
              >
                {t('settings.gemma_variant_label')}
              </label>
              <select
                id="settings-gemma-variant"
                data-testid="settings-gemma-variant"
                className="input"
                value={gemmaVariant}
                onChange={(event) => {
                  setGemmaVariant(event.target.value as GemmaModelVariant);
                  setGemmaRegistryProbe(null);
                }}
              >
                {GEMMA_MODEL_VARIANTS.map((variant) => (
                  <option key={variant} value={variant}>
                    {t(`settings.gemma_variant_${variant}`)}
                  </option>
                ))}
              </select>
              <p className="text-sm text-secondary" style={{ margin: 0 }}>
                {t('settings.gemma_variant_help')}
              </p>
            </div>

            {gemmaMemoryAdvisory && (
              <WarningBanner variant="warning">{gemmaMemoryAdvisory}</WarningBanner>
            )}

            {gemmaStatus && (
              <WarningBanner variant={gemmaStatus.ready ? 'success' : 'warning'}>
                {gemmaStatus.ready
                  ? t('settings.gemma_status_ready')
                  : !gemmaEnabled
                    ? t('settings.gemma_status_disabled')
                    : !gemmaStatus.ollamaReachable
                      ? t('settings.gemma_status_no_ollama')
                      : !gemmaStatus.modelInstalled
                        ? t('settings.gemma_status_no_model')
                        : gemmaStatus.message}
              </WarningBanner>
            )}

            {gemmaStatus && !gemmaStatus.ready && gemmaEnabled && (
              <div
                className="flex flex-col gap-3"
                style={{
                  border: '1px solid var(--color-border)',
                  borderRadius: 8,
                  padding: 12,
                }}
              >
                <div className="text-sm font-medium">{t('settings.gemma_setup_title')}</div>

                {/* Step 1 — Install Ollama */}
                <div
                  style={{
                    opacity: gemmaStatus.ollamaReachable ? 0.5 : 1,
                  }}
                >
                  <div className="text-sm font-medium mb-1">
                    {gemmaStatus.ollamaReachable ? '✓ ' : ''}
                    {t('settings.gemma_step1_title')}
                  </div>
                  <div className="text-sm text-secondary mb-2">
                    {t('settings.gemma_step1_desc')}
                  </div>
                  {!gemmaStatus.ollamaReachable && (
                    <button
                      type="button"
                      data-testid="settings-gemma-install-ollama"
                      className="btn btn-secondary btn-sm"
                      onClick={handleOpenOllamaSite}
                    >
                      {t('settings.gemma_step1_button')}
                    </button>
                  )}
                </div>

                {/* Step 2 — Download Gemma */}
                {gemmaStatus.ollamaReachable && (
                  <div>
                    <div className="text-sm font-medium mb-1">
                      {t('settings.gemma_step2_title')}
                    </div>
                    <div className="text-sm text-secondary mb-2">
                      {t('settings.gemma_step2_desc', {
                        model: gemmaStatus.model || gemmaVariant,
                      })}
                    </div>

                    {pullProgress?.inProgress ? (
                      <div className="flex flex-col gap-1">
                        <div className="text-sm">
                          {t('settings.gemma_pull_in_progress')} — {pullProgress.status}
                        </div>
                        {pullProgress.totalBytes > 0 && (
                          <>
                            <div
                              style={{
                                height: 8,
                                background: 'var(--color-border)',
                                borderRadius: 4,
                                overflow: 'hidden',
                              }}
                            >
                              <div
                                style={{
                                  height: '100%',
                                  background: 'var(--color-accent)',
                                  width: `${Math.min(
                                    100,
                                    (pullProgress.completedBytes / pullProgress.totalBytes) * 100,
                                  ).toFixed(1)}%`,
                                  transition: 'width 0.3s',
                                }}
                              />
                            </div>
                            <div className="text-sm text-secondary">
                              {(pullProgress.completedBytes / 1024 / 1024).toFixed(0)} /{' '}
                              {(pullProgress.totalBytes / 1024 / 1024).toFixed(0)} MB
                            </div>
                          </>
                        )}
                      </div>
                    ) : (
                      <button
                        type="button"
                        data-testid="settings-gemma-pull"
                        className="btn btn-primary btn-sm"
                        onClick={handleStartGemmaPull}
                      >
                        {t('settings.gemma_step2_button')}
                      </button>
                    )}
                  </div>
                )}
              </div>
            )}

            <label className="flex items-center gap-2 text-sm">
              <input
                data-testid="settings-gemma-enabled"
                type="checkbox"
                checked={gemmaEnabled}
                onChange={(e) => setGemmaEnabled(e.target.checked)}
              />
              {t('settings.gemma_enable_label')}
            </label>

            {/* Advanced: remote Ollama endpoint. Hidden behind <details> to keep
                the settings panel friendly for non-technical users. The field
                is locked whenever Ollama is already running locally — there is
                no valid reason to point it elsewhere in that case, and changing
                it would only break the working setup. */}
            {(() => {
              const localOllamaRunning = gemmaStatus?.ollamaReachable ?? false;
              return (
                <details>
                  <summary
                    className="text-sm text-secondary"
                    style={{ cursor: 'pointer', userSelect: 'none' }}
                  >
                    {t('settings.gemma_advanced_summary')}
                  </summary>
                  <div className="flex flex-col gap-2 mt-3">
                    <label
                      htmlFor="settings-gemma-endpoint"
                      className="text-sm font-medium"
                      style={{ display: 'block' }}
                    >
                      {t('settings.gemma_endpoint_label')}
                    </label>
                    <p className="text-sm text-secondary" style={{ margin: 0 }}>
                      {t('settings.gemma_endpoint_help')}
                    </p>
                    <input
                      id="settings-gemma-endpoint"
                      data-testid="settings-gemma-endpoint"
                      type="text"
                      className="input input-sm"
                      style={{ width: '100%' }}
                      value={gemmaEndpoint}
                      onChange={(e) => setGemmaEndpoint(e.target.value)}
                      placeholder={t('settings.gemma_endpoint_placeholder')}
                      disabled={localOllamaRunning}
                    />
                    {localOllamaRunning && (
                      <p
                        className="text-sm text-secondary"
                        style={{
                          margin: 0,
                          fontStyle: 'italic',
                          opacity: 0.85,
                        }}
                      >
                        {t('settings.gemma_endpoint_locked')}
                      </p>
                    )}
                  </div>
                </details>
              );
            })()}

            <div className="flex gap-2">
              <button
                type="button"
                data-testid="settings-gemma-save"
                className="btn btn-primary btn-sm"
                onClick={handleSaveGemmaSettings}
                disabled={gemmaSaving}
              >
                {gemmaSaving ? t('common.saving') : t('common.save')}
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={handleProbeGemmaRegistry}
                disabled={gemmaSaving || gemmaProbeLoading}
              >
                {gemmaProbeLoading
                  ? t('settings.gemma_registry_probe_loading')
                  : t('settings.gemma_registry_probe')}
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={refreshGemmaStatus}
                disabled={gemmaSaving}
              >
                {t('settings.gemma_refresh')}
              </button>
            </div>

            {gemmaRegistryProbe && (
              <div
                style={{
                  border: '1px solid var(--color-border)',
                  borderRadius: 'var(--radius-md)',
                  padding: 'var(--space-3)',
                  background: 'var(--color-bg-secondary)',
                }}
              >
                <div className="text-sm font-medium">
                  {t('settings.gemma_registry_probe_result', {
                    model: gemmaRegistryProbe.model,
                  })}
                </div>
                <div className="text-sm text-secondary" style={{ marginTop: 'var(--space-1)' }}>
                  {gemmaRegistryProbe.message}
                </div>
                <div className="text-xs text-secondary" style={{ marginTop: 'var(--space-2)' }}>
                  {gemmaRegistryProbe.httpStatus
                    ? t('settings.gemma_registry_http_status', {
                        status: gemmaRegistryProbe.httpStatus,
                      })
                    : t('settings.gemma_registry_http_status_unknown')}
                </div>
              </div>
            )}

            {gemmaNotice && (
              <WarningBanner variant={gemmaNotice.variant}>{gemmaNotice.message}</WarningBanner>
            )}
          </div>
        </SectionCard>

        <details open>
          <summary className="text-sm text-secondary mb-3" style={{ cursor: 'pointer' }}>
            {t('settings.advanced')}
          </summary>
          <div className="flex flex-col gap-6">
            <SectionCard title={t('settings.filesystem_memory_policy')}>
              <div className="flex flex-col gap-3">
                <p className="text-sm text-secondary">
                  {t('settings.filesystem_memory_policy_description')}
                </p>
                <p className="text-xs text-secondary" style={{ fontStyle: 'italic' }}>
                  {t('settings.filesystem_memory_policy_disclaimer')}
                </p>

                <label className="text-sm font-medium" htmlFor="filesystem-memory-policy-mode">
                  {t('settings.filesystem_memory_policy_mode_label')}
                </label>
                <select
                  id="filesystem-memory-policy-mode"
                  data-testid="settings-filesystem-memory-policy-mode"
                  className="input"
                  value={policy.mode}
                  onChange={(event) => {
                    const nextMode = event.target.value as MonitoringPolicy['mode'];
                    if (nextMode === 'scheduled') {
                      setPolicy({ mode: 'scheduled', intervalMinutes: policyInterval });
                    } else if (nextMode === 'manual') {
                      setPolicy({ mode: 'manual' });
                    } else {
                      setPolicy({ mode: 'realtime' });
                    }
                  }}
                >
                  <option value="manual">
                    {t('settings.filesystem_memory_policy_mode_manual')}
                  </option>
                  <option value="scheduled">
                    {t('settings.filesystem_memory_policy_mode_scheduled')}
                  </option>
                  <option value="realtime">
                    {t('settings.filesystem_memory_policy_mode_realtime')}
                  </option>
                </select>

                {policy.mode === 'scheduled' && (
                  <div className="flex flex-col gap-1">
                    <label
                      className="text-sm font-medium"
                      htmlFor="filesystem-memory-policy-interval"
                    >
                      {t('settings.filesystem_memory_policy_interval_label', {
                        minimum: MIN_MONITORING_INTERVAL_MINUTES,
                      })}
                    </label>
                    <input
                      id="filesystem-memory-policy-interval"
                      data-testid="settings-filesystem-memory-policy-interval"
                      type="number"
                      min={MIN_MONITORING_INTERVAL_MINUTES}
                      step={5}
                      value={policyInterval}
                      onChange={(event) => {
                        const parsed = Number.parseInt(event.target.value, 10);
                        if (Number.isFinite(parsed)) {
                          setPolicyInterval(Math.max(parsed, MIN_MONITORING_INTERVAL_MINUTES));
                        }
                      }}
                      className="input"
                    />
                  </div>
                )}

                {policy.mode === 'realtime' && (
                  <WarningBanner variant="warning">
                    {t('settings.filesystem_memory_policy_realtime_disclaimer')}
                  </WarningBanner>
                )}

                <div>
                  <button
                    type="button"
                    className="btn btn-secondary"
                    data-testid="settings-filesystem-memory-policy-save"
                    onClick={handleSavePolicy}
                    disabled={policySaving}
                  >
                    {policySaving
                      ? t('settings.filesystem_memory_policy_saving')
                      : t('settings.filesystem_memory_policy_save')}
                  </button>
                </div>

                {policyNotice && (
                  <WarningBanner variant={policyNotice.variant}>
                    {policyNotice.message}
                  </WarningBanner>
                )}
              </div>
            </SectionCard>

            <SectionCard title={t('settings.support_bundle')}>
              <div className="flex flex-col gap-3">
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {t('settings.support_bundle_desc')}
                </p>
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {t('settings.support_bundle_contents')}
                </p>
                <button
                  type="button"
                  data-testid="settings-support-bundle-export"
                  className="btn btn-secondary"
                  onClick={handleSupportBundleExport}
                  disabled={supportBundleExporting}
                >
                  {supportBundleExporting
                    ? t('settings.support_bundle_exporting')
                    : t('settings.support_bundle_action')}
                </button>
                {supportBundleNotice && (
                  <WarningBanner variant={supportBundleNotice.variant}>
                    {supportBundleNotice.message}
                  </WarningBanner>
                )}
              </div>
            </SectionCard>

            <SectionCard title={t('settings.license')}>
              <div className="flex flex-col gap-3">
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {t('settings.license_desc')}
                </p>
                <WarningBanner variant={licenseInfo?.valid ? 'success' : 'info'}>
                  {licenseInfo
                    ? t('settings.license_status_value', {
                        status: t(`settings.license_status_${licenseInfo.status}`),
                      })
                    : t('settings.license_status_unknown')}
                  {licenseInfo?.message ? ` ${licenseInfo.message}` : ''}
                </WarningBanner>

                <div className="grid grid-2 gap-3">
                  <div className="stat-card">
                    <span className="stat-card-label">{t('settings.license_tier')}</span>
                    <span className="stat-card-value">{licenseInfo?.tier ?? '—'}</span>
                  </div>
                  <div className="stat-card">
                    <span className="stat-card-label">{t('settings.license_email')}</span>
                    <span className="stat-card-value" style={{ fontSize: 'var(--font-size-sm)' }}>
                      {licenseInfo?.email ?? '—'}
                    </span>
                  </div>
                </div>

                {machineFingerprint && (
                  <div>
                    <div className="text-sm font-medium" style={{ marginBottom: 'var(--space-1)' }}>
                      {t('settings.license_machine_fingerprint')}
                    </div>
                    <div
                      className="text-xs text-secondary"
                      style={{ fontFamily: 'var(--font-mono)', wordBreak: 'break-all' }}
                    >
                      {machineFingerprint}
                    </div>
                  </div>
                )}

                <div className="flex flex-col gap-2">
                  <label
                    htmlFor="settings-license-key"
                    className="text-sm font-medium"
                    style={{ display: 'block' }}
                  >
                    {t('settings.license_key_label')}
                  </label>
                  <div className="flex gap-2">
                    <input
                      id="settings-license-key"
                      data-testid="settings-license-key"
                      type="text"
                      className="input input-sm"
                      style={{
                        flex: 1,
                        fontFamily: 'var(--font-mono)',
                        fontSize: 'var(--font-size-xs)',
                      }}
                      value={licenseKeyInput}
                      onChange={(event) => setLicenseKeyInput(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          void handleActivateLicense();
                        }
                      }}
                      placeholder={t('settings.license_key_placeholder')}
                      disabled={licenseLoading}
                    />
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      onClick={handleActivateLicense}
                      disabled={licenseLoading || licenseKeyInput.trim().length === 0}
                    >
                      {licenseLoading
                        ? t('settings.license_working')
                        : t('settings.license_activate')}
                    </button>
                  </div>
                </div>

                <div className="flex gap-2">
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void refreshLicenseState()}
                    disabled={licenseLoading}
                  >
                    {t('settings.license_refresh')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={handleDeactivateLicense}
                    disabled={licenseLoading || !licenseInfo?.valid}
                  >
                    {t('settings.license_deactivate')}
                  </button>
                </div>

                {licenseNotice && (
                  <WarningBanner variant={licenseNotice.variant}>
                    {licenseNotice.message}
                  </WarningBanner>
                )}
              </div>
            </SectionCard>

            <SectionCard title={t('settings.lab_bundle')}>
              <div className="flex flex-col gap-3">
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {t('settings.lab_bundle_desc')}
                </p>
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {selectedDevice
                    ? t('settings.lab_bundle_device', {
                        name: selectedDevice.name,
                        path: selectedDevice.devicePath,
                      })
                    : t('settings.lab_bundle_no_device')}
                </p>
                <button
                  type="button"
                  data-testid="settings-lab-bundle-export"
                  className="btn btn-secondary"
                  onClick={handleLabBundleExport}
                  disabled={labBundleExporting || !selectedDeviceId}
                >
                  {labBundleExporting
                    ? t('settings.lab_bundle_exporting')
                    : t('settings.lab_bundle_action')}
                </button>
                {labBundleNotice && (
                  <WarningBanner variant={labBundleNotice.variant}>
                    {labBundleNotice.message}
                  </WarningBanner>
                )}
              </div>
            </SectionCard>

            <SectionCard title={t('settings.diagnostics')}>
              <div className="flex flex-col gap-3">
                <p className="text-sm text-secondary" style={{ margin: 0 }}>
                  {t('settings.diagnostics_desc')}
                </p>

                <WarningBanner variant={auditStatusVariant}>
                  {auditChecking
                    ? t('settings.diagnostics_checking')
                    : auditReport === null
                      ? t('settings.diagnostics_unavailable')
                      : auditReport.total === 0
                        ? t('settings.diagnostics_empty')
                        : auditReport.firstBrokenId === null
                          ? t('settings.diagnostics_chain_ok', { count: auditReport.ok })
                          : t('settings.diagnostics_chain_broken', {
                              id: auditReport.firstBrokenId,
                            })}
                </WarningBanner>

                <div className="grid grid-3 gap-3">
                  <div className="stat-card">
                    <span className="stat-card-label">
                      {t('settings.diagnostics_total_records')}
                    </span>
                    <span className="stat-card-value">{auditReport?.total ?? 0}</span>
                  </div>
                  <div className="stat-card">
                    <span className="stat-card-label">
                      {t('settings.diagnostics_verified_records')}
                    </span>
                    <span className="stat-card-value">{auditReport?.ok ?? 0}</span>
                  </div>
                  <div className="stat-card">
                    <span className="stat-card-label">
                      {t('settings.diagnostics_broken_records')}
                    </span>
                    <span className="stat-card-value">{auditReport?.broken ?? 0}</span>
                  </div>
                  <div className="stat-card">
                    <span className="stat-card-label">
                      {t('settings.diagnostics_recent_traces')}
                    </span>
                    <span className="stat-card-value">{recentTraces.length}</span>
                  </div>
                </div>

                {auditReport && auditReport.total > 0 && (
                  <div className="rounded-2xl border border-default bg-subtle px-4 py-3 text-xs text-secondary">
                    <div>
                      {t('settings.diagnostics_chain_tip_hash')}:{' '}
                      <code>{auditReport.chainTipHash}</code>
                    </div>
                    <div>
                      {t('settings.diagnostics_genesis_hash')}:{' '}
                      <code>{auditReport.genesisPrevHash}</code>
                    </div>
                  </div>
                )}

                <div className="flex gap-2">
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void refreshDiagnostics()}
                    disabled={auditChecking || tracesLoading}
                  >
                    {auditChecking || tracesLoading
                      ? t('settings.diagnostics_refreshing')
                      : t('settings.diagnostics_refresh')}
                  </button>
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={handleClearRecentTraces}
                    disabled={tracesClearing}
                  >
                    {tracesClearing
                      ? t('settings.diagnostics_clearing')
                      : t('settings.diagnostics_clear_traces')}
                  </button>
                </div>

                <div
                  style={{
                    border: '1px solid var(--color-border)',
                    borderRadius: 'var(--radius-md)',
                    padding: 'var(--space-3)',
                    background: 'var(--color-bg-secondary)',
                  }}
                >
                  <div className="text-sm font-medium" style={{ marginBottom: 'var(--space-2)' }}>
                    {t('settings.diagnostics_trace_feed')}
                  </div>
                  <div
                    className="text-xs text-secondary"
                    style={{ marginBottom: 'var(--space-3)' }}
                  >
                    {lastTrace
                      ? t('settings.diagnostics_last_trace', {
                          timestamp: formatTimestamp(lastTrace.timestampMs),
                        })
                      : t('settings.diagnostics_no_traces')}
                  </div>
                  <div
                    className="flex flex-col gap-2"
                    style={{ maxHeight: 240, overflowY: 'auto' }}
                  >
                    {recentTraces.length === 0 ? (
                      <div className="text-sm text-secondary">
                        {tracesLoading
                          ? t('settings.diagnostics_loading_traces')
                          : t('settings.diagnostics_no_traces')}
                      </div>
                    ) : (
                      recentTraces.map((trace, index) => (
                        <div
                          key={`${trace.timestampMs}-${trace.target}-${index}`}
                          style={{
                            borderBottom:
                              index === recentTraces.length - 1
                                ? 'none'
                                : '1px solid var(--color-border)',
                            paddingBottom: 'var(--space-2)',
                          }}
                        >
                          <div className="flex items-center gap-2 text-xs text-secondary">
                            <span>{formatTimestamp(trace.timestampMs)}</span>
                            <span
                              style={{
                                padding: '2px 6px',
                                borderRadius: 'var(--radius-full)',
                                background: 'var(--color-bg-tertiary)',
                                textTransform: 'uppercase',
                                letterSpacing: '0.04em',
                              }}
                            >
                              {trace.level}
                            </span>
                            <span>{trace.target}</span>
                          </div>
                          <div className="text-sm" style={{ marginTop: 'var(--space-1)' }}>
                            {trace.message}
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                {diagnosticsNotice && (
                  <WarningBanner variant={diagnosticsNotice.variant}>
                    {diagnosticsNotice.message}
                  </WarningBanner>
                )}
              </div>
            </SectionCard>

            <SectionCard title={t('settings.about')}>
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <span className="text-secondary text-sm">{t('settings.version')}</span>
                  <span className="font-semibold text-sm">
                    {buildInfo?.appVersion ?? fallbackAppVersion}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-secondary text-sm">{t('settings.platform')}</span>
                  <span className="text-sm">
                    {buildInfo
                      ? `${buildInfo.operatingSystem}/${buildInfo.architecture}`
                      : t('settings.build_info_unavailable_short')}
                  </span>
                </div>
              </div>
            </SectionCard>
          </div>
        </details>
      </div>
    </div>
  );
}
