# Découpage progressif de `commands/mod.rs`

## Pourquoi
[src-tauri/src/commands/mod.rs](../src-tauri/src/commands/mod.rs) totalise ~11 900 LoC avec 201 fonctions, ~50 commandes Tauri et un état partagé global (`scan_sessions()`, `export_sessions()`). C'est le P0 dette identifié dans l'audit. Découper en une seule passe est risqué car les helpers privés et les locks `OnceLock<Mutex<...>>` sont accédés depuis plusieurs domaines.

Ce document décrit le **découpage progressif** à exécuter en N sous-passes, **chacune validée par `cargo check && cargo test --lib`** avant de passer à la suivante. Chaque sous-passe est sûre, atomique, et reverse-able indépendamment.

## Cible finale

```
src-tauri/src/commands/
├── mod.rs        # déclarations + ré-exports + types/constants partagés
├── state.rs      # registres globaux (scan_sessions, export_sessions, helpers de persistance, types Persisted*)
├── device.rs     # get_devices, get_diagnostic, get_smart_report, RAID, encryption
├── runtime.rs    # get_runtime_capabilities, get_app_build_info
├── scan.rs       # start_scan, start_potential_volume_scan, pause/resume/cancel, get_scan_*, sessions de scan
├── imaging.rs    # start_imaging + helpers privileged_imager
├── export.rs     # start_export, get_export_*, validate_export_destination, save_technical_timeline_report, save_support_bundle, generate_recovery_report, export_results_csv, generate_lab_bundle
├── preview.rs    # get_file_preview, get_file_hex_preview, get_file_auxiliary_*, save_file_auxiliary_payload
├── ai.rs         # get_ai_advisory, get_scan_ai_brief, ai_autopilot_scan, gemma_*, classify_scan_files, predict_scan_recovery, generate_narrative_report, suggest_file_reconstruction, build_cloud_ai_prompt, run_gemma_analysis, chat_with_ai, smart_select_by_category, search_file_by_name
├── license.rs    # activate_license, get_license_status, deactivate_license, get_machine_fingerprint
└── audit.rs      # get_audit_trail, clear_local_history
```

## Ordre d'exécution recommandé

L'ordre est choisi pour **commencer par les modules les plus indépendants** (peu de helpers partagés) et finir par scan/export/imaging qui partagent l'état de session le plus complexe.

### Sous-passe 1 — `commands/license.rs` (le plus simple)
**Fonctions à déplacer** : `activate_license`, `get_license_status`, `deactivate_license`, `get_machine_fingerprint`. Aucun helper privé, aucun accès au state partagé. Juste des wrappers `crate::license::*`.

**Recette**
1. `cp commands/mod.rs commands/license.rs` mentalement → garder uniquement les 4 fonctions ci-dessus + les `use` nécessaires.
2. Dans `commands/mod.rs` :
   - Supprimer les 4 fonctions.
   - Ajouter en haut : `mod license; pub use license::*;`.
3. Dans `lib.rs` : aucun changement (les imports `commands::activate_license` etc. continuent de marcher grâce à `pub use`).
4. `cargo check && cargo test --lib`. Commit.

### Sous-passe 2 — `commands/audit.rs`
**Fonctions** : `get_audit_trail`, `clear_local_history`.

⚠️ `clear_local_history` touche aux registres `scan_sessions()` et `export_sessions()`. Comme ils sont encore privés à `commands/mod.rs`, on doit d'abord les rendre `pub(super)` ou les exposer via une fonction helper. Solution simple : pour cette sous-passe, on garde `clear_local_history` dans `mod.rs` et on n'extrait que `get_audit_trail` (trivial).

`cargo check && cargo test --lib`. Commit.

### Sous-passe 3 — `commands/runtime.rs`
**Fonctions** : `get_runtime_capabilities`, `get_app_build_info`, helpers `runtime_capabilities()`, `app_build_info()`, constantes `APP_PRODUCT_NAME`, `APP_BUNDLE_IDENTIFIER`.

**Recette** : déplacer le bloc tel quel + `use crate::types::*;`. Compile-check.

### Sous-passe 4 — `commands/state.rs` (extraction du state partagé)
C'est la clé pour pouvoir extraire scan/export/imaging ensuite.

**À déplacer** :
- Structures `InventoryScanSession`, `ExportSession`, `ScanControlState`, `ScanControl`, `PersistedScanRecord`, `PersistedScanArchive`, `PersistedExportArchive`, `PersistedExportRecord`, `LegacyPersistedExportArchive`.
- Constantes : `MAX_SESSION_LOGS`, `MAX_PERSISTED_SCAN_RECORDS`, `MAX_PERSISTED_EXPORT_RECORDS`, `SCAN_CANCELLED_SENTINEL`.
- Fonctions de registre : `scan_sessions()`, `export_sessions()`, `get_session()`.
- Persistance : `persist_scan_session`, `persist_export_session`, `snapshot_scan_record`, `snapshot_export_record`, `upsert_persisted_*`, `load_persisted_*`.
- Lifecycle : `append_scan_log`, `append_export_log`, `push_technical_log`, `request_scan_pause/resume/cancel`, `wait_for_scan_permission`, `finalize_cancelled_scan`, `fail_scan_session`, `scan_control_handle`, `scan_cancelled_error`, `is_scan_cancelled_error`.

