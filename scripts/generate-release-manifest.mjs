import { createHash } from "node:crypto";
import { mkdir, readFile, stat, writeFile, readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const bundleDir = path.join(rootDir, "src-tauri", "target", "release", "bundle");
const outputDir = path.join(rootDir, "dist", "release");
const releasePlatform = process.env.RELEASE_PLATFORM ?? null;
const manifestSuffix = (process.env.RELEASE_MANIFEST_SUFFIX ?? releasePlatform ?? "").trim();
const releaseSigned = process.env.RELEASE_SIGNED === "true";
const releaseNotarized = process.env.RELEASE_NOTARIZED === "true";
const releaseStapled = process.env.RELEASE_STAPLED === "true";

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

async function pathExists(targetPath) {
  try {
    await stat(targetPath);
    return true;
  } catch {
    return false;
  }
}

async function sha256ForFile(filePath) {
  const buffer = await readFile(filePath);
  return createHash("sha256").update(buffer).digest("hex");
}

async function listFilesRecursive(directoryPath, relativeRoot = directoryPath) {
  const entries = await readdir(directoryPath, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const absolutePath = path.join(directoryPath, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await listFilesRecursive(absolutePath, relativeRoot)));
      continue;
    }

    if (entry.isFile()) {
      files.push({
        absolutePath,
        relativePath: path.relative(relativeRoot, absolutePath),
      });
    }
  }

  files.sort((left, right) => left.relativePath.localeCompare(right.relativePath));
  return files;
}

async function directorySizeBytes(directoryPath) {
  const files = await listFilesRecursive(directoryPath);
  let total = 0;

  for (const file of files) {
    total += (await stat(file.absolutePath)).size;
  }

  return total;
}

async function sha256ForDirectory(directoryPath) {
  const files = await listFilesRecursive(directoryPath);
  const hash = createHash("sha256");

  for (const file of files) {
    hash.update(file.relativePath);
    hash.update("\0");
    hash.update(await readFile(file.absolutePath));
    hash.update("\0");
  }

  return hash.digest("hex");
}

async function resolveMacHelperHash(appPath) {
  const macosDir = path.join(appPath, "Contents", "MacOS");
  if (!(await pathExists(macosDir))) {
    return null;
  }

  const entries = (await readdir(macosDir, { withFileTypes: true }))
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right));

  if (entries.length === 0) {
    return null;
  }

  const preferredName =
    entries.find((name) => /imager|recup[eè]re/i.test(name)) ?? entries[0];
  return sha256ForFile(path.join(macosDir, preferredName));
}

function buildReleaseNotes(version, manifestName, artifacts) {
  const listedArtifacts = artifacts
    .map((artifact) => `- \`${artifact.fileName}\` (${artifact.platform}, ${artifact.artifact_kind})`)
    .join("\n");

  return `# Récupère ${version}

## Included artifacts

- \`${manifestName}\`
- \`release-checksums${manifestSuffix ? `-${manifestSuffix}` : ""}.txt\`
${listedArtifacts || "- No bundle artifacts were discovered."}

## Signing status

- macOS signed: ${releaseSigned ? "yes" : "no"}
- macOS notarized: ${releaseNotarized ? "yes" : "no"}
- macOS stapled: ${releaseStapled ? "yes" : "no"}
- Windows/Linux builds published in this tranche remain unsigned unless external certificates are configured.

## Bootable rescue MVP

- documented workflow: \`docs/bootable-rescue-workflow.md\`
- current mode: Linux live USB + packaged \`AppImage\`
- posture: source stays read-only, export goes only to a separate writable destination
`;
}

async function collectFileArtifacts(directory, matcher, platform, artifactKind) {
  if (!(await pathExists(directory))) {
    return [];
  }

  const entries = await readdir(directory);
  const matches = entries.filter((entry) => matcher(entry)).sort((left, right) => left.localeCompare(right));
  const artifacts = [];

  for (const entry of matches) {
    const absolutePath = path.join(directory, entry);
    const stats = await stat(absolutePath);
    artifacts.push({
      platform,
      artifact_kind: artifactKind,
      fileName: entry,
      relativePath: path.relative(rootDir, absolutePath),
      sizeBytes: stats.size,
      checksum: await sha256ForFile(absolutePath),
      signed: platform === "macos" ? releaseSigned : false,
      notarized: platform === "macos" ? releaseNotarized : false,
      stapled: platform === "macos" ? releaseStapled : false,
      helper_hash: null,
    });
  }

  return artifacts;
}

