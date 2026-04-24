#!/usr/bin/env node
// ============================================================================
// Récupère — benchmark HTML report generator
// ============================================================================
//
// Builds a publishable static HTML page from `benchmarks/results/*.json`.
// The report is intentionally conservative: it surfaces missing competitor
// runs, still-pending P0 scenarios, and read-only uncertainty instead of
// flattening everything into a single success number.
//
// Usage:
//   node scripts/benchmark-report.mjs
//     [--out dist/benchmarks-report.html]
//     [--manifest benchmarks/corpus/v1/manifest.json]
// ============================================================================

import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import process from 'node:process';

const REPO_ROOT = resolve(new URL('..', import.meta.url).pathname);
const DEFAULT_RESULTS_DIR = join(REPO_ROOT, 'benchmarks', 'results');
const DEFAULT_MANIFEST_PATH = join(REPO_ROOT, 'benchmarks', 'corpus', 'v1', 'manifest.json');
const DEFAULT_DIST_DIR = join(REPO_ROOT, 'dist');
const STATUS_ORDER = [
  'completed',
  'completed-with-gaps',
  'unsupported',
  'blocked',
  'invalid-run',
  'not-run'
];
const STATUS_META = {
  completed: { className: 'status-ok', label: 'completed', short: 'OK' },
  'completed-with-gaps': {
    className: 'status-partial',
    label: 'completed with gaps',
    short: 'Partial'
  },
  unsupported: { className: 'status-unsupported', label: 'unsupported', short: 'Unsupported' },
  blocked: { className: 'status-blocked', label: 'blocked', short: 'Blocked' },
  'invalid-run': { className: 'status-invalid', label: 'invalid run', short: 'Invalid' },
  'not-run': { className: 'status-pending', label: 'not run', short: 'Not run' },
  unknown: { className: 'status-unknown', label: 'unknown', short: '?' }
};
const REQUIRED_PUBLIC_CAMPAIGN_GROUPS = [
  {
    id: 'recupere',
    label: 'Récupère public campaign run',
    matchers: ['recupere', 'récupère']
  },
  {
    id: 'photorec',
    label: 'PhotoRec public campaign run',
    matchers: ['photorec']
  },
  {
    id: 'testdisk',
    label: 'TestDisk public campaign run',
    matchers: ['testdisk']
  }
];
const OPTIONAL_EVIDENCE_GROUPS = [
  {
    id: 'dmde',
    label: 'DMDE evidence',
    matchers: ['dmde']
  },
  {
    id: 'r-studio',
    label: 'R-Studio evidence',
    matchers: ['r-studio', 'r studio']
  },
  {
    id: 'ux-commercial',
    label: 'UX-oriented commercial evidence',
    matchers: ['disk drill', 'stellar', 'easeus', 'recoverit', 'wondershare']
  }
];

function parseArgs(argv) {
  const options = {
    outPath: join(DEFAULT_DIST_DIR, 'benchmarks-report.html'),
    manifestPath: DEFAULT_MANIFEST_PATH,
    resultsDir: DEFAULT_RESULTS_DIR
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--out' && argv[index + 1]) {
      options.outPath = resolve(argv[index + 1]);
      index += 1;
    } else if (arg === '--manifest' && argv[index + 1]) {
      options.manifestPath = resolve(argv[index + 1]);
      index += 1;
    } else if (arg === '--results-dir' && argv[index + 1]) {
      options.resultsDir = resolve(argv[index + 1]);
      index += 1;
    }
  }

  return options;
}

function escapeHtml(value) {
  if (value === null || value === undefined) {
    return '';
  }

  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, 'utf8'));
}

function statusMeta(status) {
  return STATUS_META[status] ?? STATUS_META.unknown;
}

function runScopeValue(run) {
  const value = String(run?.data?.run?.runScope ?? '').trim();
  return value || 'custom';
}

function runScopeLabel(run) {
  switch (runScopeValue(run)) {
    case 'internal-baseline':
      return 'Internal baseline';
    case 'public-campaign':
      return 'Public campaign';
    case 'spot-check':
      return 'Spot-check';
    case 'targeted-regression':
      return 'Targeted regression';
    default:
      return 'Custom';
  }
}