**Visibilité** : tout en `pub(super)` pour rester accessible aux autres modules `commands::*` sans fuiter au reste du crate.

**Test impact** : tous les tests inline qui utilisent ces helpers vont casser. Solution : déplacer ces tests aussi dans `commands/state.rs`.

`cargo check && cargo test --lib`. Commit.

### Sous-passe 5 — `commands/device.rs`
**Fonctions** : `get_devices`, `get_diagnostic`, `get_smart_report`, `detect_raid_metadata`, `get_encryption_info`, `unlock_encrypted_device`, `bruteforce_encrypted_device`. Aucune dépendance au state.

### Sous-passe 6 — `commands/preview.rs`
**Fonctions** : toutes les `get_file_*_preview` + `save_file_auxiliary_payload`. Dépendance au state via `get_session`.

### Sous-passe 7 — `commands/ai.rs`
Toutes les commandes AI (déjà identifiées dans le plan principal).

### Sous-passe 8 — `commands/imaging.rs`
`start_imaging` et tous ses helpers (`resolve_imaging_source_plan`, `imaging_requires_elevation_fallback`, `privileged_imager_executable_path`, `is_raw_device_path`, `run_image_acquisition`).

### Sous-passe 9 — `commands/scan.rs`
`start_scan`, `start_potential_volume_scan`, `pause/resume/cancel_scan`, `get_scan_progress`, `get_results`, `get_scan_history`, `get_scan_logs`, `initialize_scan_session`, `supported_*_filesystem`, `best_supported_potential_volume`, `guided_supported_potential_volume_candidate`, ainsi que tous les `run_*_scan` (`run_inventory_scan`, `run_deleted_ntfs_scan`, `run_deleted_ext4_scan`, etc.).

C'est la sous-passe la plus volumineuse (~4 000 LoC) — à faire en dernier quand tous les helpers sont stabilisés en `state.rs`.

### Sous-passe 10 — `commands/export.rs`
`start_export`, `get_export_*`, `validate_export_destination`, `save_technical_timeline_report`, `save_support_bundle`, `generate_recovery_report`, `export_results_csv`, `generate_lab_bundle`, et `clear_local_history` (qui peut maintenant être déplacée vers `audit.rs` ou rester ici).

## Vérification après chaque sous-passe

```bash
cd src-tauri
cargo fmt
cargo check
cargo clippy -- -D warnings
cargo test --lib
cd ..
npx tsc --noEmit  # côté front rien ne devrait bouger
```

Le smoke test manuel (`npm run tauri dev`, lancer un scan + un export) est obligatoire après les sous-passes 4, 8, 9, 10.

## Anti-règles

- ❌ **Ne pas** changer la signature publique d'une commande Tauri pendant le découpage. Le `tauri::generate_handler![...]` dans `lib.rs` ne doit pas être modifié.
- ❌ **Ne pas** combiner deux sous-passes dans un même commit. Si quelque chose casse, on doit pouvoir reverter une seule sous-passe.
- ❌ **Ne pas** changer la logique métier. Découpage uniquement.
- ❌ **Ne pas** déplacer les tests inline vers `tests/` pendant le découpage. Le faire dans une passe séparée à la fin (PR distincte) car cela transforme des tests blanc-boîte en tests noir-boîte et change leur surface d'exécution.

## État actuel

- ✅ Le plan de découpage est documenté.
- ⏳ Aucune sous-passe n'a encore été exécutée.
- 🎯 Recommandation : commencer par la sous-passe 1 (license) qui est triviale et permet de valider la mécanique de `pub use`.

Quand chaque sous-passe est complétée, ajouter ici une ligne `- [x] Sous-passe N — <nom> (commit `<sha>`)`.

- [x] Sous-passe 1 — license.rs (~50 LoC extraites)
- [x] Sous-passe 2 — audit.rs (`get_audit_trail` extrait, `clear_local_history` extrait dans sub-pass 10)
- [x] Sous-passe 3 — runtime.rs (`get_runtime_capabilities`, `get_app_build_info`)
- [x] Sous-passe 4a — state.rs : types et constantes de session uniquement (`InventoryScanSession`, `ExportSession`, `ScanControl*`, `Persisted*`, `MAX_SESSION_LOGS`, etc., 108 LoC)
- [x] Sous-passe 4b — 33 helpers internes marqués `pub(super)` dans `mod.rs` (registre, persistance, lifecycle, summaries) pour permettre aux sous-modules suivants de les appeler. Le déplacement physique est repoussé à une PR dédiée.
- [x] Sous-passe 5 — device.rs (`get_devices`, `get_diagnostic`, `get_smart_report`, `detect_raid_metadata`, `get_encryption_info`, `unlock_encrypted_device`, `bruteforce_encrypted_device` — 7 commandes, 115 LoC)
- [x] Sous-passe 6 — file_preview.rs (`get_file_preview`, `get_file_hex_preview`, `get_file_auxiliary_preview`, `get_file_auxiliary_hex_preview`, `save_file_auxiliary_payload` — 5 commandes, 163 LoC)
- [x] Sous-passe 7 — ai.rs (17 commandes Tauri + 3 helpers + `require_gemma_ready`, 461 LoC)
- [x] Sous-passe 8 — imaging_cmd.rs (`start_imaging` uniquement, 72 LoC). Le worker `run_image_acquisition_*` reste dans `mod.rs` pour l'instant.
- [x] Sous-passe 9 — scan.rs (7 wrappers minces : `get_scan_progress`, `get_results`, `pause_scan`, `resume_scan`, `cancel_scan`, `get_scan_history`, `get_scan_logs` — 95 LoC). `start_scan`, `start_potential_volume_scan` et tous les workers `run_*_scan` restent dans `mod.rs`.
- [x] Sous-passe 10 — export.rs (7 wrappers : `validate_export_destination`, `save_technical_timeline_report`, `save_support_bundle`, `get_export_progress`, `get_export_logs`, `get_export_history`, `clear_local_history` — 229 LoC). `start_export`, `generate_recovery_report`, `export_results_csv`, `generate_lab_bundle` et le worker `run_export_session` restent dans `mod.rs`.
- [x] Bloc imaging — `imaging_cmd.rs` contient maintenant aussi `run_macos_privileged_image_acquisition`, `run_image_acquisition`, `run_image_acquisition_to`, `create_local_image_snapshot` et `run_image_acquisition_with_destination` (validation : `cargo fmt`, `cargo check`, `cargo test --lib`, `npx tsc --noEmit` le 2026-04-08).
- [x] Entrées scan — `scan.rs` contient maintenant aussi `start_scan` et `start_potential_volume_scan`, tandis que les workers `run_*_scan` restent dans `mod.rs` (même validation le 2026-04-08).

