import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_MANIFEST = path.resolve('benchmarks/corpus/v1/manifest.json');
const DEFAULT_RESULTS_DIR = path.resolve('benchmarks/results');
const VALID_STATUSES = new Set([
  'not-run',
  'completed',
  'completed-with-gaps',
  'unsupported',
  'blocked',
  'invalid-run'
]);
const VALID_RUN_SCOPES = new Set([
  'custom',
  'internal-baseline',
  'public-campaign',
  'spot-check',
  'targeted-regression'
]);

function fail(message) {
  console.error(`benchmark-results: ${message}`);
  process.exit(1);
}

function readJson(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    fail(`unable to read ${filePath}: ${error.message}`);
  }
}

function requireString(value, label) {
  if (typeof value !== 'string' || value.trim() === '') {
    fail(`${label} must be a non-empty string`);
  }
}

function isNullableNumber(value) {
  return value === null || (typeof value === 'number' && Number.isFinite(value) && value >= 0);
}

function isNullableBoolean(value) {
  return value === null || typeof value === 'boolean';
}

function parseArgs(argv) {
  const options = {
    manifestPath: DEFAULT_MANIFEST,
    resultFiles: []
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--manifest') {
      options.manifestPath = path.resolve(argv[index + 1] ?? '');
      index += 1;
    } else if (arg === '--result') {
      options.resultFiles.push(path.resolve(argv[index + 1] ?? ''));
      index += 1;
    } else {
      fail(`unknown argument '${arg}'`);
    }
  }

  return options;
}

function discoverResultFiles() {
  if (!fs.existsSync(DEFAULT_RESULTS_DIR)) {
    return [];
  }

  return fs
    .readdirSync(DEFAULT_RESULTS_DIR)
    .filter((entry) => entry.endsWith('.json'))
    .filter((entry) => !entry.startsWith('template-'))
    .map((entry) => path.join(DEFAULT_RESULTS_DIR, entry))
    .sort();
}

function validateScenarioResult(result, manifestScenario, label) {
  if (!VALID_STATUSES.has(result.status)) {
    fail(`${label}.status must be one of ${Array.from(VALID_STATUSES).join(', ')}`);
  }

  if (!result.metrics || typeof result.metrics !== 'object') {
    fail(`${label}.metrics must be an object`);
  }

  const metrics = result.metrics;
  const metricKeys = [
    'filesFound',
    'intactExports',
    'partialUsableExports',
    'falsePositives',
    'timeToFirstSafeActionSeconds',
    'timeToFirstSuccessfulExportSeconds'
  ];

  for (const key of metricKeys) {
    if (!isNullableNumber(metrics[key])) {
      fail(`${label}.metrics.${key} must be null or a non-negative number`);
    }
  }

  if (!isNullableBoolean(metrics.readOnlyPreserved)) {
    fail(`${label}.metrics.readOnlyPreserved must be null or a boolean`);
  }

  if (!result.artifacts || typeof result.artifacts !== 'object') {
    fail(`${label}.artifacts must be an object`);
  }

  if (!Array.isArray(result.artifacts.paths) || !Array.isArray(result.artifacts.missing)) {
    fail(`${label}.artifacts.paths and .missing must be arrays`);
  }

  if (result.evidenceRefs !== undefined && !Array.isArray(result.evidenceRefs)) {
    fail(`${label}.evidenceRefs must be an array when present`);
  }

  if (result.notes !== undefined && typeof result.notes !== 'string') {
    fail(`${label}.notes must be a string when present`);
  }

  if (manifestScenario.priority === 'P0' && result.status === 'not-run') {
    console.warn(
      `benchmark-results: warning: ${label} is P0 in the manifest but still marked not-run`
    );
  }
}

