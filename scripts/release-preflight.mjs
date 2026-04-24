import { spawnSync } from 'node:child_process';
import { access, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const reportPath = path.join(rootDir, 'dist', 'release-preflight.json');

const checks = [];

function parseArgs(argv) {
  return {
    requireLicensePubkey: argv.includes('--require-license-pubkey'),
  };
}

function isVersionTagRelease() {
  const gitRef = process.env.GITHUB_REF ?? '';
  return /^refs\/tags\/v.+/.test(gitRef);
}

// Allow an explicit local override — `RECUPERE_RELEASE=1 npm run release:preflight`
// — so developers can dry-run the strict release gate before pushing a tag.
// `GITHUB_REF=refs/tags/v...` continues to work for CI.
function isStrictReleaseGate() {
  if (isVersionTagRelease()) return true;
  const flag = process.env.RECUPERE_RELEASE ?? '';
  return flag === '1' || flag.toLowerCase() === 'true';
}

function addCheck(id, severity, passed, message, details) {
  checks.push({
    id,
    severity,
    status: passed ? 'pass' : 'fail',
    message,
    ...(details ? { details } : {}),
  });
}

async function readJsonFile(filePath) {
  const text = await readFile(filePath, 'utf8');
  return JSON.parse(text);
}

function normalizeTomlArray(rawValue) {
  return rawValue
    .split(',')
    .map((entry) => entry.trim().replace(/^"(.*)"$/, '$1'))
    .filter(Boolean);
}

function readTomlPackageField(tomlText, key) {
  const packageMatch = tomlText.match(/\[package\]([\s\S]*?)(?:\n\[|$)/);
  if (!packageMatch) {
    return null;
  }

  const section = packageMatch[1];
  const quotedMatch = section.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm'));
  if (quotedMatch) {
    return quotedMatch[1];
  }

  const arrayMatch = section.match(new RegExp(`^${key}\\s*=\\s*\\[(.*?)\\]`, 'm'));
  if (arrayMatch) {
    return normalizeTomlArray(arrayMatch[1]);
  }

  return null;
}

async function fileExists(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

function hasAnyEnv(keys) {
  return keys.some((key) => Boolean(process.env[key]));
}

function hasEveryEnv(keys) {
  return keys.every((key) => Boolean(process.env[key]));
}

function runNpmAudit() {
  const result = spawnSync('npm', ['audit', '--json'], {
    cwd: rootDir,
    encoding: 'utf8',
    shell: process.platform === 'win32',
  });
  const output = result.stdout || result.stderr || '{}';
  try {
    const report = JSON.parse(output);
    return {
      ok: (report.metadata?.vulnerabilities?.total ?? 1) === 0,
      vulnerabilities: report.metadata?.vulnerabilities ?? null,
    };
  } catch (error) {
    return {
      ok: false,
      vulnerabilities: null,
      error: `Unable to parse npm audit output: ${error}`,
    };
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const packageJsonPath = path.join(rootDir, 'package.json');
  const cargoTomlPath = path.join(rootDir, 'src-tauri', 'Cargo.toml');
  const tauriConfigPath = path.join(rootDir, 'src-tauri', 'tauri.conf.json');
  const bootableRescueDocPath = path.join(rootDir, 'docs', 'bootable-rescue-workflow.md');
  const releaseSecretsRunbookPath = path.join(rootDir, '.github', 'RELEASE_SECRETS.md');

  const [packageJson, tauriConfig, cargoTomlText] = await Promise.all([
    readJsonFile(packageJsonPath),
    readJsonFile(tauriConfigPath),
    readFile(cargoTomlPath, 'utf8'),
  ]);

  const cargoVersion = readTomlPackageField(cargoTomlText, 'version');
  const cargoDescription = readTomlPackageField(cargoTomlText, 'description');
  const cargoAuthors = readTomlPackageField(cargoTomlText, 'authors');

  addCheck(
    'version-consistency',
    'error',
    packageJson.version === tauriConfig.version && tauriConfig.version === cargoVersion,
    'Versions package.json / tauri.conf.json / Cargo.toml are aligned.',
    {
      packageJson: packageJson.version,
      tauriConfig: tauriConfig.version,
      cargoToml: cargoVersion,
    },
  );

  addCheck(
    'bundle-identifier',
    'error',
    typeof tauriConfig.identifier === 'string' &&
      tauriConfig.identifier.length > 0 &&
      tauriConfig.identifier.includes('.') &&
      !tauriConfig.identifier.endsWith('.app'),
    'Tauri bundle identifier is present, namespaced, and does not end with .app.',
    { identifier: tauriConfig.identifier },
  );

  addCheck(
    'product-name',
    'error',
    typeof tauriConfig.productName === 'string' && tauriConfig.productName.trim().length > 0,
    'Tauri product name is present.',
    { productName: tauriConfig.productName },
  );

  addCheck(
    'bundle-active',
    'error',
    tauriConfig.bundle?.active === true,
    'Tauri bundle generation is enabled.',
    { active: tauriConfig.bundle?.active ?? null },
  );

  const icons = Array.isArray(tauriConfig.bundle?.icon) ? tauriConfig.bundle.icon : [];
  const missingIcons = [];
  for (const relativeIconPath of icons) {
    const iconPath = path.join(rootDir, 'src-tauri', relativeIconPath);
    if (!(await fileExists(iconPath))) {
      missingIcons.push(relativeIconPath);
    }
  }

  addCheck(
    'bundle-icons',
    'error',
    missingIcons.length === 0 && icons.length > 0,
    'All Tauri bundle icons referenced in tauri.conf.json exist on disk.',
    {
      checked: icons,
      missing: missingIcons,
    },
  );

  const placeholderDescription = cargoDescription === 'A Tauri App';
  const placeholderAuthors =
    Array.isArray(cargoAuthors) && cargoAuthors.some((author) => author.toLowerCase() === 'you');

  addCheck(
    'cargo-metadata',
    'error',
    !placeholderDescription &&
      !placeholderAuthors &&
      typeof cargoDescription === 'string' &&
      cargoDescription.trim().length > 0 &&
      Array.isArray(cargoAuthors) &&
      cargoAuthors.length > 0,
    'Cargo package metadata is present and no longer uses placeholders.',
    {
      description: cargoDescription,
      authors: cargoAuthors,
    },
  );

  addCheck(
    'release-scripts',
    'error',
    typeof packageJson.scripts?.['release:preflight'] === 'string' &&
      typeof packageJson.scripts?.['release:build'] === 'string',
    'Release scripts are exposed through package.json.',
    {
      releasePreflight: packageJson.scripts?.['release:preflight'] ?? null,
      releaseBuild: packageJson.scripts?.['release:build'] ?? null,
    },
  );

  addCheck(
    'bootable-rescue-doc',
    'warning',
    await fileExists(bootableRescueDocPath),
    (await fileExists(bootableRescueDocPath))
      ? 'Bootable rescue MVP documentation is present.'
      : 'Bootable rescue workflow documentation is missing, so rescue readiness stays implicit.',
    {
      path: path.relative(rootDir, bootableRescueDocPath),
    },
  );

  addCheck(
    'release-secrets-runbook',
    'error',
    await fileExists(releaseSecretsRunbookPath),
    (await fileExists(releaseSecretsRunbookPath))
      ? 'Release secrets and signing runbook is present.'
      : 'Release secrets and signing runbook is missing, so production signing setup is not reproducible.',
    {
      path: path.relative(rootDir, releaseSecretsRunbookPath),
    },
  );

  const npmAudit = runNpmAudit();
  addCheck(
    'npm-audit',
    'error',
    npmAudit.ok,
    npmAudit.ok
      ? 'npm audit reports no known vulnerabilities in the installed dependency tree.'
      : 'npm audit reports known vulnerabilities in the installed dependency tree.',
    npmAudit,
  );

  const updaterConfig = tauriConfig.plugins?.updater;
  const updaterActive = updaterConfig?.active === true;
  const updaterPubkey =
    typeof updaterConfig?.pubkey === 'string' ? updaterConfig.pubkey.trim() : '';
  const updaterEndpoints = Array.isArray(updaterConfig?.endpoints)
    ? updaterConfig.endpoints.filter(
        (endpoint) => typeof endpoint === 'string' && endpoint.trim().length > 0,
      )
    : [];

  // Evaluated once up-front so every release-sensitive check below uses the
  // same notion of "strict mode" (tag build, or `RECUPERE_RELEASE=1` for local
  // dry-runs). Moved ahead of first use — a later refactor accidentally
  // referenced it above its declaration.
  const isTagRelease = isVersionTagRelease();
  const strictRelease = isStrictReleaseGate();
  const licensePubkey = process.env.RECUPERE_LICENSE_PUBKEY_HEX ?? '';
  const devPlaceholderPubkey =
    '3c53dd0a122c2b684148c1754f9462e54acb1c52cc1e1265ff3e3780d474b83c';
  const hasLicensePubkey = /^[0-9a-f]{64}$/.test(licensePubkey);
  const hasProductionLicensePubkey =
    hasLicensePubkey &&
    !/^0{64}$/.test(licensePubkey) &&
    licensePubkey !== devPlaceholderPubkey;

  addCheck(
    'license-public-key-env',
    strictRelease || args.requireLicensePubkey ? 'error' : 'warning',
    hasProductionLicensePubkey,
    strictRelease || args.requireLicensePubkey
      ? 'Release bundle builds require RECUPERE_LICENSE_PUBKEY_HEX with a real 64-char lowercase hex public key, not zero or the dev placeholder.'
      : hasProductionLicensePubkey
        ? 'License public key is present for release bundle compilation.'
        : 'No RECUPERE_LICENSE_PUBKEY_HEX detected. Generic preflight stays green, but release bundle compilation will be refused by src-tauri/build.rs.',
    {
      strictRelease,
      requireLicensePubkey: args.requireLicensePubkey,
      configured: hasLicensePubkey,
      productionKey: hasProductionLicensePubkey,
    },
  );

  addCheck(
    'updater-pubkey',
    'error',
    !updaterActive || updaterPubkey.length > 0,
    updaterActive
      ? 'Updater is active and a non-empty public key is configured.'
      : 'Updater is inactive, so an empty public key does not block the preflight.',
    {
      active: updaterActive,
      pubkeyConfigured: updaterPubkey.length > 0,
    },
  );

  // Strict release gate: a shipping build MUST have an active updater wired
  // to a real public key. Non-release preflights still stay green with the
  // updater turned off, so day-to-day local builds keep working.
  addCheck(
    'updater-active-for-release',
    strictRelease ? 'error' : 'warning',
    !strictRelease || (updaterActive && updaterPubkey.length > 0 && updaterEndpoints.length > 0),
    strictRelease
      ? 'Release builds require plugins.updater.active=true with a non-empty pubkey and at least one endpoint.'
      : 'Updater configuration is not enforced outside of release builds.',
    {
      strictRelease,
      active: updaterActive,
      pubkeyConfigured: updaterPubkey.length > 0,
      endpoints: updaterEndpoints,
    },
  );

  addCheck(
    'updater-endpoints-while-inactive',
    'warning',
    updaterActive || updaterEndpoints.length === 0,
    updaterActive
      ? 'Updater endpoints are configured for an active updater.'
      : updaterEndpoints.length === 0
        ? 'No updater endpoints are configured while the updater is inactive.'
        : 'Updater endpoints are configured even though the updater is inactive.',
    {
      active: updaterActive,
      endpoints: updaterEndpoints,
    },
  );

  const hasMacSigningInputs = hasAnyEnv([
    'APPLE_SIGNING_IDENTITY',
    'APPLE_CERTIFICATE',
    'APPLE_CERTIFICATE_PASSWORD',
    'APPLE_API_ISSUER',
    'APPLE_API_KEY',
    'APPLE_API_KEY_PATH',
    'APPLE_API_PRIVATE_KEY',
  ]);
  const hasUpdaterInputs = hasAnyEnv([
    'TAURI_SIGNING_PRIVATE_KEY',
    'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  ]);
  const hasCompleteAppleSigningConfig =
    hasEveryEnv([
      'APPLE_SIGNING_IDENTITY',
      'APPLE_CERTIFICATE',
      'APPLE_CERTIFICATE_PASSWORD',
      'APPLE_API_ISSUER',
      'APPLE_API_KEY',
    ]) && Boolean(process.env.APPLE_API_KEY_PATH || process.env.APPLE_API_PRIVATE_KEY);
  const hasCompleteUpdaterConfig = hasEveryEnv([
    'TAURI_SIGNING_PRIVATE_KEY',
    'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  ]);

  addCheck(
    'macos-signing-env',
    strictRelease ? 'error' : 'warning',
    strictRelease ? hasCompleteAppleSigningConfig : hasMacSigningInputs,
    strictRelease
      ? 'Release builds require a complete macOS signing/notarization configuration.'
      : hasMacSigningInputs
        ? 'At least one macOS signing/notarization input is configured in the environment.'
        : 'No macOS signing/notarization environment variables detected. Local preflight stays green, but notarized release is not yet configured.',
    strictRelease
      ? {
          required: [
            'APPLE_SIGNING_IDENTITY',
            'APPLE_CERTIFICATE',
            'APPLE_CERTIFICATE_PASSWORD',
            'APPLE_API_ISSUER',
            'APPLE_API_KEY',
            'APPLE_API_KEY_PATH|APPLE_API_PRIVATE_KEY',
          ],
          strictTrigger: isTagRelease ? 'git-tag' : 'RECUPERE_RELEASE',
        }
      : undefined,
  );

  const hasPartialAppleApiConfig =
    (process.env.APPLE_API_KEY ||
      process.env.APPLE_API_ISSUER ||
      process.env.APPLE_API_KEY_PATH ||
      process.env.APPLE_API_PRIVATE_KEY) &&
    !(
      process.env.APPLE_API_KEY &&
      process.env.APPLE_API_ISSUER &&
      (process.env.APPLE_API_KEY_PATH || process.env.APPLE_API_PRIVATE_KEY)
    );

  addCheck(
    'macos-notarization-config-shape',
    strictRelease ? 'error' : 'warning',
    !hasPartialAppleApiConfig,
    !hasPartialAppleApiConfig
      ? 'Apple notarization inputs are either absent or structurally complete.'
      : 'Partial Apple notarization variables were detected. Expected APPLE_API_KEY + APPLE_API_ISSUER + (APPLE_API_KEY_PATH or APPLE_API_PRIVATE_KEY).',
  );

  addCheck(
    'updater-signing-env',
    strictRelease ? 'error' : 'warning',
    strictRelease ? hasCompleteUpdaterConfig : hasUpdaterInputs,
    strictRelease
      ? 'Release builds require updater signing keys for published manifests.'
      : hasUpdaterInputs
        ? 'Updater signing environment variables are present.'
        : 'No updater signing key detected. Bundles can still be built locally, but signed update feeds are not ready.',
  );

  const errorCount = checks.filter(
    (check) => check.severity === 'error' && check.status === 'fail',
  ).length;
  const warningCount = checks.filter(
    (check) => check.severity === 'warning' && check.status === 'fail',
  ).length;

  const report = {
    generatedAt: new Date().toISOString(),
    status: errorCount === 0 ? 'pass' : 'fail',
    releaseContext: {
      gitRef: process.env.GITHUB_REF ?? null,
      tagRelease: isTagRelease,
      strictRelease,
    },
    summary: {
      errors: errorCount,
      warnings: warningCount,
      checks: checks.length,
    },
    versions: {
      packageJson: packageJson.version,
      tauriConfig: tauriConfig.version,
      cargoToml: cargoVersion,
    },
    checks,
  };

  await mkdir(path.dirname(reportPath), { recursive: true });
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');

  const summaryLine = `[release-preflight] status=${report.status} errors=${errorCount} warnings=${warningCount} report=${path.relative(rootDir, reportPath)}`;
  console.log(summaryLine);

  for (const check of checks) {
    const marker = check.status === 'pass' ? 'PASS' : check.severity === 'error' ? 'FAIL' : 'WARN';
    console.log(`${marker} ${check.id}: ${check.message}`);
  }

  if (errorCount > 0) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error('[release-preflight] unexpected failure');
  console.error(error instanceof Error ? (error.stack ?? error.message) : error);
  process.exitCode = 1;
});