## Bilan actuel (2026-04-17, post slice export_worker)

| Module | LoC |
|---|---|
| `commands/mod.rs` | **8 427** (-3 453 vs original 11 880) |
| `commands/state.rs` | 1 072 |
| `commands/license.rs` | 42 |
| `commands/audit.rs` | 37 |
| `commands/runtime.rs` | 65 |
| `commands/device.rs` | 738 |
| `commands/file_preview.rs` | 799 |
| `commands/ai.rs` | 510 |
| `commands/imaging_cmd/` (mod.rs + privileged_macos.rs) | 601 |
| `commands/scan.rs` | 645 |
| `commands/export.rs` | 1 155 |
| `commands/repair_cmd.rs` | 119 |
| `commands/validation.rs` | 78 |
| **Total extrait** | **5 861 LoC** dans 12 modules dédiés |

## Ce qui reste dans `mod.rs`

Les **commandes lourdes restantes** (`start_export`,
`generate_recovery_report`, `export_results_csv`, `generate_lab_bundle`) et
les **workers de scan/export** (`run_potential_volume_scan`,
`run_deleted_*_scan`, `run_signature_carving_scan`, `run_inventory_scan`,
`run_export_session`, …) restent dans `mod.rs`. Ce sont eux qui pèsent
l'essentiel des 10 592 LoC restants.

Une PR dédiée pourra continuer le travail en déplaçant ces workers
**en bloc** (un domaine à la fois : scan, puis imaging, puis export) avec
leurs helpers spécialisés. Les `pub(super)` sont déjà en place pour les
helpers transverses, donc l'extraction sera mécanique.

## Plan détaillé pour finir l'extraction (workers restants)

Cette section décrit, **bloc par bloc**, le travail mécanique restant pour
faire tomber `commands/mod.rs` à ~3-4k LoC. Chaque bloc doit être appliqué
**dans l'ordre** et validé par `cargo check && cargo test --lib` avant le
suivant. Aucun changement de logique métier — uniquement des coupes et des
visibilités.

### Pré-passe : promouvoir les helpers privés en `pub(super)` ✅ réalisée

Avant tout déplacement, marquer `pub(super)` les fonctions suivantes dans
[commands/mod.rs](../src-tauri/src/commands/mod.rs) (numéros de ligne au moment
de la rédaction de ce plan, à actualiser si le fichier a bougé) :

| Helper | Ligne | Utilisé par |
|---|---|---|
| `update_image_acquisition_progress` | 1574 | bloc imaging |
| `read_u64_report` | 1592 | bloc imaging |
| `create_privileged_helper_temp_dir` | 1596 | bloc imaging |
| `build_macos_privileged_imager_script` | 1623 | bloc imaging |
| `build_privileged_imaging_failure` | 1651 | bloc imaging |
| `update_progress` | 5895 | tous les blocs |
| `elapsed_seconds` | 5916 | tous les blocs |

`unix_timestamp_ms` (5923), `append_scan_log`, `wait_for_scan_permission`,
`fail_scan_session`, `finalize_cancelled_scan`, `persist_scan_session` sont
déjà `pub(super)`.

Note : `create_local_image_snapshot` a finalement été déplacée dans
`imaging_cmd.rs` avec le bloc imaging.

### Bloc 1 — `imaging` (~250 LoC) ✅ réalisé

Déplacer dans [commands/imaging_cmd.rs](../src-tauri/src/commands/imaging_cmd.rs)
(à la fin du fichier, après `start_imaging`) :

| Fonction | Lignes |
|---|---|
| `run_macos_privileged_image_acquisition` | 1678-1768 |
| `run_image_acquisition` | 1770-1777 |
| `run_image_acquisition_to` (déjà `pub(super)`) | 1779-1793 |
| `create_local_image_snapshot` | 1795-1827 |
| `run_image_acquisition_with_destination` | 1829-1921 |

