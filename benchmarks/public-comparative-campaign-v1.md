# Public Comparative Campaign v1

Status: `active`  
Status date: `2026-04-22`

## Purpose

Turn `TT-01` into an executable campaign instead of a vague future promise.
This runbook defines the first publishable comparative benchmark slice for
Récupère.

It does **not** authorize fabricated competitor evidence.

## Initial scope

The first public campaign is intentionally narrow:

- use the shared corpus manifest in `benchmarks/corpus/v1/manifest.json`;
- gate publication on the `ready-in-repo` scenarios that are both `P0` and
  stable enough to compare today;
- require one run for `Récupère`, one for `PhotoRec`, and one for `TestDisk`;
- treat `R-Studio`, `Disk Drill`, `Stellar`, `EaseUS`, and other paid suites
  as optional bonus evidence, not as publication blockers;
- treat `DMDE` free mode as optional bonus evidence when its distribution and
  execution surface are usable in the current environment;
- keep `unsupported`, `blocked`, and `not-run` outcomes visible.

This keeps the first campaign honest while the broader corpus is still
becoming publishable.

As of `2026-04-22`, `apfs_deleted_orphan_catalog_v1` remains `P0` but is
temporarily `public-artifact-pending` because the current macOS 15.7.4
fixture/path is not deterministic enough to count as benchmark-grade evidence.

## Operator workflow

1. Validate the manifest.

```bash
npm run benchmark:validate
```

2. Generate a run file for the tool you are about to execute.

```bash
npm run benchmark:template -- \
  --out benchmarks/results/2026-04-22-dmde-4.2.json \
  --tool-name "DMDE" \
  --tool-version "4.2" \
  --build-ref "public-campaign-v1" \
  --host-os "Windows 11 24H2" \
  --host-arch "x86_64" \
  --operator "Initials"
```

3. Execute the scenarios using the exact scenario ids from the manifest.

4. Fill the result file with:
   - `run.runScope = "public-campaign"`;
   - `run.campaignId = "public-comparative-campaign-v1"`;
   - one protocol status per scenario;
   - the measured metrics when known;
   - the missing artefacts that were expected but not produced;
   - `evidenceRefs` pointing to logs, exported hashes, report artefacts, or
     archived operator notes.

For GUI-only or paid tools used as optional bonus evidence, archive an
operator note with the exact downloaded version, hash, and automation blocker
when the tool cannot be driven from the current environment. Do not convert
installation or launch proof into a fake completed run.

5. Validate every result file.

```bash
npm run benchmark:check
```

6. Generate the publishable HTML comparison.

```bash
npm run benchmark:report
```

The report lands in `dist/benchmarks-report.html`.

Targeted blocker refresh runs, such as the APFS orphan-catalog regression
checks, must not be mixed into this campaign silently. Record them with a
different `runScope` such as `targeted-regression`.

## Publication gate

Do not publish a market-facing conclusion until all of the following are true:

- the report contains a real `Récupère` run plus the required accessible
  comparator set: `PhotoRec` and `TestDisk`;
- every `P0` `ready-in-repo` scenario is covered for those required runs;
- unsupported cases remain explicitly visible;
- the roadmap and scorecard were refreshed from the evidence that exists, not
  from implementation intent.

## Interpretation rules

- Compare scenario by scenario before summarizing any trend.
- Treat `completed-with-gaps` as partial evidence, not as a full success.
- Treat `unsupported` as acceptable only when it remains visible.
- Never claim superiority from a single synthetic run without repeatability.
- Never convert a `blocked` or `invalid-run` result into a silent omission.