function normalizeToolName(value) {
  return String(value ?? '')
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .trim();
}

function loadManifest(filePath) {
  if (!existsSync(filePath)) {
    return null;
  }

  try {
    return readJson(filePath);
  } catch (error) {
    console.warn(`[benchmark-report] cannot parse manifest ${filePath}: ${error.message}`);
    return null;
  }
}

function isTemplateLikeRun(data, fileName) {
  if (fileName.startsWith('template-')) {
    return true;
  }

  return !data?.run?.toolName || !Array.isArray(data?.scenarios);
}

function discoverRuns(resultsDir) {
  if (!existsSync(resultsDir)) {
    return [];
  }

  const entries = readdirSync(resultsDir)
    .filter((name) => name.endsWith('.json'))
    .sort();
  const runs = [];

  for (const entry of entries) {
    const filePath = join(resultsDir, entry);

    try {
      const data = readJson(filePath);
      if (isTemplateLikeRun(data, entry)) {
        continue;
      }
      runs.push({ fileName: entry, filePath, data });
    } catch (error) {
      console.warn(`[benchmark-report] skipping ${entry}: ${error.message}`);
    }
  }

  runs.sort((left, right) => {
    const leftDate = left.data.generatedAt ?? left.fileName;
    const rightDate = right.data.generatedAt ?? right.fileName;
    return leftDate.localeCompare(rightDate);
  });

  return runs;
}

function buildScenarioMeta(manifest) {
  const byId = new Map();

  for (const scenario of manifest?.scenarios ?? []) {
    byId.set(scenario.id, scenario);
  }

  return byId;
}

function summarizeRun(run) {
  const counts = Object.fromEntries(STATUS_ORDER.map((status) => [status, 0]));
  let readOnlyFailures = 0;
  let readOnlyUnknown = 0;
  let covered = 0;

  for (const scenario of run.data.scenarios ?? []) {
    const status = STATUS_ORDER.includes(scenario.status) ? scenario.status : 'not-run';
    counts[status] += 1;

    if (status !== 'not-run') {
      covered += 1;
    }

    if (scenario.metrics?.readOnlyPreserved === false) {
      readOnlyFailures += 1;
    } else if (scenario.metrics?.readOnlyPreserved === null) {
      readOnlyUnknown += 1;
    }
  }

  return {
    counts,
    covered,
    total: run.data.scenarios?.length ?? 0,
    readOnlyFailures,
    readOnlyUnknown
  };
}

function findScenarioResult(run, scenarioId) {
  return run.data.scenarios?.find((scenario) => scenario.id === scenarioId) ?? null;
}

function hasCoverageForScenarioIds(run, scenarioIds) {
  return scenarioIds.every((scenarioId) => {
    const result = findScenarioResult(run, scenarioId);
    return result && result.status !== 'not-run';
  });
}

function findMatchingRuns(runs, group, scope = null) {
  return runs.filter((run) => {
    if (scope && runScopeValue(run) !== scope) {
      return false;
    }

    return group.matchers.some((matcher) =>
      normalizeToolName(run.data.run.toolName).includes(matcher)
    );
  });
}

