#!/usr/bin/env node
// ============================================================================
// Récupère — SBOM generator
// ============================================================================
//
// Produces two CycloneDX-flavoured SBOMs:
//   - `dist/sbom-rust.json`  : every Rust dep in the workspace (via `cargo metadata`)
//   - `dist/sbom-node.json`  : every npm dep in production (via `npm ls --all --json`)
//
// Plus a top-level `dist/sbom-summary.json` listing counts + hashes of both
// SBOMs. The release pipeline uploads all three as build artefacts so any
// shipped bundle is paired with a machine-readable bill of materials.
//
// This script is dependency-light on purpose: it uses `cargo metadata` and
// `npm ls` rather than `cyclonedx-cli` so the build container doesn't need
// extra tooling. The output shape is a minimal CycloneDX 1.5-compatible
// subset (bomFormat, specVersion, version, components[]) — fine for the
// GitHub dependency graph and most downstream tools.
// ============================================================================

import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import process from 'node:process';

const REPO_ROOT = resolve(new URL('..', import.meta.url).pathname);
const DIST_DIR = join(REPO_ROOT, 'dist');

mkdirSync(DIST_DIR, { recursive: true });

function runJson(cmd, args, { cwd = REPO_ROOT } = {}) {
  const result = spawnSync(cmd, args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) {
    throw new Error(`${cmd} ${args.join(' ')} exited with ${result.status}: ${result.stderr}`);
  }
  return JSON.parse(result.stdout);
}

function sha256OfString(str) {
  return createHash('sha256').update(str).digest('hex');
}

function rustSbom() {
  const metadata = runJson('cargo', [
    'metadata',
    '--format-version=1',
    '--all-features',
    '--manifest-path',
    'src-tauri/Cargo.toml',
  ]);
  const components = metadata.packages.map((pkg) => ({
    type: 'library',
    name: pkg.name,
    version: pkg.version,
    purl: `pkg:cargo/${encodeURIComponent(pkg.name)}@${pkg.version}`,
    licenses: (pkg.license ?? '').split(/ OR | AND |\//).filter(Boolean).map((license) => ({
      license: { id: license.trim() },
    })),
    ...(pkg.repository ? { externalReferences: [{ type: 'vcs', url: pkg.repository }] } : {}),
  }));
  return {
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    version: 1,
    metadata: {
      timestamp: new Date().toISOString(),
      tools: [{ name: 'recupere-sbom', vendor: 'Récupère', version: '0.1.0' }],
      component: {
        type: 'application',
        name: 'recupere',
        version: metadata.packages.find((p) => p.name === 'recupere')?.version ?? 'unknown',
      },
    },
    components,
  };
}

function nodeSbom() {
  let tree;
  try {
    tree = runJson('npm', ['ls', '--all', '--json', '--omit=dev']);
  } catch (error) {
    // `npm ls` exits non-zero on dep warnings but still emits valid JSON.
    if (error.message.includes('exited with 1')) {
      const fallback = spawnSync('npm', ['ls', '--all', '--json', '--omit=dev'], {
        cwd: REPO_ROOT,
        encoding: 'utf8',
        maxBuffer: 64 * 1024 * 1024,
      });
      try {
        tree = JSON.parse(fallback.stdout);
      } catch {
        throw error;
      }
    } else {
      throw error;
    }
  }

  const seen = new Map();
  function walk(node) {
    if (!node) return;
    const deps = node.dependencies ?? {};
    for (const [name, info] of Object.entries(deps)) {
      const key = `${name}@${info.version}`;
      if (!seen.has(key) && info.version) {
        seen.set(key, {
          type: 'library',
          name,
          version: info.version,
          purl: `pkg:npm/${encodeURIComponent(name)}@${info.version}`,
          ...(info.resolved ? { externalReferences: [{ type: 'distribution', url: info.resolved }] } : {}),
        });
      }
      walk(info);
    }
  }
  walk(tree);

  return {
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    version: 1,
    metadata: {
      timestamp: new Date().toISOString(),
      tools: [{ name: 'recupere-sbom', vendor: 'Récupère', version: '0.1.0' }],
      component: {
        type: 'application',
        name: tree.name ?? 'recupere',
        version: tree.version ?? '0.0.0',
      },
    },
    components: [...seen.values()],
  };
}

function writeSbom(filename, payload) {
  const path = join(DIST_DIR, filename);
  const serialized = JSON.stringify(payload, null, 2);
  writeFileSync(path, serialized);
  return {
    path,
    components: payload.components.length,
    sha256: sha256OfString(serialized),
  };
}

function main() {
  const rust = writeSbom('sbom-rust.json', rustSbom());
  console.log(`[sbom] Rust components:   ${rust.components} → ${rust.path}`);
  const node = writeSbom('sbom-node.json', nodeSbom());
  console.log(`[sbom] Node components:   ${node.components} → ${node.path}`);

  const summary = {
    generated_at: new Date().toISOString(),
    rust,
    node,
  };
  writeFileSync(join(DIST_DIR, 'sbom-summary.json'), JSON.stringify(summary, null, 2));
  console.log(`[sbom] summary:           ${join(DIST_DIR, 'sbom-summary.json')}`);
}

try {
  main();
} catch (error) {
  console.error('[sbom] failed:', error.message);
  process.exit(1);
}