function validateResultFile(filePath, manifest) {
  const result = readJson(filePath);

  if (result.templateVersion !== 1) {
    fail(`${filePath}: templateVersion must equal 1`);
  }

  if (!result.manifest || typeof result.manifest !== 'object') {
    fail(`${filePath}: manifest section must exist`);
  }

  if (result.manifest.corpusId !== manifest.corpusId) {
    fail(`${filePath}: manifest.corpusId must equal ${manifest.corpusId}`);
  }

  if (!result.run || typeof result.run !== 'object') {
    fail(`${filePath}: run section must exist`);
  }

  requireString(result.run.toolName, `${filePath}: run.toolName`);
  requireString(result.run.toolVersion, `${filePath}: run.toolVersion`);
  requireString(result.run.buildRef, `${filePath}: run.buildRef`);
  requireString(result.run.hostOs, `${filePath}: run.hostOs`);
  requireString(result.run.hostArch, `${filePath}: run.hostArch`);
  requireString(result.run.operator, `${filePath}: run.operator`);
  if (
    result.run.runScope !== undefined &&
    !VALID_RUN_SCOPES.has(result.run.runScope)
  ) {
    fail(
      `${filePath}: run.runScope must be one of ${Array.from(VALID_RUN_SCOPES).join(', ')}`
    );
  }
  if (
    result.run.campaignId !== undefined &&
    typeof result.run.campaignId !== 'string'
  ) {
    fail(`${filePath}: run.campaignId must be a string when present`);
  }

  if (!Array.isArray(result.scenarios)) {
    fail(`${filePath}: scenarios must be an array`);
  }

  if (result.scenarios.length !== manifest.scenarios.length) {
    fail(
      `${filePath}: scenarios length ${result.scenarios.length} does not match manifest length ${manifest.scenarios.length}`
    );
  }

  const manifestById = new Map(manifest.scenarios.map((scenario) => [scenario.id, scenario]));
  const seen = new Set();
  const counts = new Map();

  for (const scenarioResult of result.scenarios) {
    requireString(scenarioResult.id, `${filePath}: scenario.id`);
    if (seen.has(scenarioResult.id)) {
      fail(`${filePath}: duplicate scenario id ${scenarioResult.id}`);
    }
    seen.add(scenarioResult.id);

    const manifestScenario = manifestById.get(scenarioResult.id);
    if (!manifestScenario) {
      fail(`${filePath}: scenario id ${scenarioResult.id} does not exist in the manifest`);
    }

    validateScenarioResult(scenarioResult, manifestScenario, `${filePath}:${scenarioResult.id}`);
    counts.set(
      scenarioResult.status,
      (counts.get(scenarioResult.status) ?? 0) + 1
    );
  }

  console.log(`Validated ${filePath}`);
  for (const [status, count] of [...counts.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    console.log(`  - ${status}: ${count}`);
  }

  return result;
}

function scenarioMetricValue(scenario, key) {
  const value = scenario.metrics?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function summarizeResult(result) {
  const statusCounts = Object.fromEntries([...VALID_STATUSES].sort().map((status) => [status, 0]));
  const totals = {
    filesFound: 0,
    intactExports: 0,
    partialUsableExports: 0,
    falsePositives: 0,
    readOnlyFailures: 0,
    readOnlyUnknown: 0
  };

  for (const scenario of result.scenarios) {
    statusCounts[scenario.status] = (statusCounts[scenario.status] ?? 0) + 1;
    totals.filesFound += scenarioMetricValue(scenario, 'filesFound');
    totals.intactExports += scenarioMetricValue(scenario, 'intactExports');
    totals.partialUsableExports += scenarioMetricValue(scenario, 'partialUsableExports');
    totals.falsePositives += scenarioMetricValue(scenario, 'falsePositives');
    if (scenario.metrics?.readOnlyPreserved === false) {
      totals.readOnlyFailures += 1;
    } else if (scenario.metrics?.readOnlyPreserved === null) {
      totals.readOnlyUnknown += 1;
    }
  }

  return {
    toolName: result.run.toolName,
    toolVersion: result.run.toolVersion,
    buildRef: result.run.buildRef,
    runScope: result.run.runScope ?? 'custom',
    generatedAt: result.generatedAt ?? null,
    scenarioCount: result.scenarios.length,
    statusCounts,
    totals
  };
}

function writeBenchmarkSummary(manifest, results, resultFiles) {
  const distDir = path.resolve('dist');
  fs.mkdirSync(distDir, { recursive: true });

  const manifestById = new Map(manifest.scenarios.map((scenario) => [scenario.id, scenario]));
  const runs = results.map(summarizeResult);
  const p0ScenarioIds = manifest.scenarios
    .filter((scenario) => scenario.priority === 'P0')
    .map((scenario) => scenario.id);
  const p0Coverage = p0ScenarioIds.map((scenarioId) => {
    const scenario = manifestById.get(scenarioId);
    const statuses = results.map((result) => {
      const scenarioResult = result.scenarios.find((entry) => entry.id === scenarioId);
      return {
        toolName: result.run.toolName,
        status: scenarioResult?.status ?? 'not-run'
      };
    });

    return {
      id: scenarioId,
      title: scenario?.title ?? scenarioId,
      readiness: scenario?.readiness ?? 'unknown',
      statuses
    };
  });
  const summary = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    corpusId: manifest.corpusId,
    resultFiles: resultFiles.map((filePath) => path.relative(process.cwd(), filePath)),
    runs,
    p0Coverage,
    cautions: [
      'This summary is evidence tracking, not a marketing claim.',
      'Unsupported, blocked, and not-run scenarios stay visible by design.',
      'Read-only preservation must be false-free before any public recovery claim.'
    ]
  };

  const jsonPath = path.join(distDir, 'benchmark-results-summary.json');
  const markdownPath = path.join(distDir, 'benchmark-results-summary.md');
  fs.writeFileSync(jsonPath, `${JSON.stringify(summary, null, 2)}\n`);
  fs.writeFileSync(markdownPath, buildBenchmarkSummaryMarkdown(summary));
  console.log(`benchmark-results: wrote ${jsonPath}`);
  console.log(`benchmark-results: wrote ${markdownPath}`);
}

function buildBenchmarkSummaryMarkdown(summary) {
  const lines = [
    '# Benchmark Results Summary',
    '',
    `Generated: ${summary.generatedAt}`,
    `Corpus: \`${summary.corpusId}\``,
    '',
    'This report is conservative evidence tracking. It does not claim market superiority.',
    '',
    '## Runs',
    ''
  ];

  for (const run of summary.runs) {
    lines.push(
      `- **${run.toolName} ${run.toolVersion}** (${run.runScope}, ${run.buildRef})`,
      `  - scenarios: ${run.scenarioCount}`,
      `  - files found: ${run.totals.filesFound}`,
      `  - intact exports: ${run.totals.intactExports}`,
      `  - partial usable exports: ${run.totals.partialUsableExports}`,
      `  - false positives: ${run.totals.falsePositives}`,
      `  - read-only failures: ${run.totals.readOnlyFailures}`,
      `  - read-only unknown: ${run.totals.readOnlyUnknown}`,
      `  - statuses: ${Object.entries(run.statusCounts)
        .filter(([, count]) => count > 0)
        .map(([status, count]) => `${status}=${count}`)
        .join(', ') || 'none'}`,
      ''
    );
  }

  lines.push('## P0 Coverage', '');
  for (const scenario of summary.p0Coverage) {
    lines.push(
      `- **${scenario.id}** — ${scenario.title} (${scenario.readiness})`,
      `  - ${scenario.statuses.map((status) => `${status.toolName}: ${status.status}`).join('; ')}`,
      ''
    );
  }

  lines.push('## Cautions', '');
  for (const caution of summary.cautions) {
    lines.push(`- ${caution}`);
  }

  return `${lines.join('\n')}\n`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = readJson(options.manifestPath);
  const resultFiles = options.resultFiles.length > 0 ? options.resultFiles : discoverResultFiles();

  if (resultFiles.length === 0) {
    console.log('benchmark-results: no benchmark result files to validate');
    return;
  }

  const results = [];
  for (const filePath of resultFiles) {
    results.push(validateResultFile(filePath, manifest));
  }
  writeBenchmarkSummary(manifest, results, resultFiles);
}

main();