async function collectAppArtifacts(directory) {
  if (!(await pathExists(directory))) {
    return [];
  }

  const entries = await readdir(directory);
  const apps = entries.filter((entry) => entry.endsWith(".app")).sort((left, right) => left.localeCompare(right));
  const artifacts = [];

  for (const entry of apps) {
    const absolutePath = path.join(directory, entry);
    artifacts.push({
      platform: "macos",
      artifact_kind: "app",
      fileName: entry,
      relativePath: path.relative(rootDir, absolutePath),
      sizeBytes: await directorySizeBytes(absolutePath),
      checksum: await sha256ForDirectory(absolutePath),
      signed: releaseSigned,
      notarized: releaseNotarized,
      stapled: releaseStapled,
      helper_hash: await resolveMacHelperHash(absolutePath),
    });
  }

  return artifacts;
}

async function collectReleaseArtifacts() {
  const artifacts = [];
  artifacts.push(...(await collectAppArtifacts(path.join(bundleDir, "macos"))));
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "dmg"), (entry) => entry.endsWith(".dmg"), "macos", "dmg")),
  );
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "appimage"), (entry) => entry.endsWith(".AppImage"), "linux", "appimage")),
  );
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "deb"), (entry) => entry.endsWith(".deb"), "linux", "deb")),
  );
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "rpm"), (entry) => entry.endsWith(".rpm"), "linux", "rpm")),
  );
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "msi"), (entry) => entry.endsWith(".msi"), "windows", "msi")),
  );
  artifacts.push(
    ...(await collectFileArtifacts(path.join(bundleDir, "nsis"), (entry) => entry.endsWith(".exe"), "windows", "nsis")),
  );
  return artifacts;
}

async function main() {
  const packageJson = await readJson(path.join(rootDir, "package.json"));
  const tauriConfig = await readJson(path.join(rootDir, "src-tauri", "tauri.conf.json"));
  const artifacts = await collectReleaseArtifacts();
  const bootableRescueDocPath = path.join(rootDir, "docs", "bootable-rescue-workflow.md");

  if (artifacts.length === 0) {
    throw new Error("No release bundle artifacts were discovered under src-tauri/target/release/bundle.");
  }

  const rescueArtifacts = artifacts.filter(
    (artifact) => artifact.platform === "linux" && artifact.artifact_kind === "appimage",
  );
  const rescueDocPresent = await pathExists(bootableRescueDocPath);

  const manifest = {
    generatedAt: new Date().toISOString(),
    productName: tauriConfig.productName,
    version: packageJson.version,
    bundleIdentifier: tauriConfig.identifier,
    channel: "stable",
    platform: releasePlatform ?? "mixed",
    signed: releaseSigned,
    notarized: releaseNotarized,
    stapled: releaseStapled,
    rescueWorkflow: {
      status:
        rescueDocPresent && rescueArtifacts.length > 0
          ? "mvp-ready"
          : rescueDocPresent
            ? "doc-only"
            : "missing",
      mode: "linux-live-usb-appimage",
      documentation: rescueDocPresent ? path.relative(rootDir, bootableRescueDocPath) : null,
      readOnlySourceRequired: true,
      separateWritableDestinationRequired: true,
      supportedOperations: [
        "device-detection",
        "read-only-scan",
        "read-only-imaging",
        "safe-export",
        "support-bundle",
      ],
      limitations: [
        "no custom Recupere-branded boot ISO yet",
        "no broad hardware-driver parity claim",
        "no direct macOS or Windows rescue boot path in this tranche",
        "no pre-boot encryption parity claim",
      ],
      artifacts: rescueArtifacts.map((artifact) => artifact.fileName),
    },
    artifacts,
  };

  const manifestName = `release-manifest-${packageJson.version}${manifestSuffix ? `-${manifestSuffix}` : ""}.json`;
  const checksumsName = `release-checksums${manifestSuffix ? `-${manifestSuffix}` : ""}.txt`;
  const notesName = `release-notes${manifestSuffix ? `-${manifestSuffix}` : ""}.md`;
  const manifestPath = path.join(outputDir, manifestName);
  const checksumsPath = path.join(outputDir, checksumsName);
  const notesPath = path.join(outputDir, notesName);

  const checksumsText = artifacts
    .map((artifact) => `${artifact.checksum}  ${artifact.fileName}`)
    .join("\n");

  await mkdir(outputDir, { recursive: true });
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  await writeFile(checksumsPath, `${checksumsText}\n`, "utf8");
  await writeFile(notesPath, buildReleaseNotes(packageJson.version, manifestName, artifacts), "utf8");

  console.log(
    `[release-manifest] version=${packageJson.version} artifacts=${artifacts.length} manifest=${path.relative(rootDir, manifestPath)} checksums=${path.relative(rootDir, checksumsPath)}`,
  );
}

main().catch((error) => {
  console.error("[release-manifest] unexpected failure");
  console.error(error instanceof Error ? error.stack ?? error.message : error);
  process.exitCode = 1;
});