function buildPublicationReadiness(manifest, runs) {
  const p0ReadyScenarios = (manifest?.scenarios ?? []).filter(
    (scenario) => scenario.priority === 'P0' && scenario.readiness === 'ready-in-repo'
  );
  const p0BlockedScenarios = (manifest?.scenarios ?? []).filter(
    (scenario) => scenario.priority === 'P0' && scenario.readiness !== 'ready-in-repo'
  );
  const p0ReadyIds = p0ReadyScenarios.map((scenario) => scenario.id);
  const publicCampaignRuns = runs.filter((run) => runScopeValue(run) === 'public-campaign');
  const items = [];

  items.push({
    passed: Boolean(manifest),
    label: 'Manifest benchmark chargé',
    detail: manifest
      ? `${manifest.corpusId} (${manifest.scenarios.length} scenarios)`
      : 'Manifeste absent ou illisible'
  });

  items.push({
    passed: runs.length > 0,
    label: 'Au moins un run exploitable',
    detail: `${runs.length} fichier(s) de résultats chargés`
  });

  items.push({
    passed: publicCampaignRuns.length > 0,
    label: 'Au moins un run de campagne publique',
    detail: `${publicCampaignRuns.length} run(s) scopes public-campaign`
  });

  items.push({
    passed: p0ReadyIds.length > 0,
    label: 'Scénarios P0 ready-in-repo disponibles',
    detail: `${p0ReadyIds.length} scénario(s) publiables aujourd'hui`
  });

  for (const group of REQUIRED_PUBLIC_CAMPAIGN_GROUPS) {
    const matchingRuns = findMatchingRuns(runs, group, 'public-campaign');
    items.push({
      passed: matchingRuns.length > 0,
      label: group.label,
      detail:
        matchingRuns.length > 0
          ? matchingRuns.map((run) => run.fileName).join(', ')
          : 'Aucun run présent'
    });

    if (matchingRuns.length > 0 && p0ReadyIds.length > 0) {
      items.push({
        passed: matchingRuns.some((run) => hasCoverageForScenarioIds(run, p0ReadyIds)),
        label: `${group.label} couvre les scénarios P0 prêts`,
        detail: `${p0ReadyIds.length} scénario(s) P0 ready-in-repo requis`
      });
    }
  }

  return {
    items,
    optionalEvidence: OPTIONAL_EVIDENCE_GROUPS.map((group) => ({
      label: group.label,
      runs: findMatchingRuns(runs, group)
    })),
    p0ReadyScenarios,
    p0BlockedScenarios
  };
}

function renderReadinessSection(readiness) {
  return `<section class="panel">
    <h2>Publication readiness</h2>
    <p>The first public campaign is publishable only when the required accessible trio exists as real <code>public-campaign</code> runs: Récupère, PhotoRec, and TestDisk. Optional evidence remains visible, but does not silently become a publication gate.</p>
    <ul class="readiness-list">
      ${readiness.items
        .map(
          (item) => `<li class="${item.passed ? 'ready-pass' : 'ready-fail'}">
            <strong>${item.passed ? 'Pass' : 'Fail'}</strong>
            <span>${escapeHtml(item.label)}</span>
            <small>${escapeHtml(item.detail)}</small>
          </li>`
        )
        .join('\n')}
    </ul>
    ${
      readiness.optionalEvidence.some((group) => group.runs.length > 0)
        ? `<p class="optional-evidence">Optional evidence present: ${readiness.optionalEvidence
            .filter((group) => group.runs.length > 0)
            .map(
              (group) =>
                `${escapeHtml(group.label)} (${group.runs
                  .map((run) => escapeHtml(run.fileName))
                  .join(', ')})`
            )
            .join(' • ')}</p>`
        : ''
    }
  </section>`;
}

function renderSummaryTable(runs) {
  const rows = runs
    .map((run) => {
      const summary = summarizeRun(run);
      return `<tr>
        <td><strong>${escapeHtml(run.data.run.toolName)}</strong><br><small>${escapeHtml(run.fileName)}</small></td>
        <td>${escapeHtml(runScopeLabel(run))}</td>
        <td><code>${escapeHtml(run.data.run.campaignId ?? '—')}</code></td>
        <td><code>${escapeHtml(run.data.run.buildRef ?? '—')}</code></td>
        <td>${summary.counts.completed}</td>
        <td>${summary.counts['completed-with-gaps']}</td>
        <td>${summary.counts.unsupported}</td>
        <td>${summary.counts.blocked}</td>
        <td>${summary.counts['invalid-run']}</td>
        <td>${summary.counts['not-run']}</td>
        <td>${summary.covered}/${summary.total}</td>
        <td>${summary.readOnlyFailures}</td>
        <td>${summary.readOnlyUnknown}</td>
      </tr>`;
    })
    .join('\n');

  return `<section class="panel">
    <h2>Run summary</h2>
    <table class="summary-table">
      <thead>
        <tr>
          <th>Run</th>
          <th>Scope</th>
          <th>Campaign</th>
          <th>Build</th>
          <th>Completed</th>
          <th>Partial</th>
          <th>Unsupported</th>
          <th>Blocked</th>
          <th>Invalid</th>
          <th>Not run</th>
          <th>Coverage</th>
          <th>RO fail</th>
          <th>RO ?</th>
        </tr>
      </thead>
      <tbody>${rows}</tbody>
    </table>
  </section>`;
}

