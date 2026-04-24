# Top-tier scorecard v1

Status date: `2026-04-23`

This scorecard turns the broad top-tier roadmap into a smaller set of
evidence-backed tracks that can be updated over time.

It is intentionally conservative:

- `closed` means the repo contains product flow and validation evidence;
- `partial` means the repo contains real capability but still visible gaps;
- `open` means the capability is still materially behind the target level.

It also acts as the quickest repo-level truth source for tracks already
partially delivered, so `TT-07` and `TT-08` are included alongside the
benchmark-linked engine tracks.

## Tracks

| Track | Status | Why it matters | Current evidence |
|---|---|---|---|
| `TT-03` unstable-media imaging | `partial` | Protects risky sources and conditions all later recovery work | Imaging incident UI, rescue-map export, support bundle handoff, unreadable-range samples in live/report/history |
| `TT-02` bootable rescue workflow | `partial` | Critical when the host OS no longer boots and serious recovery has to stay read-only | Bootable rescue MVP is now documented around a Linux live USB + `AppImage` workflow, and release metadata/preflight can describe that rescue posture honestly; custom rescue media and hardware validation still remain open |
| `TT-04` advanced storage / lab workflows | `partial` | Distinguishes serious recovery tools from basic desktop apps | RAID analysis image workflow, imported-source provenance, expert readiness, but VM/NAS/storage matrices still incomplete |
| `TT-05` APFS / encryption / macOS difficult cases | `partial` | Decisive on modern Apple workflows and locked volumes | APFS operator surfacing, conservative APFS deleted-file triage, encryption gatekeeping, but snapshots/clones/full parity still open and the orphan-catalog deleted-fixture path is currently not deterministic on macOS 15.7.4 |
| `TT-01` public benchmark proof | `partial` | Keeps market claims honest and comparable | Corpus manifest, protocol, internal baseline, report generator, scoped run metadata, APFS blocker runs for `2026-04-22` and `2026-04-23`, full accessible-comparator `P0` result files for `PhotoRec 7.2` and `TestDisk 7.2`, bonus `DMDE 4.4.6` evidence on JPEG carving, plus archived optional `R-Studio 7.5.191751` operator notes are now checked in; a full public-campaign `Récupère` run and the blocked APFS P0 path still remain missing |
| `TT-07` premium UX / audit / reporting | `partial` | Converts technical quality into user trust, support quality, and premium operator value | History/support/handoff flows, richer support bundle posture, novice/expert separation, hash-chained audit diagnostics now surface broken-link counts and chain tip hash, but signed audit artefacts and stress-path polish remain open |
| `TT-08` docs / packaging / QA / release readiness | `partial` | Prevents a serious codebase from being presented more mature than it is | `cargo check`, `npm run test:ui`, `npm run benchmark:check`, and release metadata/preflight now describe rescue-readiness more honestly; broader release proof and commercial readiness still remain open |

## Required evidence to move each track to `closed`

### `TT-03`

- degraded-media corpus runs with stable result recording;
- explicit checkpoint/resume/rescue-map evidence on benchmark fixtures;
- stronger engine-level evidence beyond UI/reporting hardening.

### `TT-02`

- validate the Linux live-USB + `AppImage` rescue path on real boot scenarios;
- add broader hardware and storage-controller evidence before claiming serious field parity;
- keep the rescue posture read-only and bounded until a custom rescue medium really exists.

### `TT-04`

- source-type support matrix carried in product and docs;
- broader advanced-source fixtures and validations;
- clearer VM / NAS / virtual-disk boundaries with less ambiguity.

### `TT-05`

- more APFS snapshot/clone evidence;
- stronger macOS encrypted-volume case handling;
- stabilize the deleted APFS orphan-catalog benchmark path on current macOS hosts;
- fewer “present but not yet delivered” APFS/macOS pathways.

### `TT-01`

- broader competitor result files beyond the first accessible baseline;
- a full public-campaign `Récupère` run across the ready-in-repo `P0` slice;
- repeatable score comparison once the blocked APFS `P0` path is stable again;
- scorecard refreshed from actual evidence, not internal intent.

### `TT-07`

- signed or otherwise tamper-evident audit artefacts;
- stronger stress-path consistency across guided and expert flows;
- fewer operator handoff gaps between live UI, reports, and bundles.

### `TT-08`

- roadmap, scorecard, and product docs stay aligned without manual drift;
- broader cross-platform release and QA evidence;
- fewer “serious but still partial” surfaces presented as market-ready.
