#!/usr/bin/env node
// ============================================================================
// Récupère — Tauri updater keypair generator
// ============================================================================
// Wraps `npx tauri signer generate` so the team has a single canonical way to
// create the Ed25519 keypair used to sign update manifests. The PRIVATE key
// is written locally and must be imported into the CI secret store; it must
// NOT be committed to the repo. The PUBLIC key is printed at the end so it
// can be pasted into `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
//
// Usage:
//   npm run release:updater-keygen [-- --out path/to/key.bin --password mypass]
//
// Defaults:
//   - key written to `./updater.key` (gitignored: add it yourself)
//   - password is REQUIRED and prompted via Tauri CLI if not passed
// ============================================================================
import { spawn } from "node:child_process";
import { readFile, access } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function parseArgs(argv) {
  const args = { out: path.join(rootDir, "updater.key"), force: false, password: null };
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--out" && argv[i + 1]) {
      args.out = path.resolve(rootDir, argv[i + 1]);
      i += 1;
    } else if (token === "--password" && argv[i + 1]) {
      args.password = argv[i + 1];
      i += 1;
    } else if (token === "--force") {
      args.force = true;
    } else if (token === "--help" || token === "-h") {
      args.help = true;
    }
  }
  return args;
}

function printHelp() {
  console.log(`Generate a Tauri updater signing keypair.

Usage:
  npm run release:updater-keygen -- [options]

Options:
  --out <path>       Where to write the private key (default: ./updater.key)
  --password <pass>  Password protecting the key (prompted if omitted)
  --force            Overwrite an existing key file
  -h, --help         Show this help

After the key is generated:
  1. Copy the printed PUBLIC KEY (base64) into src-tauri/tauri.conf.json
     under plugins.updater.pubkey.
  2. Store the PRIVATE key AND the password in your CI secret store as
     TAURI_SIGNING_PRIVATE_KEY (contents of the file) and
     TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
  3. Ensure updater.key is in .gitignore — it must never be committed.
`);
}

async function fileExists(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    printHelp();
    return;
  }

  if (!args.password) {
    console.error(
      "[updater-keygen] refusing to generate: pass --password or export TAURI_SIGNING_PASSWORD.",
    );
    console.error("An unprotected signing key must never exist on disk.");
    process.exit(2);
  }

  if (!args.force && (await fileExists(args.out))) {
    console.error(`[updater-keygen] ${args.out} already exists. Use --force to overwrite.`);
    process.exit(2);
  }

  const cliArgs = [
    "--yes",
    "tauri",
    "signer",
    "generate",
    "--write-keys",
    args.out,
    "--password",
    args.password,
    ...(args.force ? ["--force"] : []),
  ];

  const child = spawn("npx", cliArgs, { stdio: "inherit", cwd: rootDir });

  await new Promise((resolve, reject) => {
    child.on("close", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`tauri signer exited with code ${code}`));
    });
    child.on("error", reject);
  });

  // Echo the public-key file — Tauri CLI writes <out>.pub next to <out>.
  const pubPath = `${args.out}.pub`;
  if (await fileExists(pubPath)) {
    const pubKey = (await readFile(pubPath, "utf8")).trim();
    console.log("\n=== Récupère updater PUBLIC KEY ===");
    console.log(pubKey);
    console.log("====================================");
    console.log(
      `\nPaste this value into src-tauri/tauri.conf.json -> plugins.updater.pubkey, then set active: true.`,
    );
    console.log(
      `Store the PRIVATE key file (${path.relative(rootDir, args.out)}) in CI as TAURI_SIGNING_PRIVATE_KEY and the password as TAURI_SIGNING_PRIVATE_KEY_PASSWORD.`,
    );
  } else {
    console.error(
      `[updater-keygen] key generated but ${pubPath} was not found. Check Tauri CLI output above.`,
    );
    process.exit(1);
  }
}

main().catch((error) => {
  console.error("[updater-keygen] failed:");
  console.error(error instanceof Error ? error.stack ?? error.message : error);
  process.exit(1);
});