function renderComparisonMatrix(manifest, runs, title, description) {
  const scenarios = (manifest?.scenarios ?? []).filter(
    (scenario) => scenario.priority === 'P0' && scenario.readiness === 'ready-in-repo'
  );

  if (scenarios.length === 0 || runs.length === 0) {
    return '';
  }

  const header = runs
    .map(
      (run) => `<th>${escapeHtml(run.data.run.toolName)}<br><small>${escapeHtml(
        run.data.run.toolVersion ?? ''
      )}</small></th>`
    )
    .join('\n');

  const rows = scenarios
    .map((scenario) => {
      const cells = runs
        .map((run) => {
          const result = findScenarioResult(run, scenario.id);
          const status = statusMeta(result?.status ?? 'not-run');
          const intact = result?.metrics?.intactExports;
          const partial = result?.metrics?.partialUsableExports;
          const readOnly = result?.metrics?.readOnlyPreserved;
          return `<td class="${status.className}">
            <strong>${escapeHtml(status.short)}</strong>
            <small>I:${escapeHtml(intact ?? '—')} P:${escapeHtml(partial ?? '—')}</small>
            <small>RO:${escapeHtml(readOnly === null || readOnly === undefined ? '?' : readOnly ? 'yes' : 'no')}</small>
          </td>`;
        })
        .join('\n');

      return `<tr>
        <td>
          <strong>${escapeHtml(scenario.id)}</strong><br>
          <small>${escapeHtml(scenario.title)}</small><br>
          <small>${escapeHtml(scenario.filesystem)} • ${escapeHtml(scenario.class)} • ${escapeHtml(
            scenario.difficulty
          )}</small>
        </td>
        ${cells}
      </tr>`;
    })
    .join('\n');

  return `<section class="panel">
    <h2>${escapeHtml(title)}</h2>
    <p>${escapeHtml(description)}</p>
    <table class="matrix-table">
      <thead>
        <tr>
          <th>Scenario</th>
          ${header}
        </tr>
      </thead>
      <tbody>${rows}</tbody>
    </table>
  </section>`;
}

function renderBlockedP0Section(readiness) {
  if (readiness.p0BlockedScenarios.length === 0) {
    return '';
  }

  return `<section class="panel">
    <h2>P0 scenarios still outside publishable scope</h2>
    <p>These P0 scenarios remain intentionally visible even though they are not ready-in-repo yet. Keeping them visible prevents the benchmark layer from overstating maturity.</p>
    <ul class="readiness-list">
      ${readiness.p0BlockedScenarios
        .map(
          (scenario) => `<li class="ready-fail">
            <strong>${escapeHtml(scenario.id)}</strong>
            <span>${escapeHtml(scenario.title)}</span>
            <small>${escapeHtml(scenario.readiness)} • ${escapeHtml(
              scenario.source?.notes ?? 'No manifest note'
            )}</small>
          </li>`
        )
        .join('\n')}
    </ul>
  </section>`;
}

function renderScenarioDetail(result, scenarioMeta) {
  const meta = statusMeta(result.status);
  const metrics = Object.entries(result.metrics ?? {})
    .map(
      ([key, value]) => `<tr><td class="metric-key">${escapeHtml(key)}</td><td>${escapeHtml(
        value ?? '—'
      )}</td></tr>`
    )
    .join('\n');
  const evidenceRefs = (result.evidenceRefs ?? [])
    .map((ref) => `<li>${escapeHtml(ref)}</li>`)
    .join('\n');
  const missing = (result.artifacts?.missing ?? [])
    .map((item) => `<span class="pill pill-missing">${escapeHtml(item)}</span>`)
    .join(' ');

  return `<article class="scenario-card ${meta.className}">
    <header>
      <div>
        <h3>${escapeHtml(result.id)}</h3>
        ${
          scenarioMeta
            ? `<p class="scenario-meta">${escapeHtml(scenarioMeta.title)} • ${escapeHtml(
                scenarioMeta.priority
              )} • ${escapeHtml(scenarioMeta.readiness)}</p>`
            : ''
        }
      </div>
      <span class="pill">${escapeHtml(meta.label)}</span>
    </header>
    <table class="metrics-table"><tbody>${metrics}</tbody></table>
    ${
      evidenceRefs
        ? `<details><summary>Evidence refs</summary><ul>${evidenceRefs}</ul></details>`
        : ''
    }
    ${missing ? `<div class="missing"><strong>Missing artefacts:</strong> ${missing}</div>` : ''}
    ${result.notes ? `<p class="notes">${escapeHtml(result.notes)}</p>` : ''}
  </article>`;
}

