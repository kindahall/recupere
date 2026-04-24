import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_MANIFEST = path.resolve('benchmarks/corpus/v1/manifest.json');
const VALID_PRIORITIES = new Set(['P0', 'P1', 'P2']);
const VALID_READINESS = new Set([
  'ready-in-repo',
  'public-artifact-pending',
  'planned'
]);
const VALID_CLASSES = new Set([
  'deleted-file',
  'lost-volume',
  'carving',
  'unstable-media'
]);
const VALID_DIFFICULTIES = new Set(['simple', 'medium', 'advanced']);
const VALID_RUN_SCOPES = new Set([
  'custom',
  'internal-baseline',
  'public-campaign',
  'spot-check',
  'targeted-regression'
]);

function fail(message) {
  console.error(`benchmark-manifest: ${message}`);
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

function requireArray(value, label) {
  if (!Array.isArray(value) || value.length === 0) {
    fail(`${label} must be a non-empty array`);
  }
}

function increment(map, key) {
  map.set(key, (map.get(key) ?? 0) + 1);
}

function validateScenario(scenario, index, ids, counts) {
  const prefix = `scenario[${index}]`;

  requireString(scenario.id, `${prefix}.id`);
  if (ids.has(scenario.id)) {
    fail(`${prefix}.id '${scenario.id}' is duplicated`);
  }
  ids.add(scenario.id);

  requireString(scenario.title, `${prefix}.title`);
  requireString(scenario.filesystem, `${prefix}.filesystem`);
  requireString(scenario.class, `${prefix}.class`);
  requireString(scenario.difficulty, `${prefix}.difficulty`);

  if (!VALID_PRIORITIES.has(scenario.priority)) {
    fail(`${prefix}.priority must be one of ${Array.from(VALID_PRIORITIES).join(', ')}`);
  }
  if (!VALID_READINESS.has(scenario.readiness)) {
    fail(`${prefix}.readiness must be one of ${Array.from(VALID_READINESS).join(', ')}`);
  }
  if (!VALID_CLASSES.has(scenario.class)) {
    fail(`${prefix}.class must be one of ${Array.from(VALID_CLASSES).join(', ')}`);
  }
  if (!VALID_DIFFICULTIES.has(scenario.difficulty)) {
    fail(`${prefix}.difficulty must be one of ${Array.from(VALID_DIFFICULTIES).join(', ')}`);
  }

  requireArray(scenario.platforms, `${prefix}.platforms`);
  if (scenario.platforms.some((entry) => typeof entry !== 'string' || entry.trim() === '')) {
    fail(`${prefix}.platforms entries must be non-empty strings`);
  }

  if (!scenario.source || typeof scenario.source !== 'object') {
    fail(`${prefix}.source must be an object`);
  }
  requireString(scenario.source.kind, `${prefix}.source.kind`);
  requireString(scenario.source.location, `${prefix}.source.location`);
  requireString(scenario.source.generator, `${prefix}.source.generator`);

  if (!scenario.groundTruth || typeof scenario.groundTruth !== 'object') {
    fail(`${prefix}.groundTruth must be an object`);
  }
  if (
    typeof scenario.groundTruth.expectedRecoverableItemsMin !== 'number' ||
    scenario.groundTruth.expectedRecoverableItemsMin < 0
  ) {
    fail(`${prefix}.groundTruth.expectedRecoverableItemsMin must be a non-negative number`);
  }
  if (
    typeof scenario.groundTruth.expectedIntactExportsMin !== 'number' ||
    scenario.groundTruth.expectedIntactExportsMin < 0
  ) {
    fail(`${prefix}.groundTruth.expectedIntactExportsMin must be a non-negative number`);
  }

  if (!scenario.evidence || typeof scenario.evidence !== 'object') {
    fail(`${prefix}.evidence must be an object`);
  }
  requireArray(scenario.evidence.requiredArtifacts, `${prefix}.evidence.requiredArtifacts`);

  increment(counts.byReadiness, scenario.readiness);
  increment(counts.byFilesystem, scenario.filesystem);
  increment(counts.byClass, scenario.class);
}

function validateManifest(manifest, manifestPath) {
  if (manifest.schemaVersion !== 1) {
    fail(`schemaVersion must equal 1 in ${manifestPath}`);
  }

  requireString(manifest.corpusId, 'corpusId');
  requireString(manifest.title, 'title');
  requireString(manifest.status, 'status');
  requireString(manifest.statusDate, 'statusDate');
  requireArray(manifest.scenarios, 'scenarios');

  const ids = new Set();
  const counts = {
    byReadiness: new Map(),
    byFilesystem: new Map(),
    byClass: new Map()
  };

  manifest.scenarios.forEach((scenario, index) => {
    validateScenario(scenario, index, ids, counts);
  });

  return counts;
}

function printCounts(label, counts) {
  console.log(label);
  for (const [key, value] of [...counts.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    console.log(`  - ${key}: ${value}`);
  }
}

function buildTemplate(manifest) {
  return {
    templateVersion: 1,
    generatedAt: new Date().toISOString(),
    manifest: {
      corpusId: manifest.corpusId,
      schemaVersion: manifest.schemaVersion,
      statusDate: manifest.statusDate
    },
    run: {
      toolName: '',
      toolVersion: '',
      buildRef: '',
      hostOs: '',
      hostArch: '',
      operator: '',
      runScope: 'custom',
      campaignId: '',
      notes: ''
    },
    scenarios: manifest.scenarios.map((scenario) => ({
      id: scenario.id,
      status: 'not-run',
      metrics: {
        filesFound: null,
        intactExports: null,
        partialUsableExports: null,
        falsePositives: null,
        timeToFirstSafeActionSeconds: null,
        timeToFirstSuccessfulExportSeconds: null,
        readOnlyPreserved: null
      },
      artifacts: {
        paths: [],
        missing: []
      },
      notes: ''
    }))
  };
}

function parseArgs(argv) {
  const options = {
    manifestPath: DEFAULT_MANIFEST,
    templatePath: null,
    run: {
      toolName: '',
      toolVersion: '',
      buildRef: '',
      hostOs: '',
      hostArch: '',
      operator: '',
      notes: ''
    }
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--manifest') {
      options.manifestPath = path.resolve(argv[index + 1] ?? '');
      index += 1;
    } else if (arg === '--template' || arg === '--out') {
      options.templatePath = path.resolve(argv[index + 1] ?? '');
      index += 1;
    } else if (arg === '--tool-name') {
      options.run.toolName = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--tool-version') {
      options.run.toolVersion = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--build-ref') {
      options.run.buildRef = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--host-os') {
      options.run.hostOs = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--host-arch') {
      options.run.hostArch = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--operator') {
      options.run.operator = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--run-scope') {
      options.run.runScope = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--campaign-id') {
      options.run.campaignId = argv[index + 1] ?? '';
      index += 1;
    } else if (arg === '--notes') {
      options.run.notes = argv[index + 1] ?? '';
      index += 1;
    } else {
      fail(`unknown argument '${arg}'`);
    }
  }

  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const manifest = readJson(options.manifestPath);
  const counts = validateManifest(manifest, options.manifestPath);

  console.log(`Validated ${options.manifestPath}`);
  console.log(`Corpus id: ${manifest.corpusId}`);
  console.log(`Scenario count: ${manifest.scenarios.length}`);
  printCounts('Readiness', counts.byReadiness);
  printCounts('Filesystems', counts.byFilesystem);
  printCounts('Classes', counts.byClass);

  if (options.templatePath) {
    const template = buildTemplate(manifest);
    if (!VALID_RUN_SCOPES.has(options.run.runScope)) {
      fail(
        `runScope must be one of ${Array.from(VALID_RUN_SCOPES).join(', ')} when generating a template`
      );
    }
    template.run = {
      ...template.run,
      ...options.run
    };
    fs.mkdirSync(path.dirname(options.templatePath), { recursive: true });
    fs.writeFileSync(options.templatePath, `${JSON.stringify(template, null, 2)}\n`);
    console.log(`Wrote template ${options.templatePath}`);
  }
}

main();
