# Benchmark Protocol v1

Status: `draft`  
Effective date: `2026-04-11`

## Purpose

This protocol defines how Récupère is benchmarked against professional recovery tools in a way that is:

- reproducible
- honest
- auditable
- useful for product decisions

## Non-goals

This protocol does not exist to:

- publish vague marketing wins;
- hide weak scenarios;
- compare tools on different source data;
- imply recovery of physically destroyed data.

## Scope

Phase 1 focuses on a hybrid corpus:

- `ready-in-repo` scenarios generated from in-repo synthetic fixtures;
- `public-artifact-pending` scenarios already defined but not yet shipped as redistributable benchmark images;
- manual competitor runs recorded with the same scenario ids and evidence rules.

Phase 2 will add:

- public fixture bundles where redistribution is legally and technically safe;
- dedicated runners for more automated Récupère-side execution;
- publication-ready reports.

## Benchmark rules

Every tool run must:

- use the same scenario id from the corpus manifest;
- record the tool name, version, host OS, date, operator, and notes;
- preserve a read-only workflow on the source;
- export recovered data to a separate destination;
- capture the required evidence artifacts for that scenario.

## Mandatory metrics

Each scenario result must record:

- `filesFound`
- `intactExports`
- `partialUsableExports`
- `falsePositives`
- `timeToFirstSafeActionSeconds`
- `timeToFirstSuccessfulExportSeconds`
- `readOnlyPreserved`
- `notes`

The benchmark may also record:

- `operatorComplexity`
- `manualStepsCount`
- `warningsObserved`
- `artifactsMissing`

## Result status values

- `not-run`
- `completed`
- `completed-with-gaps`
- `unsupported`
- `blocked`
- `invalid-run`

## Evidence requirements

At minimum, each run should retain the artifacts declared by the scenario:

- technical report or equivalent
- support bundle or equivalent log archive when available
- exported file hashes where feasible
- screenshots only as secondary material

If a competitor cannot produce one of these artifacts, the result must say so explicitly.

## Publication threshold

No public market claim should be made until all of the following are true:

- the corpus manifest is versioned and validated;
- the result files are present for all `P0` ready scenarios;
- missing or weaker scenarios are documented;
- the comparison includes at least two non-paid-accessible competitors, with `PhotoRec` and `TestDisk` as the default baseline set for public publication;
- the benchmark notes clearly separate measured facts from interpretation.

## First internal milestone

The first internal milestone for `TT-01` is:

- a validated corpus manifest;
- a standard result template;
- at least one internal result file for Récupère;
- explicit readiness tracking for scenarios not yet publishable as public artifacts.
