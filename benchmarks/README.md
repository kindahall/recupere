# Benchmarks

This directory is the working area for `TT-01` benchmark public reproducible work.

Its role is to keep benchmark work:

- versioned
- auditable
- explicit about limitations
- decoupled from marketing claims

## Current status

As of `2026-04-11`, the benchmark workspace is in `bootstrap` mode:

- the corpus is described by a machine-readable manifest;
- most initial scenarios are recipe-based and point to in-repo synthetic fixture generators;
- public redistributable fixture bundles are not all published yet;
- competitor runs are expected to be recorded manually, using the same scenario identifiers and evidence rules.

## Layout

- [`protocol-v1.md`](./protocol-v1.md): benchmark protocol to follow
- [`public-comparative-campaign-v1.md`](./public-comparative-campaign-v1.md): operator runbook for the first publishable comparative campaign
- [`scorecard-v1.md`](./scorecard-v1.md): dated top-tier track scorecard
- [`corpus/v1/manifest.json`](./corpus/v1/manifest.json): scenario inventory and readiness
- [`results/`](./results/): result files and result conventions

## Workflow

1. Update the scenario manifest before adding or removing benchmark cases.
2. Validate the manifest with `npm run benchmark:validate`.
3. Generate a fresh result template with `npm run benchmark:template`.
   For an operator-ready run file, pass run metadata through to the generator,
   including `--run-scope` and `--campaign-id` when the run belongs to a
   public campaign or to a targeted blocker probe.
4. Run Récupère and competitor tools against the same scenario ids.
5. Save result JSON files in `benchmarks/results/`.
6. Refresh the top-tier scorecard when evidence changes.
7. Update `docs/benchmark-market.md` and `docs/top-tier-roadmap.md` only after evidence exists.

## Ground rules

- No benchmark result may claim superiority without reproducible evidence.
- Every scenario must preserve the repo's read-only safety posture.
- Unsupported cases must stay visible in results.
- Spot-checks, blocker probes, internal baselines, and public campaign runs
  must stay distinguishable in the result metadata and in the HTML report.
- Screenshots are supporting material, never the primary proof.