`use` à ajouter en tête du sous-module :

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::imaging::{self, ImagingSourcePlan};
use super::state::{InventoryScanSession, MAX_SESSION_LOGS, TechnicalLogEntry};
use super::{
    append_scan_log, build_macos_privileged_imager_script,
    build_privileged_imaging_failure, create_local_image_snapshot,
    create_privileged_helper_temp_dir, elapsed_seconds, fail_scan_session,
    finalize_cancelled_scan, persist_scan_session, read_u64_report,
    unix_timestamp_ms, update_image_acquisition_progress, update_progress,
    wait_for_scan_permission,
};
```

Réalisation effective : le bloc a été extrait et validé le `2026-04-08`.

### Bloc 2 — `scan` workers (~3500 LoC, entrées `start_*` déjà extraites)

Déplacer dans [commands/scan.rs](../src-tauri/src/commands/scan.rs) (à la fin
du fichier, après les wrappers existants) :

| Fonction | Lignes approx |
|---|---|
| `run_potential_volume_scan` | 2028-2740 |
| `run_deleted_fat32_scan` | 2742-2917 |
| `run_deleted_exfat_scan` | 2919-3094 |
| `run_deleted_ntfs_scan` | 3096-3407 |
| `run_deleted_ext4_scan` | 3409-3697 |
| `run_deleted_hfsplus_scan` | 3699-3931 |
| `run_deleted_apfs_scan` | 3933-4108 |
| `run_signature_carving_scan` | 4110-4277 |
| `run_inventory_scan` | 4279-4546 |

C'est le bloc le plus volumineux. Recommandation : le faire en **plusieurs
sous-passes** (un `run_*_scan` à la fois), avec un commit + `cargo check`
entre chaque, plutôt qu'en un seul gigot. Les `run_deleted_*_scan` partagent
les mêmes helpers (`process_deleted_files`, `score_recovery`, etc.) qui
restent dans `mod.rs` ou sont déjà `pub(super)`.

Vérifier `cargo check && cargo test --lib`. Commit.

### Bloc 3 — `export` workers (~1500 LoC)

Déplacer dans [commands/export.rs](../src-tauri/src/commands/export.rs) (à la
fin du fichier) :

| Fonction | Lignes approx |
|---|---|
| `start_export` | 641-1677 (attention : plage estimée car la fonction délègue à `run_export_session` qui est plus bas) |
| `run_export_session` | 4548-… |
| `generate_recovery_report` | 6494-6658 |
| `export_results_csv` | 6660-6709 |
| `generate_lab_bundle` | 6711-… |

Vérifier `cargo check && cargo test --lib`. Commit.

### Vérification finale

Après les 3 blocs :

```bash
cd src-tauri
cargo fmt
cargo check
cargo clippy -- -D warnings
cargo test --lib
```

Smoke test runtime : lancer un scan + un export + un imaging pour valider
qu'aucune commande Tauri n'a perdu son `#[tauri::command]` ou un argument.

Cible LoC après les 3 blocs : `commands/mod.rs` aux alentours de 3 000-4 000
LoC, contenant uniquement les helpers transverses et les types partagés.
Une PR ultérieure pourra ensuite déplacer ces helpers vers `state.rs`.

### Pourquoi ce travail n'a pas été exécuté dans la session du 2026-04-07

L'environnement utilisé pour cette session n'avait pas la toolchain Rust
disponible, donc impossible de valider `cargo check` après chaque
extraction. Un patch aveugle de plusieurs centaines de LoC sur 10 800 LoC
de Rust est trop risqué — la moindre `use` manquante casse la compilation
et les diagnostics ne sont pas instantanés.

Le front (`useIpc.ts`, `ResultsPage.tsx`, `ExpertPage.tsx`) a en revanche
été entièrement refactoré dans cette même session car `npx tsc --noEmit`
permettait une validation continue.

Pour reprendre Phase 4, il faut soit :
- Ouvrir une session avec `cargo` accessible et exécuter ce plan bloc par
  bloc avec validation entre chaque, soit
- Préparer chaque bloc localement et lancer manuellement `cargo check`
  entre chaque commit.

## Notes d'extraction

- **Conflit de nom** : `mod preview;` dans `commands/` entrait en collision
  avec `use crate::preview;` (le top-level). Solution : le sous-module a été
  renommé `file_preview` et l'export reste `pub use file_preview::*;`.
- **Conflit de nom** (idem) : `mod ai;` entrait en collision avec
  `use crate::ai;`. Comme aucune fonction de `mod.rs` n'utilise plus `ai::`
  (toutes ont été déplacées dans le sous-module), `ai` a été retiré du
  `use crate::{...}` global.
- **Helpers exposés en `pub(super)`** : `build_diagnostic`, `get_session`,
  `runtime_capabilities`, `app_build_info`, `build_file_preview`,
  `build_file_hex_preview`, `build_file_auxiliary_preview`,
  `build_file_auxiliary_hex_preview`, `save_file_auxiliary_payload_to_path`,
  ainsi que la struct `InventoryScanSession`. Ces helpers restent dans
  `mod.rs` pour l'instant (sub-pass 4 les déplacera vers `state.rs`).

## Sous-passe validation (I5 slice 1 — 2026-04-17)

Premier pas du découpage post-Sprint 2.1. Extraction minimale, 100 % mécanique, 0 changement de comportement.