function renderRunSection(run, scenarioMetaById) {
  return `<section class="run-section">
    <header class="run-header">
      <div>
        <h2>${escapeHtml(run.data.run.toolName)} ${escapeHtml(run.data.run.toolVersion ?? '')}</h2>
        <p class="run-meta">
          Scope <code>${escapeHtml(runScopeLabel(run))}</code> •
          campaign <code>${escapeHtml(run.data.run.campaignId ?? '—')}</code> •
          Build <code>${escapeHtml(run.data.run.buildRef ?? '—')}</code> •
          ${escapeHtml(run.data.run.hostOs ?? '')} ${escapeHtml(run.data.run.hostArch ?? '')} •
          operator ${escapeHtml(run.data.run.operator ?? '—')}
        </p>
      </div>
      <small>${escapeHtml(run.fileName)}</small>
    </header>
    ${run.data.run.notes ? `<p class="run-notes">${escapeHtml(run.data.run.notes)}</p>` : ''}
    <div class="scenario-grid">
      ${(run.data.scenarios ?? [])
        .map((result) => renderScenarioDetail(result, scenarioMetaById.get(result.id)))
        .join('\n')}
    </div>
  </section>`;
}

function renderPage(manifest, runs) {
  const scenarioMetaById = buildScenarioMeta(manifest);
  const readiness = buildPublicationReadiness(manifest, runs);
  const generatedAt = new Date().toISOString();
  const publicCampaignRuns = runs.filter((run) => runScopeValue(run) === 'public-campaign');
  const styles = `
    :root {
      color-scheme: light;
      --bg: #f5f1e8;
      --panel: #fffdf8;
      --ink: #192126;
      --muted: #6a737d;
      --line: #d9d0c3;
      --ok: #0f766e;
      --partial: #a16207;
      --unsupported: #7c3aed;
      --blocked: #b45309;
      --invalid: #b91c1c;
      --pending: #64748b;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background:
        radial-gradient(circle at top left, rgba(221, 176, 91, 0.18), transparent 26rem),
        linear-gradient(180deg, #f6f0e5 0%, var(--bg) 100%);
      color: var(--ink);
      font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", serif;
    }
    main {
      width: min(1200px, calc(100vw - 2rem));
      margin: 0 auto;
      padding: 2rem 0 3rem;
    }
    h1, h2, h3 { margin: 0; }
    p { line-height: 1.5; }
    code {
      font-family: "SFMono-Regular", "SF Mono", Consolas, monospace;
      font-size: 0.92em;
    }
    .hero, .panel, .run-section {
      background: rgba(255, 253, 248, 0.96);
      border: 1px solid var(--line);
      border-radius: 18px;
      box-shadow: 0 18px 45px rgba(90, 72, 40, 0.08);
      padding: 1.25rem 1.4rem;
      margin-bottom: 1.25rem;
    }
    .hero p { margin: 0.75rem 0 0; }
    .readiness-list {
      list-style: none;
      padding: 0;
      margin: 1rem 0 0;
      display: grid;
      gap: 0.75rem;
    }
    .readiness-list li {
      border: 1px solid var(--line);
      border-radius: 12px;
      padding: 0.75rem 0.9rem;
      display: grid;
      gap: 0.2rem;
    }
    .ready-pass strong { color: var(--ok); }
    .ready-fail strong { color: var(--invalid); }
    .optional-evidence {
      margin-top: 1rem;
      color: var(--muted);
      font-size: 0.95rem;
    }
    .summary-table, .matrix-table, .metrics-table {
      width: 100%;
      border-collapse: collapse;
      margin-top: 1rem;
    }
    .summary-table th, .summary-table td,
    .matrix-table th, .matrix-table td,
    .metrics-table td {
      border-bottom: 1px solid var(--line);
      padding: 0.55rem 0.6rem;
      text-align: left;
      vertical-align: top;
    }
    .matrix-table td small,
    .summary-table small,
    .scenario-meta,
    .run-meta,
    .run-notes,
    .notes,
    .missing,
    .metric-key {
      color: var(--muted);
    }
    .matrix-table td strong { display: block; }
    .matrix-table td small { display: block; margin-top: 0.15rem; }
    .run-header {
      display: flex;
      justify-content: space-between;
      gap: 1rem;
      align-items: flex-start;
    }
    .scenario-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(290px, 1fr));
      gap: 1rem;
      margin-top: 1rem;
    }
    .scenario-card {
      border: 1px solid var(--line);
      border-radius: 14px;
      padding: 0.95rem 1rem;
      background: #fffdfa;
    }
    .scenario-card header {
      display: flex;
      justify-content: space-between;
      gap: 0.75rem;
      align-items: flex-start;
      margin-bottom: 0.75rem;
    }
    .pill {
      display: inline-block;
      border-radius: 999px;
      padding: 0.18rem 0.6rem;
      background: #ede6d8;
      font-size: 0.78rem;
      white-space: nowrap;
    }
    .pill-missing {
      background: rgba(185, 28, 28, 0.12);
      color: var(--invalid);
      margin-right: 0.35rem;
    }
    .status-ok strong, .status-ok .pill { color: var(--ok); }
    .status-partial strong, .status-partial .pill { color: var(--partial); }
    .status-unsupported strong, .status-unsupported .pill { color: var(--unsupported); }
    .status-blocked strong, .status-blocked .pill { color: var(--blocked); }
    .status-invalid strong, .status-invalid .pill { color: var(--invalid); }
    .status-pending strong, .status-pending .pill,
    .status-unknown strong, .status-unknown .pill { color: var(--pending); }
    .status-ok { background: rgba(15, 118, 110, 0.05); }
    .status-partial { background: rgba(161, 98, 7, 0.06); }
    .status-unsupported { background: rgba(124, 58, 237, 0.05); }
    .status-blocked { background: rgba(180, 83, 9, 0.06); }
    .status-invalid { background: rgba(185, 28, 28, 0.06); }
    .status-pending, .status-unknown { background: rgba(100, 116, 139, 0.05); }
    @media (max-width: 900px) {
      main { width: min(100vw, calc(100vw - 1rem)); }
      .run-header { flex-direction: column; }
      .summary-table, .matrix-table { font-size: 0.92rem; }
    }
  `;

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Récupère benchmark report</title>
  <style>${styles}</style>
</head>
<body>
  <main>
    <section class="hero">
      <h1>Récupère benchmark report</h1>
      <p>Generated ${escapeHtml(generatedAt)} from ${runs.length} run(s). This page compares evidence files under <code>benchmarks/results/</code> against the shared corpus manifest and keeps unsupported, blocked, and still-not-run scenarios visible.</p>
      <p>Manifest: <code>${escapeHtml(manifest?.corpusId ?? 'missing')}</code>. Protocol source: <code>benchmarks/protocol-v1.md</code>.</p>
    </section>
    ${renderReadinessSection(readiness)}
    ${renderSummaryTable(runs)}
    ${renderBlockedP0Section(readiness)}
    ${renderComparisonMatrix(
      manifest,
      publicCampaignRuns,
      'Public campaign P0 ready scenario comparison',
      'This matrix compares only runs explicitly marked public-campaign. Unsupported or blocked outcomes still count as evidence; hidden gaps do not.'
    )}
    ${runs.map((run) => renderRunSection(run, scenarioMetaById)).join('\n')}
  </main>
</body>
</html>`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = loadManifest(options.manifestPath);
  const runs = discoverRuns(options.resultsDir);

  const html = renderPage(manifest, runs);
  mkdirSync(dirname(options.outPath), { recursive: true });
  writeFileSync(options.outPath, html);
  console.log(`[benchmark-report] wrote ${options.outPath} (${runs.length} run(s))`);
}

main();