**Livré** : `src-tauri/src/commands/validation.rs` contient désormais `normalize_scan_type` et `normalize_conflict_strategy` (pures, sans état). Re-exportées via `commands/mod.rs` (`pub(crate) use validation::{...}`) pour que les call sites dans `commands/scan.rs` (via `super::normalize_scan_type`) et `commands/export.rs` (via le `use super::{..., normalize_conflict_strategy, ...}` existant) continuent à compiler sans changement.

**Tests ajoutés** : 3 nouveaux tests dans `validation::tests` (couverture positive + rejet unknown + parcours complet des variants). Le seul test inline qui vivait dans `commands/mod.rs` a été retiré car doublon.

**Vérifié** : `cargo test --lib` = 295 tests verts (était 292 avant extract).

## Sous-passe export_worker (I5 slice 2 — 2026-04-17)

Deuxième pas du découpage. 8 fonctions déplacées de `mod.rs` vers `commands/export.rs`, aucun changement de comportement.

**Livré** :
- `run_export_session` (worker, ~370 LoC) + helpers locaux `fail_export_session`, `export_source_description` (privés au module).
- Les 3 derniers `#[tauri::command]` de `mod.rs` : `generate_recovery_report`, `export_results_csv`, `generate_lab_bundle`.
- 2 helpers purs `current_timestamp_rfc3339` et `html_escape` (privés, ne servaient qu'à ces 3 commandes).

**Promus `pub(super)` dans `mod.rs`** pour rester appelables depuis `export.rs` : `update_export_progress`, `push_export_error`, `export_recovered_file`, `export_resource_fork_sidecar`, `export_alternate_data_stream_sidecars`, `relative_dir_from_display_path`, `resolve_target_path`, `verify_exported_file`, `verify_reconstructed_export`.

**Import de `chrono::SecondsFormat`** retiré de `mod.rs` (n'était utilisé que par `current_timestamp_rfc3339`). Ajouté en tête de `export.rs`.

**Aucun changement** dans `lib.rs::generate_handler!` : les 3 `pub fn` nouvellement dans `export.rs` restent visibles comme `commands::*` grâce au `pub use export::*;` déjà en place dans `commands/mod.rs`.

**Vérifié** : `cargo fmt && cargo check` OK ; `cargo test --lib` = 295 tests verts ; `npm run rust:clippy` = 0 erreur (warn-level, baseline inchangé) ; `npx tsc --noEmit` clean ; `npx vitest run` = 29 tests verts.

**Bilan LoC** : `mod.rs` tombe de 9 300 à **8 427** (-873 LoC physiques) ; `export.rs` monte de 335 à **1 155** (+820 LoC). `mod.rs` n'héberge plus **aucun** point d'entrée Tauri.

**Prochaine slice** (suggestion) : extraire le cluster `scan_deleted_*` (5 workers `run_deleted_*_scan`, ~1 200 LoC) vers `commands/scan.rs` ou un nouveau `scan_deleted.rs` — la plus grosse masse restante dans `mod.rs` avec une logique homogène (tous suivent le même pattern `process_deleted_files` + scoring).

## Sous-passe imaging_helpers (Sprint 5, Chantier 76 — 2026-04-18)

Extraction mécanique du bloc de helpers imaging encore dans `commands/mod.rs` vers un nouveau fichier `commands/imaging_cmd/helpers.rs`. Aucun changement de comportement, aucune signature publique Tauri touchée.

**Livré** dans `commands/imaging_cmd/helpers.rs` :
- `ImagingSourcePlan` (enum + `impl` : `source_path`, `requires_elevation`).
- Résolveur de plan et prédicats d'élévation : `resolved_imaging_source_path`, `is_raw_device_path`, `is_permission_denied_imaging_error`, `imaging_requires_elevation_fallback`, `resolve_imaging_source_plan`.
- Profils et journalisation : `recommended_imaging_profile`, `recommended_imaging_profile_reason_key`, `append_imaging_profile_log`, `imaging_profile_for_session`.
- Helpers d'artefact : `imaging_unreadable_error_count`, `append_imaging_artifact_issue_logs`, `apply_imaging_artifact_issue_metrics`, `apply_imaging_artifact_session_details`.
- Progression et lecture de rapport : `update_image_acquisition_progress`, `read_u64_report`, `read_image_artifact_report` (macOS).
- Orchestrateur macOS privilégié `create_read_only_image_with_optional_elevation` + ses helpers internes `try_unmount_macos_device`, `run_macos_privileged_image_acquisition_for_recovery`.
- Inspection diagnostic : `inspect_potential_volumes_for_diagnostic`.

**Visibilité** : chaque fonction/type promu de `pub(super) fn` à `pub(crate) fn` pour permettre la ré-exportation depuis `commands/mod.rs` via `pub(super) use imaging_cmd::helpers::{...}`. Les prédicats `is_raw_device_path` et `is_permission_denied_imaging_error` restent privés au module `helpers`.

**Imports nettoyés dans `commands/mod.rs`** :
- suppression du `use imaging_cmd::privileged_macos::{...}` (plus utilisé par le code non-test depuis que l'orchestrateur a migré) ;
- `crate::core` et `crate::partitioning` retirés du `use crate::{...}` de tête (le premier n'a plus aucune référence, le second n'est plus utilisé qu'en tests et est importé dans `mod tests` via `use crate::partitioning;`) ;
- bloc `pub(super) use imaging_cmd::helpers::{...}` ajouté pour que `scan.rs`, `device.rs` et les tests inline continuent d'atteindre les helpers par leurs noms bruts via `super::<fn>`.

**Tests** : le test `imaging_requires_elevation_fallback_only_for_permission_denied_raw_devices` et `build_macos_privileged_imager_script_quotes_paths_safely` gardent leur emplacement inline dans `commands::tests`. Imports ajoutés en tête du bloc : `use super::imaging_cmd::helpers::imaging_requires_elevation_fallback;` et `#[cfg(target_os = "macos")] use super::imaging_cmd::privileged_macos::build_macos_privileged_imager_script;`.

**Vérifié** : `cargo fmt` ; `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **325 tests verts** ; `npx tsc --noEmit` propre ; `npm run test:ui` = **54 tests verts**.

**Bilan LoC** : `commands/mod.rs` tombe de **7 961 → 7 444** (−517 LoC physiques), `commands/imaging_cmd/mod.rs` stable à ~468 lignes, nouveau fichier `commands/imaging_cmd/helpers.rs` à **555 LoC**. La cible `mod.rs < 1 500` reste à atteindre via la prochaine passe (extraction des workers scan `run_deleted_*_scan`, `run_potential_volume_scan`, `run_signature_carving_scan`, `run_inventory_scan`, ~2 700 LoC).

## Sous-passe scan_deleted_fat_family (Sprint 5, Chantier 76 — 2026-04-18)

Extraction mécanique de 3 workers de `commands/mod.rs` vers `commands/scan.rs`. Ordre choisi : FAT32/exFAT/NTFS ensemble (706 lignes), les autres workers (ext4/HFS+/APFS + carving + inventory + potential-volume) suivent dans les slices ultérieures.

**Livré** (ajouté à `commands/scan.rs`) :
- `pub(crate) fn run_deleted_fat32_scan` (~180 lignes).
- `pub(crate) fn run_deleted_exfat_scan` (~180 lignes).
- `pub(crate) fn run_deleted_ntfs_scan` (~345 lignes, inclut les chemins USN/MFT mirror + correlate_recovery_sources + ADS).

**Imports ajoutés dans `commands/scan.rs`** : `crate::analyzers::{exfat, fat32, ntfs}` + bloc `use super::{append_imaging_artifact_issue_logs, append_scan_log, apply_imaging_artifact_issue_metrics, apply_imaging_artifact_session_details, create_read_only_image_with_optional_elevation, elapsed_seconds, fail_scan_session, finalize_cancelled_scan, imaging_profile_for_session, persist_scan_session, unix_timestamp_ms, update_progress, wait_for_scan_permission}` pour que les workers accèdent aux helpers session/imagerie par leurs noms bruts. Les appels pleinement qualifiés (`crate::commands::state::lock_or_recover`, `crate::correlation::correlate_recovery_sources`, `crate::fallback::recover_ntfs_from_mft_mirror`) restent inchangés.

**Ré-export dans `commands/mod.rs`** : `pub(super) use scan::{run_deleted_exfat_scan, run_deleted_fat32_scan, run_deleted_ntfs_scan}` ajouté au bloc existant pour que le dispatcher `start_scan` (qui utilise encore `super::run_deleted_*_scan`) continue de résoudre + que les tests inline de `commands::tests` y accèdent via `use super::*`.

**Vérifié** : `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **325 verts** (inchangé).

**Bilan LoC** : `commands/mod.rs` **7 444 → 6 737** (−707), `commands/scan.rs` **737 → 1 452** (+715).

## Sous-passe scan_deleted_unix_family (Sprint 5, Chantier 76 — 2026-04-18)

Deuxième lot de 3 workers, symétrique du premier.

**Livré** (ajouté à `commands/scan.rs`) :
- `pub(crate) fn run_deleted_ext4_scan` (~319 lignes, inclut fallback `crate::fallback::recover_ext4_from_backup_superblock` + journal ext4).
- `pub(crate) fn run_deleted_hfsplus_scan` (~237 lignes).
- `pub(crate) fn run_deleted_apfs_scan` (~180 lignes).

**Imports** : `scan.rs` étend son analyzer block à `{apfs, exfat, ext4, fat32, hfsplus, ntfs}`. Aucun nouvel import de helper session nécessaire (déjà couvert par la slice précédente).

**Nettoyage `commands/mod.rs`** : `analyzers::ext4` retiré du `use crate::{...}` de tête (non-test n'y touche plus), déplacé vers `mod tests` (seuls les tests ext4 synthétiques le référencent encore).

**Ré-export dans `commands/mod.rs`** : ajout de `run_deleted_apfs_scan, run_deleted_ext4_scan, run_deleted_hfsplus_scan` au bloc `pub(super) use scan::{...}`.

**Vérifié** : `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **325 verts**.

**Bilan LoC** : `commands/mod.rs` **6 737 → 6 001** (−736), `commands/scan.rs` **1 452 → 2 189** (+737).

## Sous-passe scan_workers_tail (Sprint 5, Chantier 76 — 2026-04-18)

Troisième lot : les workers restants — `run_inventory_scan`, `run_signature_carving_scan`, `run_potential_volume_scan` — plus leurs helpers privés de découpe de volumes potentiels.

**Livré** (ajouté à `commands/scan.rs`) :
- `pub(crate) fn run_potential_volume_scan` (~725 lignes) : orchestre snapshot de la source, extraction de la slice read-only, puis bascule vers le worker de filesystem détecté (FAT/exFAT/NTFS/ext4/HFS+/APFS) ou fallback inventory.
- `pub(crate) fn run_signature_carving_scan` (~181 lignes) : appelle `carving::carve_signatures`.
- `pub(crate) fn run_inventory_scan` (~232 lignes) : utilise la constante `QUICK_SCAN_MAX_DEPTH` (2), déplacée de `mod.rs` vers le début de `scan.rs`.
- 5 helpers privés au module (`fn`) : `potential_volume_source_snapshot_path`, `potential_volume_slice_path`, `potential_volume_slice_length`, `rebase_slice_offset`, `recovered_file_from_slice`.

**Imports ajoutés dans `commands/scan.rs`** :
- `std::fs` ;
- `crate::{carving, imaging}` ;
- types partagés additionnels : `ByteRun, FileFork, NamedFileFork, PotentialVolume` ;
- `super::{filesystem_label, imaging_cmd, ImagingSourcePlan}` pour appeler le snapshot local (`imaging_cmd::create_local_image_snapshot`), le label de filesystem, et typer le plan imaging en entrée.
- `const QUICK_SCAN_MAX_DEPTH: usize = 2;` ré-écrit en tête de fichier.

**Nettoyage `commands/mod.rs`** :
- suppression du `use crate::{analyzers::{...}, carving, imaging, types::*}` complet — simplifié en `use crate::types::*;` (tous les analyzers, `carving`, `imaging` n'étaient plus utilisés qu'en tests) ;
- suppression de `use std::{sync::{Arc, Mutex}, path::{Path, PathBuf}, time::SystemTime};` — seul `std::{fs, io::{Cursor, Write}, path::Path}` reste nécessaire pour le support-bundle builder + reporting ;
- suppression de `use state::{InventoryScanSession, MAX_SESSION_LOGS};` — non-test n'y touche plus ;
- suppression de la const `QUICK_SCAN_MAX_DEPTH` (déplacée dans `scan.rs`) ;
- nettoyage du bloc `pub(super) use scan::{...}` : gardé `guess_mime_type` + ajout de `run_inventory_scan, run_potential_volume_scan, run_signature_carving_scan` ; `compute_progress` déplacé en `#[cfg(test)] use scan::compute_progress;` (seuls les tests le référencent encore) ; `display_parent_path, is_previewable_extension, register_scan_error` supprimés (plus aucune référence).
- `mod tests` reçoit les imports manquants : `crate::analyzers::{apfs, ext4, hfsplus, ntfs}`, `crate::imaging`, `std::path::PathBuf`, `std::sync::{Arc, Mutex}`, ainsi que `InventoryScanSession, MAX_SESSION_LOGS` déplacés dans le `use super::state::{...}`.

**Vérifié** : `cargo fmt` ; `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **325 verts** ; `npx tsc --noEmit` propre ; `npm run test:ui` = **54 verts**.

**Bilan LoC** : `commands/mod.rs` **6 001 → 4 750** (−1 251) ; `commands/scan.rs` **2 189 → 3 441** (+1 252). `commands/mod.rs` passe de **7 961 LoC en début de Sprint 5 à 4 750 LoC** (−3 211, −40 %). La cible `< 1 500` reste à atteindre via des sorties futures : support-bundle builder (~200 LoC), recovery/narrative reports, CSV export, lab bundle, `supported_potential_volume_filesystem` + rankings, helpers d'écriture disque, les tests inline (~3 000 LoC à migrer vers `tests/`).

## Sprint 7 — clôture Chantier 76 (2026-04-18)

Quatre tranches enchaînées sur une seule session ; `commands/mod.rs` passe de **4 750 LoC à 211 LoC** (−4 539, −95.6 %). Cible `< 1 500` écrasée.

### T1 — Migration du bloc `mod tests` inline vers `commands/tests.rs`

Le plus gros bloc de `mod.rs` (lignes 506–4750, soit **4 243 LoC** au format `#[cfg(test)] mod tests { ... }`) n'était qu'un conteneur de tests inline. Voie choisie (B1 / in-tree) : déplacer le contenu tel quel dans un fichier frère `commands/tests.rs` et remplacer par `#[cfg(test)] mod tests;` dans `mod.rs`.

**Livré** :
- `src-tauri/src/commands/tests.rs` créé avec les **4 242 lignes** du corps du module (imports `use super::*;`, `use super::state::{...}`, `use super::imaging_cmd::helpers::...`, etc., + les 70+ fonctions `#[test]` et les helpers de fixture `unique_temp_dir`, `sample_recovered_file`, `scan_session_for_root`, …).
- `commands/mod.rs` : remplacement des lignes 506–4750 par `#[cfg(test)]\nmod tests;` (−4 243 ; +2).

**Visibilité** : zéro changement. Les tests s'appuient déjà sur `use super::*;` qui capture les items `pub(super)` et `pub(crate)` de `mod.rs`, inchangés.

**Vérifié** : `cargo fmt` + `cargo check --all-targets` (0 warning) + `cargo test --lib` = **334 verts** + `npx tsc --noEmit` propre + `npm run test:ui` = **54 verts**.

**Bilan LoC T1** : `commands/mod.rs` **4 750 → 507** (−4 243). Cible `< 1 500` atteinte dès cette tranche.

### T2 — Extraction du support-bundle builder vers `commands/support_bundle.rs`

Bloc sorti : `SupportBundleManifest` (struct privée) + les 6 fns du pipeline zip (`build_support_bundle_archive_bytes`, `add_json_bundle_entry`, `add_text_bundle_entry`, `add_binary_bundle_entry`, `format_technical_logs`, `sanitize_bundle_segment`).

**Livré** :
- `src-tauri/src/commands/support_bundle.rs` (145 LoC) : module dédié avec `SupportBundleManifest` en privée-module et `build_support_bundle_archive_bytes` en `pub(crate)`. Les 5 helpers zip/log sont privés au module.
- `commands/mod.rs` : déclaration `mod support_bundle;` + ré-export `pub(super) use support_bundle::build_support_bundle_archive_bytes;`. Nettoyage des imports orphelins (`serde::Serialize`, `std::io::{Cursor, Write}`, `zip::{...}`).
- Imports amenés dans le module : `super::export::{get_export_history, get_export_logs}`, `super::runtime::{app_build_info, runtime_capabilities}`, `super::scan::{get_scan_history, get_scan_logs}`, `super::state::{export_sessions, lock_or_recover, scan_sessions, unix_timestamp_ms}`.

**Appelants conservés par ré-export** : `commands/export.rs:254` (`super::build_support_bundle_archive_bytes()?`) + tests inline via `use super::*;`.

**Bilan LoC T2** : `commands/mod.rs` **507 → 360** (−147).

### T3 — Fusion des helpers d'écriture disque dans `commands/state.rs`

Bloc sorti : `write_text_report_to_path` + `write_binary_file_to_path` (écriture atomique `.tmp` → `rename`, ~65 LoC).

**Livré** :
- `commands/state.rs` : ajout des 2 fns en `pub(crate)` juste après `unix_timestamp_ms`. `fs` et `Path` déjà importés en tête du fichier.
- `commands/mod.rs` : retrait du bloc + extension du ré-export `pub(super) use state::{…, write_binary_file_to_path, write_text_report_to_path};`.
- Imports `use std::{fs, path::Path};` conservés en `#[cfg(test)]` pour les tests inline qui les réutilisent via `use super::*;`.

**Appelants conservés par ré-export** : `commands/export.rs:230` (`super::write_text_report_to_path(...)`) + `commands/export.rs:255` (`super::write_binary_file_to_path(...)`) + tests.

**Bilan LoC T3** : `commands/mod.rs` **360 → 296** (−64).

### T4 — Déplacement des helpers de ranking potential-volume vers `commands/scan.rs`

Bloc sorti : les 5 fns de sélection/classement (`supported_deleted_recovery_filesystem`, `supported_potential_volume_filesystem`, `potential_volume_detection_rank`, `best_supported_potential_volume`, `guided_supported_potential_volume_candidate`).

**Livré** :
- `commands/scan.rs` : ajout du bloc en fin de fichier. Les 3 fns jusqu'alors en `pub(super)` passent à `pub(crate)` (convention établie dans le fichier). Les 2 privées (`supported_deleted_recovery_filesystem`, `potential_volume_detection_rank`) restent privées au module.
- `commands/scan.rs:247` : appel `super::supported_potential_volume_filesystem(...)` simplifié en appel direct local.
- `commands/mod.rs` : retrait du bloc + extension du `pub(super) use scan::{...}` avec `best_supported_potential_volume, guided_supported_potential_volume_candidate, supported_potential_volume_filesystem`.
- Nettoyage du top-level `use crate::types::*;` — plus aucun usage hors tests ; rendu `#[cfg(test)]` pour que `tests.rs` continue d'y accéder via `use super::*;`.

**Appelants conservés par ré-export** : `commands/device.rs:261, 263, 265` (`super::supported_potential_volume_filesystem`, `super::best_supported_potential_volume`, `super::guided_supported_potential_volume_candidate`) + tests inline.

**Bilan LoC T4** : `commands/mod.rs` **296 → 211** (−85).

### Bilan Sprint 7

`commands/mod.rs` : **4 750 → 211 LoC** (−4 539, **−95.6 %**). Cible historique `< 1 500` atteinte dès T1, puis ramenée ~7× en-dessous.

| Fichier | Avant Sprint 7 | Après Sprint 7 | Delta |
|---|---|---|---|
| `commands/mod.rs` | 4 750 | 211 | −4 539 |
| `commands/tests.rs` (nouveau) | — | 4 208 | +4 208 |
| `commands/support_bundle.rs` (nouveau) | — | 145 | +145 |
| `commands/scan.rs` | 3 441 | 3 528 | +87 |
| `commands/state.rs` | 1 078 | 1 145 | +67 |

**Validé à chaque tranche** : `cargo fmt` ; `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **334 verts** ; `npx tsc --noEmit` propre ; `npm run test:ui` = **54 verts**.

**Ce qui n'a pas été fait** (hors périmètre Chantier 76) : l'éclatement de `commands/tests.rs` (4 208 LoC) par domaine (un `tests.rs` par module frère) reste ouvert si la maintenabilité du fichier monobloc devient un point dur, mais n'apporte plus de gain sur la cible LoC de `mod.rs`. `commands/mod.rs` est désormais un simple fichier d'agrégation : déclarations de sous-modules, ré-exports `pub(super) use` pour les call sites `super::<fn>` hérités, et la constante partagée `HEX_PREVIEW_LINE_WIDTH`.
