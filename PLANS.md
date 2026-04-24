# PLANS.md

## Rôle du document
Ce document contient les plans d'exécution des fonctionnalités complexes du projet.  
Chaque plan doit être compréhensible, exécutable, vérifiable, et mis à jour pendant l'avancement.

---

# Template de plan d'exécution

## 1. Nom du chantier
Nom court et descriptif.

## 2. Objectif
Décrire précisément le résultat attendu côté utilisateur et côté système.

## 3. Pourquoi
Expliquer la valeur produit, les risques adressés, et la raison de la priorité.

## 4. Périmètre
Inclure :
- ce qui est couvert,
- ce qui n'est pas couvert,
- dépendances,
- hypothèses.

## 5. Contraintes
- sécurité
- performance
- accessibilité
- confidentialité
- compatibilité multiplateforme
- contraintes de maintenance

## 6. Architecture concernée
Lister les modules concernés :
- core
- imaging
- analyzers
- carving
- scoring
- ai
- preview/export
- desktop app
- shared contracts

## 7. Contrats et interfaces
Décrire les interfaces entre modules, entrées/sorties, types partagés, erreurs attendues.

## 8. UX / UI
Décrire :
- écrans touchés,
- composants,
- états,
- messages critiques,
- comportements d'erreur,
- novice vs expert.

## 9. Étapes d'implémentation
Découpage séquentiel clair, avec ordre d'exécution recommandé.

## 10. Tests et validation
Inclure :
- tests unitaires,
- tests d'intégration,
- tests e2e si utile,
- cas limites,
- critères de "done".

## 11. Risques
Lister les risques techniques, UX, produit, sécurité.

## 12. Questions ouvertes
Tout point encore incertain.

---

# Plans actifs

## Chantier 90 — Directives concurrentielles audit 2026-04-24
**Objectif** : transformer les directives "depasser DiskDrill / R-Studio / TestDisk / EaseUS / Stellar / Recoverit" en livrables verifiables, en commencant par le differenciateur legal/forensic le plus concret : un manifeste d'export signe.

**Hypotheses** :
- les directives D1-D12 representent une roadmap produit de plusieurs semaines ; cette tranche ne doit pas pretendre fermer RAID 50/60, ZFS, OPAL/UFS ou open-core ;
- D3 est deja partiellement couvert par `SECURITY.md` et le module Gemma/Ollama local, mais l'export signe D2 n'est pas encore materialise ;
- l'attestation doit rester locale : aucune cle privee serveur, aucun cloud, aucun envoi de metadonnees.

**Risques** :
- une cle d'attestation locale stockee dans le keyring prouve la continuite de l'installation, pas une identite legale externe ;
- le manifeste contient des noms et chemins exportes : il est ecrit dans la destination choisie par l'utilisateur, pas dans un canal support automatique ;
- si le keyring OS est indisponible, l'export doit rester possible avec une signature ephemere explicitement marquee.

**Modules impactes** :
- `src-tauri/src/commands/export.rs` pour produire `MANIFEST.json` signe ;
- `src-tauri/src/commands/tests.rs` pour verifier presence, hashes et signature ;
- `docs/top-tier-roadmap.md` / `SECURITY.md` si une limite ou une preuve change.

**Plan d'execution** :
1. Ajouter une cle locale d'attestation Ed25519, persistee dans le keyring OS quand possible, ephemere sinon -> verification : test sans dependre du keyring.
2. Collecter chaque artefact exporte (fichier principal, resource fork, ADS) avec taille, SHA-256, methode, statut et offsets/runs disponibles -> verification : test export existant etendu.
3. Ecrire `MANIFEST.json` atomiquement dans la racine d'export, signe sur le payload canonique -> verification : lecture JSON + verification Ed25519 dans test.
4. Tracer le chemin du manifeste dans les logs d'export et documenter la limite "cle locale, pas HSM" -> verification : logs + PLANS.

**Criteres de validation** :
- chaque export reussi avec au moins un fichier produit un `MANIFEST.json` ;
- le manifeste contient au moins `schema`, `export_id`, `scan_id`, version app, timestamp, source hash, liste de fichiers, SHA-256, signature Ed25519 et public key ;
- les tests Rust export restent verts, sans acces au disque source en ecriture.

**Limites connues** :
- pas encore de signature HSM, cle cabinet, certificat qualifie eIDAS ou workflow notarial ;
- pas encore de typage read-only P2.1 ni de page marketing publique ;
- les directives D4/D8/D12 restent des chantiers profonds, non fermes par cette tranche.

**Statut 2026-04-24** :
- termine pour la tranche D2 : `MANIFEST.json` signe Ed25519, hashes SHA-256, offsets/runs source disponibles, version app et timestamp ;
- valide par tests Rust export, suite Rust complete, clippy, build UI, lint et benchmark manifest/results ;
- D1/D4-D12 restent une roadmap produit documentee, sans promesse marketing prematuree.

## Chantier 89 — Corrections audit securite 2026-04-24
**Objectif** : fermer les vulnerabilites P0/P1 applicables localement dans `AUDIT_SECURITE_2026_04_24.md` sans modifier le moteur de lecture source, et rendre les decisions importantes tracables par tests, preflight et logs.

**Hypotheses** :
- le depot courant est la branche principale a pousser vers `https://github.com/kindahall/recupere.git` apres validation ;
- les corrections doivent privilegier les garde-fous locaux immediats plutot qu'une refonte de 6-10 semaines ;
- les points qui demandent une infrastructure externe (revocation list, signature Apple/Windows reelle, updater GA, MDM, dashboard privacy complet) restent documentes comme limites de release.

**Risques** :
- le stockage licence via keyring peut rendre une ancienne licence plaintext indisponible si le keyring OS est absent ou verrouille ;
- le durcissement remote peut refuser des agents LAN auparavant acceptes, volontairement au profit de TLS/SSH tunnel ;
- les exports deviennent plus stricts sur les chemins, symlinks et noms reserves Windows.

**Modules impactes** :
- licence : `src-tauri/src/license`, `src-tauri/build.rs`, `src-tauri/src/bin/gen_license.rs` ;
- preview/export/repair : `src-tauri/src/commands/export.rs`, `file_preview.rs`, `repair_cmd.rs`, `state.rs` ;
- remote : `src-tauri/src/remote`, `crates/recupere-agent/src/http.rs` ;
- IA locale : `src-tauri/src/commands/ai.rs`, `src-tauri/src/cloud_ai/mod.rs` ;
- packaging : `src-tauri/capabilities/default.json`, `src-tauri/entitlements.plist`, `scripts/release-preflight.mjs` ;
- validation/tests : tests Rust cibles, `npm run release:preflight`, checks Cargo.

**Plan d'execution** :
1. Durcir la licence : fingerprint avec identifiants systeme stables, keyring, suppression plaintext, build guard tous profils non-debug, seed dev hors binaire public -> verification : tests licence/build script.
2. Fermer les ecritures dangereuses : export atomique via fichiers temporaires, validation repair/export, noms reserves Windows, CSV formula injection, rapports hors `/tmp` predictible -> verification : tests commandes export.
3. Durcir remote : URL via `url`, pas de redirects, cap JSON, telechargement sans append implicite, hash SHA-256 des pulls -> verification : tests URL/client/agent.
4. Encadrer IPC/preview/IA : bornes d'entree, source canonicalisee sous root, prompts avec contenu utilisateur delimite et echappe -> verification : tests prompts et preview/export.
5. Ajuster packaging : opener scope, entitlements hardened runtime, preflight signatures/licence -> verification : `npm run release:preflight`.

**Criteres de validation** :
- aucun changement n'autorise une ecriture sur disque source ;
- les exports et repairs refusent les destinations non validees ou hors racine canonique ;
- la licence n'est plus sauvegardee en clair par defaut ;
- un agent distant ne peut plus provoquer redirect-token leak, OOM JSON simple, ou pull sans verification de taille/hash ;
- les tests cibles passent, ou les limites restantes sont explicites.

**Limites connues** :
- pas de TPM/Secure Enclave direct ni revocation online dans cette tranche ;
- pas de dashboard privacy complet ni flow premier lancement React entier ;
- pas de signature Apple/Windows locale sans secrets de release.

**Statut 2026-04-24** : tranche securite livree. Les corrections locales P0/P1 applicables ont ete implementees : licence keyring + migration plaintext, fingerprint OS, seed dev hors API publique, garde build non-debug, export/repair atomiques, URL remote stricte, cap JSON, verification SHA-256 des pulls, bornes IPC, prompt escaping, opener scope, entitlements reduits, purge PII avec confirmation et marqueur d'audit.

**Validation 2026-04-24** : `npm run build` OK ; `npm run test:ui` 67 tests OK ; `npm run lint` OK ; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -W warnings` OK ; `cargo test --manifest-path src-tauri/Cargo.toml` 360 passed / 3 ignored ; `cargo test --manifest-path crates/recupere-agent/Cargo.toml` OK ; `npm run rust:fmt` OK ; `npm run release:preflight` PASS avec 4 warnings attendus (cle licence release absente, updater/signing release non configures localement).

## Chantier 88 — Professional Recovery UX + Benchmark Parity
**Objectif** : rapprocher l'expérience de résultats, d'export, de preview/repair et de preuve benchmark d'un produit commercial sérieux, sans promettre une récupération certaine ni masquer les limites moteur.

**Hypothèses** :
- les couches principales existent déjà : analyzers Rust, preview/export, UI résultats, benchmark workspace, rescue MVP et preflight ;
- cette passe doit améliorer l'exposition produit et les preuves sans gros refactor moteur ;
- les statuts moteur inconnus doivent être affichés comme incertains plutôt que forcés dans une catégorie plus sûre.

**Risques** :
- sur-vendre la réparation ou l'IA serait dangereux ; tout wording doit rester conservateur ;
- un export de résultat incertain doit rester possible uniquement vers une destination validée, avec vérification recommandée ;
- la preuve benchmark doit rendre les scénarios bloqués visibles au lieu de produire un score marketing unique.

**Modules impactés** :
- desktop app : `src/pages/ResultsPage.tsx`, composants résultats/export/preview, i18n ;
- shared contracts UI : types résultats et filtres ;
- benchmark/release docs : scripts benchmark, README/docs ;
- pas de modification low-level source-disk prévue dans cette tranche.

**Plan d'exécution** :
1. Auditer l'existant UX/moteur/sécurité/tests/release → vérification : écarts explicites dans ce plan et rapport final.
2. Ajouter une catégorie UI `uncertain` pour les résultats moteur inconnus ou insuffisamment qualifiés → vérification : tests de filtrage et i18n.
3. Renforcer l'écran résultats/export : filtre d'intégrité explicite, compteur incertain, avertissements export/preview/repair honnêtes → vérification : `npm run test:ui`.
4. Ajouter un résumé benchmark JSON + Markdown généré localement depuis les résultats existants → vérification : `npm run benchmark:check`.
5. Documenter la commande et les limites benchmark/rescue/release sans inventer d'infrastructure absente → vérification : lecture docs + preflight.

**Critères de validation** :
- aucun changement n'autorise l'écriture sur le disque source ;
- aucun texte ne promet une récupération ou réparation certaine ;
- i18n anglais/français à parité ;
- lint/build/tests/preflight/clippy/cargo checks exécutés autant que possible et échecs non masqués.

**Limites connues** :
- cette tranche ne crée pas un ISO bootable maison ;
- cette tranche ne ferme pas les gaps moteur APFS/degraded imaging/RAID lab ;
- les benchmarks restent limités par les fixtures et résultats actuellement disponibles.

## Chantier 86 — Durcissement sécurité et production avant release
**Objectif** : Fermer les failles critiques relevées lors de l'audit sécurité strict, remettre les validations build/lint/test dans un état exploitable, et ramener les flux sensibles au niveau attendu pour une application professionnelle de récupération de données.

**Pourquoi** :
- l'agent distant expose actuellement des primitives de lecture/suppression de fichiers trop larges ;
- l'export manipule des chemins provenant de supports potentiellement hostiles ;
- certaines opérations privilégiées et d'unlock peuvent exposer des secrets ou modifier l'état système ;
- l'application doit rester prudente, traçable, et honnête sur les limites de récupération.

**Périmètre** :
- couvert : agent HTTP, export, remote client, commandes Tauri sensibles, audit/logs, previews, lint/build/tests critiques ;
- non couvert : refonte UI complète, nouveau moteur de récupération, changement de stack, distribution publique signée ;
- hypothèses : les corrections doivent rester chirurgicales et conserver les contrats UI existants quand ils sont sûrs ;
- dépendances : `src-tauri`, `crates/recupere-agent`, frontend React uniquement si un contrat IPC change.

**Contraintes** :
- ne jamais écrire sur le disque source ;
- ne jamais restaurer dans un chemin source ou hors destination validée ;
- traiter tous les noms de fichiers récupérés comme hostiles ;
- journaliser les décisions importantes sans exposer de secrets ;
- préférer la désactivation d'une capacité risquée à une correction partielle.

**Architecture concernée** :
- preview/export : validation de destination, normalisation de chemins, intégrité ;
- remote agent : endpoints minimaux, pas d'accès fichier arbitraire ;
- desktop app : commandes IPC sensibles ;
- ai services : endpoints locaux/privés uniquement par défaut ;
- audit/production : validations CI, logs, dépendances.

**Contrats et interfaces** :
- l'agent distant ne doit servir que des artefacts générés et enregistrés par l'agent ;
- un export ne peut écrire que sous la destination validée ;
- un nom de fichier récupéré ne peut jamais influencer un chemin parent ;
- les mots de passe ne doivent pas transiter par des arguments de processus ;
- toute commande qui modifie l'état système doit être explicite, isolée et testée.

**Étapes d'implémentation** :
1. Rendre les validations de base vertes : agent, lint, clippy quand le changement reste local.
2. Supprimer ou restreindre les endpoints fichier arbitraires de l'agent.
3. Durcir la construction des chemins d'export et ajouter des tests path traversal.
4. Neutraliser les commandes sensibles exposées ou les secrets en arguments.
5. Durcir les sorties locales, rapports, previews et logs.
6. Relancer build/test/audit et documenter les limites restantes.

**Tests et validation** :
- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` ;
- `cargo check --manifest-path crates/recupere-agent/Cargo.toml` ;
- tests Rust ciblés export/remote/security ;
- `npm run test:ui` ;
- `npm run build` ;
- lint/clippy si le périmètre permet de les rendre verts sans refactor massif.

**Risques** :
- certaines API distantes peuvent devenir moins permissives, ce qui est volontaire ;
- l'ancien client remote peut nécessiter une adaptation si une primitive dangereuse est retirée ;
- le durcissement complet production peut nécessiter plusieurs passes.

**Questions ouvertes** :
- faut-il supprimer définitivement l'agent distant V1 ou le garder en mode local-only jusqu'a une V2 capability-based ?
- faut-il exiger une signature externe pour l'audit trail avant toute release payante ?

## État réel au 2026-04-18

Ce bloc a été ajouté après un audit de cohérence entre PLANS.md et le code. Les 3 seuls chantiers cochés ✅ dans le document d'origine (1, 2, 3) ne reflètent pas l'état livré : plusieurs sprints ont livré une bonne partie du backlog sans mettre à jour les cases.

### Sprints livrés (source : `~/.claude/projects/.../memory/`)

| Sprint | Date | Livraison principale | Chantiers concernés |
|---|---|---|---|
| Sprint 1 | 2026-04-16 | P0 fixes : license guard, variantes Gemma, signing runbook, flag `__ALLOW_BROWSER_PREVIEW__`, O_RDONLY explicite | 4 (imagerie read-only), 45 (IA locale), 55 (preflight) |
| Sprint 2 & 3 | 2026-04-16 | ~3 vagues de durcissement ; 279 tests Rust / 29 UI verts | 77, 78 (audit), 11 (tests critiques) |
| Session 2026-04-17 | 2026-04-17 | Slice `export_worker` + `raw_disks` macOS+Windows ; 303 tests Rust | 9 (export), cross-platform |
| Sprint 4 | 2026-04-17 | pro-plus + **Chantier 82** filesystem memory ; 325 Rust / 54 UI | **82 ✅** |
| Sprint 5 (4 passes A1) | 2026-04-18 | `imaging_helpers` + scan FAT/exFAT/NTFS + ext4/HFS+/APFS + potential-volume/carving/inventory ; `mod.rs` 7 961 → 4 750 LoC | 5, 8, 21, 22, 30, 31, 32, 35, 36, 37, 38, 39, 46, 49, 52, 54, 59, 60, 61, 63, 64 |
| Sprint 5 B6 | 2026-04-18 | Scheduler passif `filesystem_memory` complet (thread + mpsc + policy.json + 2 cmds + UI SettingsPage + i18n FR/EN) ; 334 Rust / 54 UI | 82 (complément), 84 |
| Sprint 6 A3 | 2026-04-18 | **Chantier 83** cross-platform (tauri-driver Linux+Windows + Appium Mac2 macOS, 7 specs WebdriverIO, 3 jobs CI) | **83 ✅** |
| Sprint 7 | 2026-04-18 | **Chantier 76** finalisation découpage `commands/mod.rs` 4 750 → 211 LoC (−95,6 %) | **76 ✅** |

### État effectif du code au 2026-04-18 (audit statique)

- **Backend Tauri** : 101 commandes exposées dans [src-tauri/src/lib.rs:126-228](src-tauri/src/lib.rs#L126-L228) — 100 sont invoquées depuis l'UI ([src/hooks/ipc/](src/hooks/ipc/)). 2 seulement ne sont pas branchées côté UI : `get_recent_traces`, `clear_recent_traces` (réservées au support bundle opérateur). Zéro `unimplemented!()`, zéro `todo!()`.
- **Frontend React** : 10 pages, 0 page morte, 0 stub, i18n FR/EN à parité stricte, modes novice/expert implémentés et gated.
- **Architecture AGENTS.md** : toutes les couches attendues sont présentes (core, imaging, analyzers FAT32/exFAT/NTFS/ext4/HFS+/APFS, carving, scoring, ai/cloud_ai, preview, export, types partagés).
- **Tests** : 334 tests Rust + 54 UI Vitest verts au dernier sprint ; E2E natif via tauri-driver + Appium Mac2.
- **Modules déclarés mais non câblés** : `raid/`, `encryption/`, `correlation/`, `fallback/`, `virtual_disk/` (stub VHDX notamment) — ne bloquent pas le flux principal.

### Conséquence pour la suite

Les cases ✅ TERMINÉ des Chantiers 1, 2 et 3 d'origine sont **conservatrices** par rapport à la réalité. Les chantiers 4 à ~72 et 76, 82, 83 ont été majoritairement livrés ou substantiellement câblés (backend + UI + i18n + tests), mais **leurs critères de done n'ont pas été revérifiés ligne à ligne** — aucun ✅ n'a donc été apposé au-delà des trois certifiés par la memory (76, 82, 83 cochés ci-dessous).

Avant tout nouveau chantier, il faut un pass critère-par-critère pour savoir exactement ce qui reste à câbler. Voir la liste des vrais manques produite en fin d'audit (modules `raid`/`encryption`/`correlation`/`fallback`/`virtual_disk`, 2 endpoints UI non câblés, etc.).

---

## Chantier 1 — Fondations du repo ✅ TERMINÉ
**Objectif** : Initialiser le monorepo Tauri 2 + React + TypeScript, définir les packages, types partagés, configurer lint/test/build, poser l'ossature.

**Critères de done** :
- Structure du repo stable
- Compilation locale
- Conventions documentées
- AGENTS.md + PLANS.md en place

## Chantier 2 — Shell UI desktop ✅ TERMINÉ
**Objectif** : Construire la navigation, créer les écrans vides, définir le design system, créer les composants de base, poser la séparation novice/expert.

**Critères de done** :
- App navigable
- Composants cohérents
- États vides/chargement/erreur présents
- Tokens et layout documentés
- Light mode par défaut, dark mode alternatif

## Chantier 3 — Pipeline diagnostic simulé ✅ TERMINÉ
**Objectif** : Créer un flux faux mais structuré de détection support → diagnostic → recommandation, sans vraie récupération.

**Critères de done** :
- UX testable de bout en bout
- Contrats backend/frontend stabilisés
- Scoring affiché sous forme d'estimation

## Chantier 4 — Moteur d'imagerie read-only
**Objectif** : Implémenter l'acquisition et l'imagerie disque sécurisées, journaliser les opérations, gérer les erreurs I/O et reprise.

**Critères de done** :
- Aucune écriture sur la source
- Logs fiables
- Tests de robustesse minimaux

## Chantier 5 — Analyse NTFS/FAT/exFAT MVP
**Objectif** : Lecture de structures de base, détection de fichiers supprimés, premiers résultats exploitables.

**Critères de done** :
- Récupération MVP sur cas simples
- Séparation nette entre analyzers et UI
- Tests sur échantillons

## Chantier 6 — Preview / export sécurisé
**Objectif** : Prévisualisation, filtres, export vers une destination sûre.

**Critères de done** :
- Preview robuste
- Interdiction de restaurer sur source
- Messages de sécurité visibles

## Chantier 7 — Dé-mockage des écrans desktop
**Objectif** : Supprimer les comportements simulés visibles dans l'application desktop, relier l'UI aux capacités réellement disponibles du backend, et afficher explicitement les fonctionnalités non encore implémentées au lieu de faux résultats.

**Pourquoi** :
- l'application ne doit pas donner l'illusion d'une récupération fonctionnelle en production alors que le moteur réel n'est pas prêt ;
- les écrans doivent rester fiables dans un domaine sensible où un faux positif UX est dangereux ;
- la base Tauri/React doit devenir un shell honnête avant l'arrivée des moteurs d'imagerie, scan et export.

**Périmètre** :
- couvert : commandes Tauri exposées au frontend, états des pages `Home`, `Devices`, `Diagnostic`, `Scan`, `Results`, `Export`, `History`, `Expert`, `Settings`, store global et traductions associées ;
- non couvert : implémentation complète des moteurs low-level de scan, carving, preview, export et imagerie ;
- dépendances : `src-tauri/src/core/mod.rs`, `src/hooks/useIpc.ts`, `src/stores/appStore.ts`, pages React, locales i18n ;
- hypothèses : la détection des supports et la validation basique d'une destination sont les seules capacités réellement exploitables dans ce build.

**Contraintes** :
- ne jamais afficher de faux fichiers récupérés ;
- ne jamais lancer de flux qui écrit sur le disque source ;
- distinguer clairement ce qui est disponible, estimatif ou non implémenté ;
- conserver des contrats frontend/backend stricts et simples.

**Architecture concernée** :
- core low-level : réutilisation de la détection read-only existante ;
- desktop app : suppression des données codées en dur et des écrans trompeurs ;
- shared contracts : capacités runtime, diagnostic honnête, session active ;
- preview/export : validation seule tant que l'export réel n'existe pas.

**Contrats et interfaces** :
- `get_devices` doit retourner les supports réellement détectés par le core ;
- `get_runtime_capabilities` expose les capacités effectivement disponibles dans ce build ;
- `get_diagnostic` retourne une évaluation conservative basée sur les métadonnées du support, sans prétendre analyser des données supprimées ;
- `start_scan` et les flux associés doivent échouer explicitement tant que le moteur n'existe pas ;
- l'UI ne doit plus dépendre de `dev-1`, `scan-001` ni d'autres identifiants codés en dur.

**UX / UI** :
- chaque écran doit afficher un état honnête : vide, disponible, indisponible, erreur ;
- novice : messages simples, rassurants et explicites sur les limites ;
- expert : détails techniques supplémentaires sur le support et les limites du build ;
- aucun bouton ne doit laisser croire qu'une récupération réelle a déjà eu lieu si ce n'est pas le cas.

**Étapes d'implémentation** :
1. documenter les capacités réellement disponibles et retirer les identifiants/valeurs codées en dur ;
2. brancher `get_devices` sur la détection système read-only ;
3. introduire des capacités runtime et un diagnostic dérivé des métadonnées réelles du support ;
4. convertir les pages scan/résultats/export en états honnêtes sans résultats simulés ;
5. mettre à jour les textes i18n et les messages de sécurité ;
6. vérifier build frontend et backend.

**Tests et validation** :
- build TypeScript sans erreur ;
- compilation Rust sans erreur ;
- affichage réel des supports détectés ;
- aucun écran ne présente de fichiers récupérés ou d'analyse en cours sans moteur réel ;
- navigation stable même en absence de support sélectionné.

**Risques** :
- frustration utilisateur si trop de fonctionnalités deviennent indisponibles, mais ce risque est préférable à une UX mensongère ;
- régression de navigation si des pages supposent encore un scan actif ;
- faux sentiment de support “IA” si les textes marketing ne sont pas alignés avec les capacités réelles.

**Questions ouvertes** :
- faut-il masquer complètement les routes non disponibles ou conserver des écrans informatifs ?
- jusqu'où pousser la validation réelle de destination avant l'implémentation de l'export sécurisé complet ?

## Chantier 8 — Scan read-only MVP des volumes montés
**Objectif** : Ajouter un vrai scan desktop en lecture seule qui catalogue les fichiers actuellement lisibles sur un volume monté, avec progression réelle, historique et résultats consultables, sans prétendre récupérer des fichiers supprimés.

**Pourquoi** :
- l'application a besoin d'un premier flux réellement opérationnel ;
- un scan de catalogue read-only apporte une base concrète pour la suite sans mentir sur la récupération supprimée ;
- cela permet de valider les contrats de session, progression et résultats avant les analyzers recovery.

**Périmètre** :
- couvert : démarrage de session, progression, résultats, historique, scan `quick` vs `deep`, UI associée ;
- non couvert : récupération de fichiers supprimés, carving, preview binaire, export effectif, pause/reprise, scan brut du device non monté ;
- dépendances : `src-tauri/src/commands/mod.rs`, `src-tauri/src/core/mod.rs`, types Rust/TS, pages `Diagnostic`, `Scan`, `Results`, `History`, store ;
- hypothèses : les volumes visibles par l'OS via `sysinfo` sont montés et peuvent être parcourus en lecture seule.

**Contraintes** :
- lecture seule stricte sur la source ;
- aucune promesse de récupération de données effacées ;
- progression dérivée d'un travail réel ;
- gestion robuste des erreurs d'accès et permissions.

**Architecture concernée** :
- core : exposition du point de montage ;
- desktop app : orchestration de session de scan et affichage des résultats ;
- shared contracts : ajout des métadonnées de montage si nécessaire, session active, résultats réels ;
- historique : persistance mémoire de sessions pendant l'exécution de l'app.

## Chantier 78 — Durcissement audit lot 1
**Objectif** : Appliquer un premier lot de durcissement ciblé issu de l'audit sans refactor spéculatif, en réduisant la surface d'exposition des assets, en renforçant le preflight release et la CI, et en rendant le frontend plus robuste face aux états IPC et de navigation incomplets.

**Pourquoi** :
- l'audit a confirmé que la promesse read-only est bien tenue, donc le meilleur retour immédiat vient de quick wins de durcissement ;
- ce lot réduit des surfaces de risque concrètes sans perturber les flux cœur de scan/export ;
- la traçabilité impose de documenter clairement ce qui est traité maintenant et ce qui reste volontairement hors lot.

**Périmètre** :
- couvert : `src-tauri/tauri.conf.json`, `scripts/release-preflight.mjs`, `.github/workflows/ci.yml`, `src/router.tsx`, `src/hooks/useBackgroundScanPoller.ts`, nouveaux types/guards frontend si nécessaires ;
- non couvert : migration globale des `Mutex`, refonte de `commands/mod.rs`, fuzzing analyzers, tests du privileged imager, déplacement de `PLANS.md` ;
- dépendances : moteur preview Tauri, workflow release actuel, store Zustand, contrats TypeScript de scan ;
- hypothèses : les previews matérialisés vivent uniquement dans le workspace temporaire `recupere-workspace/previews` et le lot doit rester compatible avec les previews locales et distantes existantes.

**Contraintes** :
- ne pas casser la lecture seule ni l'export sécurisé existant ;
- signaler explicitement tout élargissement de scope risqué ;
- conserver la compatibilité desktop multiplateforme ;
- éviter les validations runtime lourdes côté frontend tant qu'un mapping strict suffit.

**Architecture concernée** :
- preview/export : restriction du scope des assets exposés à la webview ;
- desktop app : guards de navigation et mapping IPC plus strict ;
- shared contracts : consolidation légère du contrat `ScanProgress` côté TypeScript ;
- gouvernance / supply chain : durcissement du preflight et de la CI.

**Contrats et interfaces** :
- les aperçus média et image doivent continuer à être servis depuis le répertoire temp dédié aux previews ;
- si `plugins.updater.active === true`, le preflight doit échouer quand `pubkey` est vide ;
- la CI doit auditer les dépendances Node au niveau `high` et limiter explicitement les permissions GitHub ;
- le poller de scan ne doit plus dépendre de `as any` pour hydrater `ScanProgress` ;
- `/scan`, `/results` et `/export` doivent rediriger proprement quand le contexte minimum n'est pas présent.

**UX / UI** :
- un utilisateur sans device sélectionné ne doit pas tomber sur un écran incohérent de scan ;
- un utilisateur sans scan actif ne doit pas atteindre résultats/export et voir des erreurs implicites ;
- les redirections doivent rester calmes, déterministes et compréhensibles.

**Étapes d'implémentation** :
1. documenter le lot et borner explicitement son périmètre ;
2. restreindre le scope `assetProtocol` au répertoire réel des previews temporaires ;
3. renforcer `release-preflight` autour de la configuration updater ;
4. ajouter l'audit npm et les permissions minimales à la CI ;
5. typer strictement le mapping du poller de progression ;
6. ajouter des guards de navigation pour les routes dépendantes d'un device ou d'un scan actif ;
7. lancer les validations ciblées disponibles.

**Tests et validation** :
- `npm run test:ui` ;
- `npm run build` ;
- `npm run release:preflight` ;
- revue manuelle du diff CI et de la config Tauri ;
- vérification fonctionnelle des redirections de routes et de la compilation TypeScript.

**Risques** :
- un scope asset trop étroit peut casser certains aperçus média ;
- des guards trop agressifs peuvent masquer des flows distants valides ;
- `npm audit` peut introduire du bruit CI si l'écosystème remonte des alertes non actionnables.

**Questions ouvertes** :
- faut-il prévoir un second emplacement de previews si l'application persiste plus tard des assets hors dossier temp ?
- faut-il déplacer ensuite la validation IPC vers un garde runtime plus complet (`zod` ou équivalent) ou rester sur un mapper strict léger ?

**Contrats et interfaces** :
- `start_scan` démarre une session asynchrone réelle sur un volume monté ;
- `get_scan_progress` retourne une progression réelle et non simulée ;
- `get_results` retourne les fichiers catalogués pour cette session ;
- `quick` = profondeur limitée ; `deep` = parcours récursif complet ;
- les résultats doivent être décrits comme des fichiers catalogués lisibles, pas comme des fichiers restaurés.

**UX / UI** :
- la page `Scan` doit permettre de lancer explicitement un quick/deep scan ;
- la page `Results` doit afficher les résultats réels de la session active ;
- l'historique doit refléter les scans réellement terminés ;
- un message visible doit rappeler que les fichiers supprimés ne sont pas encore analysés.

**Étapes d'implémentation** :
1. définir les contrats de session et les capacités exposées ;
2. implémenter le scan asynchrone read-only sur volume monté ;
3. connecter `Scan` et `Results` au moteur ;
4. alimenter l'historique et l'état global ;
5. ajuster les textes UI pour distinguer catalogue lisible et récupération supprimée ;
6. vérifier build frontend/backend.

**Tests et validation** :
- démarrage d'un quick scan réel ;
- progression observable jusqu'à `completed` ;
- résultats non vides sur un volume monté contenant des fichiers ;
- aucune donnée simulée codée en dur ;
- builds `npm run build` et `cargo check` verts.

**Risques** :
- confusion entre catalogue de fichiers lisibles et récupération réelle si le wording n'est pas strict ;
- scans profonds potentiellement lents sur gros volumes ;
- erreurs de permission selon le contenu du volume.

**Questions ouvertes** :
- faut-il stocker les résultats complets seulement en mémoire ou préparer dès maintenant une persistance sur disque de travail ?
- quel niveau de détail afficher côté novice sur un scan qui n'analyse pas encore les suppressions ?

## Chantier 9 — Export sécurisé MVP des fichiers catalogués
**Objectif** : Permettre l’export sécurisé des fichiers catalogués par le scan read-only MVP vers une destination située sur un autre support, avec progression, gestion des conflits et vérification simple d’intégrité.

**Pourquoi** :
- un scan utile doit pouvoir déboucher sur une sortie concrète ;
- l’export sécurisé matérialise la séparation stricte entre source et destination ;
- ce chantier valide les contrats d’export avant les futures restaurations avancées.

**Périmètre** :
- couvert : sélection de fichiers, validation de destination, démarrage d’export, progression, erreurs d’export, stratégies `rename/skip/overwrite`, conservation optionnelle de l’arborescence ;
- non couvert : restauration de fichiers supprimés, reprise après interruption, export différentiel, vérification cryptographique forte ;
- dépendances : sessions de scan en mémoire, page `Export`, types partagés Rust/TS, validation anti-source existante ;
- hypothèses : les fichiers catalogués restent lisibles au moment de l’export et la destination est accessible en écriture.

**Contraintes** :
- ne jamais écrire sur le support source ;
- refuser toute destination résolue sur le même support physique que la source ;
- journaliser les décisions critiques et les erreurs ;
- expliciter que l’export concerne des fichiers lisibles catalogués, pas une récupération supprimée.

**Architecture concernée** :
- preview/export : moteur d’export sécurisé MVP ;
- desktop app : orchestration UI, progression, états de succès/erreur ;
- shared contracts : sessions d’export, progression, erreurs, stratégie de conflit ;
- core : réutilisation de la validation de destination.

**Contrats et interfaces** :
- `start_export(scan_id, destination_path, selected_file_ids, conflict_strategy, preserve_structure, verify_integrity)` démarre une session asynchrone ;
- `get_export_progress(export_id)` retourne l’état courant de la session ;
- la validation de destination doit être rejouée côté backend au démarrage réel ;
- les erreurs par fichier doivent rester traçables et exposées à l’UI.

**UX / UI** :
- la page `Export` doit afficher un état prêt, en cours, terminé, erreur partielle ;
- novice : messages rassurants sur la séparation source/destination ;
- expert : visibilité sur les conflits et vérifications d’intégrité ;
- si l’export n’est pas possible, le bouton doit l’expliquer au lieu d’échouer silencieusement.

**Étapes d’implémentation** :
1. définir les types de progression d’export et les capacités runtime ;
2. implémenter le moteur Rust asynchrone d’export sécurisé ;
3. brancher la page `Export` sur un vrai démarrage + polling ;
4. mettre à jour les libellés pour distinguer export de fichiers catalogués et récupération future ;
5. vérifier builds frontend/backend.

**Tests et validation** :
- export réel d’un ou plusieurs fichiers catalogués vers un autre support ;
- refus d’une destination située sur la source ;
- progression observable jusqu’à `completed` ou `error` ;
- builds `npm run build` et `cargo check` verts.

**Risques** :
- confusion utilisateur entre copie de fichiers lisibles et restauration de fichiers supprimés ;
- conflits de noms si l’export sans arborescence concentre plusieurs fichiers ;
- erreur tardive si un fichier devient illisible entre le scan et l’export.

**Questions ouvertes** :
- faut-il introduire une vérification de contenu plus forte qu’une comparaison de taille dès ce MVP ?
- veut-on persister l’historique d’export dans un chantier séparé ?

## Chantier 10 — Historique réel et journaux techniques de session
**Objectif** : Brancher les écrans `Accueil`, `Historique & Journaux` et `Mode expert` sur un historique et des journaux techniques réellement produits par le backend, au lieu de synthétiser ces événements uniquement côté frontend.

**Pourquoi** :
- plusieurs pages restent honnêtes mais encore trop “minces” sur la traçabilité réelle ;
- un historique backend unifie la source de vérité pour les scans terminés ;
- des journaux techniques consultables rendent le build plus crédible pour un usage expert sans prétendre offrir des fonctionnalités recovery absentes.

**Périmètre** :
- couvert : historique mémoire des scans, logs techniques de scan, récupération IPC de l’historique et des logs, affichage réel dans `Home`, `History` et `Expert`, retrait des logs synthétiques frontend ;
- non couvert : persistance disque des historiques, logs d’export détaillés, visionneuse hexadécimale, audit trail cryptographiquement signé ;
- dépendances : `src-tauri/src/commands/mod.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/stores/appStore.ts`, pages `Home`, `History`, `Scan`, `Expert`, types partagés ;
- hypothèses : un historique en mémoire pendant l’exécution de l’app suffit pour ce jalon MVP.

**Contraintes** :
- conserver une lecture seule stricte sur la source ;
- journaliser les étapes critiques et les erreurs sans fabriquer de faux événements ;
- ne pas présenter l’historique comme persistant entre redémarrages si ce n’est pas implémenté ;
- garder des contrats simples et cohérents entre Rust et TypeScript.

**Architecture concernée** :
- commands : registre de sessions, journal de session, résumé d’historique ;
- shared contracts : nouveaux types IPC pour logs et historique ;
- desktop app : synchronisation du store avec les commandes backend et rendu des pages concernées.

**Contrats et interfaces** :
- `get_scan_history()` retourne les sessions connues avec métadonnées de démarrage/fin, statut, durée et compteurs ;
- `get_scan_logs(scan_id)` retourne les événements techniques réellement enregistrés pour la session ;
- les logs frontend affichés doivent provenir du backend et non d’une reconstruction UI ;
- la capacité runtime `technical_logs` passe à `true` seulement si cette lecture est réellement disponible.

**UX / UI** :
- `Accueil` affiche les derniers scans issus de l’historique backend ;
- `Historique & Journaux` reflète les scans réellement connus par le moteur ;
- `Mode expert` montre les événements techniques de la session active quand ils existent ;
- en l’absence de logs, l’interface doit expliquer qu’aucune session n’a encore produit d’événements.

**Étapes d’implémentation** :
1. définir les types Rust/TS pour l’historique et les logs ;
2. enrichir les sessions de scan avec métadonnées de temps et journal technique ;
3. exposer les commandes IPC de lecture ;
4. remplacer les logs synthétiques frontend par des logs backend ;
5. connecter `AppShell` / `History` / `Expert` à ces données ;
6. vérifier les builds frontend/backend.

**Tests et validation** :
- un scan réel crée une entrée dans l’historique backend ;
- les logs d’une session active ou terminée sont consultables via IPC ;
- `Home`, `History` et `Expert` affichent ces données sans mocks ;
- `npm run build` et `cargo check` passent.

**Risques** :
- confusion si l’utilisateur croit que l’historique est persistant après redémarrage alors qu’il reste mémoire ;
- duplication temporaire des événements si le frontend mélange anciens logs synthétiques et nouveaux logs backend ;
- bruit excessif si trop d’événements sont journalisés pendant un scan profond.

**Questions ouvertes** :
- faut-il unifier plus tard scans et exports dans un historique global unique ?
- veut-on introduire une persistance locale chiffrée des journaux dans un chantier dédié ?

## Chantier 11 — Couverture de tests pour la logique critique
**Objectif** : Ajouter une première couverture de tests automatisés sur les zones sensibles déjà implémentées, en priorité la sécurité d’export, les helpers de scan/historique et les resets d’état frontend.

**Pourquoi** :
- le projet manipule des workflows sensibles où une régression de logique peut devenir coûteuse ;
- plusieurs helpers critiques existent désormais mais ne sont pas encore verrouillés par des tests ;
- une base de tests permet de continuer l’industrialisation sans retomber dans des comportements implicites ou des faux flux.

**Périmètre** :
- couvert : tests unitaires Rust sur les helpers critiques scan/export, tests unitaires TypeScript sur le store applicatif, scripts de test documentés dans `package.json` ;
- non couvert : tests E2E Tauri, tests UI visuels complets, tests de périphériques réels, benchmarks ;
- dépendances : `src-tauri/src/commands/mod.rs`, `src/stores/appStore.ts`, configuration npm, dépendances de test frontend ;
- hypothèses : une première base unitaire apporte déjà une bonne protection sur les contrats les plus fragiles.

**Contraintes** :
- ne jamais écrire sur le support source dans les tests ;
- isoler les tests fichier backend dans des répertoires temporaires dédiés ;
- éviter les assertions couplées à l’UI visuelle ;
- garder des tests lisibles et ciblés sur des invariants métier.

**Architecture concernée** :
- backend Rust : helpers de chemin, conflits d’export, progression, résumés d’historique ;
- desktop app : store Zustand et resets d’état ;
- outillage : scripts `test`, `test:ui`, `test:rust`.

**Contrats et interfaces** :
- les stratégies `rename/skip/overwrite` doivent rester déterministes ;
- la construction des chemins d’export ne doit pas perdre l’arborescence relative ;
- l’historique de scan doit exposer des métadonnées cohérentes ;
- `selectDevice` et `setRecoveryResult` doivent remettre l’état sensible dans un état sûr.

**Étapes d’implémentation** :
1. ajouter les scripts et dépendances de test frontend ;
2. écrire des tests Rust sur les helpers critiques scan/export ;
3. écrire des tests Vitest sur le store applicatif ;
4. exécuter `npm run test`, `npm run build`, `cargo test`, `cargo check`.

**Tests et validation** :
- les stratégies de conflit d’export sont couvertes ;
- la validation et la composition de chemins critiques sont couvertes ;
- les resets de store frontend sont couverts ;
- la suite est exécutable localement via des scripts simples.

**Risques** :
- tests trop couplés à l’implémentation interne ;
- faux sentiment de sécurité si seuls les helpers sont couverts ;
- complexité accrue du repo si l’infra de tests frontend est surdimensionnée trop tôt.

**Questions ouvertes** :
- faut-il ensuite monter vers des tests d’intégration Tauri pilotant le vrai bridge IPC ?
- veut-on couvrir les pages critiques avec un harness React dédié dans un chantier séparé ?

## Chantier 12 — Tests d’intégration backend scan vers export
**Objectif** : Compléter la couverture actuelle avec des tests backend plus intégrés qui exécutent réellement le scan de catalogue et l’export sur des répertoires temporaires contrôlés.

**Pourquoi** :
- les tests unitaires actuels verrouillent bien les helpers mais pas encore le comportement global des moteurs MVP ;
- le flux `scan -> résultats -> export` est désormais central dans l’application ;
- des tests temporaires sur le filesystem local réduisent le risque de régressions silencieuses dans le pipeline read-only/export.

**Périmètre** :
- couvert : exécution réelle de `run_inventory_scan` sur un répertoire temporaire, exécution réelle de `run_export_session`, validation des résultats/progress/logs sur session ;
- non couvert : détection hardware réelle, bridge Tauri complet, périphériques physiques, volumes système, tests UI de bout en bout ;
- dépendances : `src-tauri/src/commands/mod.rs`, sessions scan/export en mémoire, helpers filesystem temporaires ;
- hypothèses : des répertoires temporaires locaux suffisent pour simuler fidèlement le comportement du moteur MVP sur volume monté.

**Contraintes** :
- rester strictement hors des disques source réels ;
- créer puis nettoyer les fixtures filesystem dans des répertoires temporaires dédiés ;
- ne pas rendre les tests flaky via des attentes temporelles ou du multithreading inutile ;
- vérifier les invariants métier, pas seulement l’absence de panic.

**Architecture concernée** :
- backend Rust : moteur de scan read-only MVP, moteur d’export sécurisé MVP, contrats de progression/logs.

**Contrats et interfaces** :
- un scan profond sur un répertoire temporaire doit cataloguer les fichiers accessibles, marquer la session `completed` et produire des logs ;
- un export sur un répertoire temporaire distinct doit copier les fichiers attendus, mettre à jour la progression et finir en `completed` ;
- la structure relative doit être préservée quand `preserve_structure=true`.

**Étapes d’implémentation** :
1. ajouter les fixtures temporaires backend nécessaires ;
2. écrire un test d’intégration scan sur arborescence temporaire ;
3. écrire un test d’intégration export sur résultats temporaires ;
4. exécuter et vérifier les suites.

**Tests et validation** :
- le scan catalogue réellement des fichiers depuis un répertoire temporaire ;
- l’export copie réellement les fichiers vers la destination temporaire ;
- les progressions et statuts finaux sont cohérents ;
- les builds et suites existantes restent vertes.

**Risques** :
- tests trop lents si l’arborescence temporaire est surdimensionnée ;
- dépendance implicite au filesystem local de l’OS ;
- confusion si les tests deviennent trop proches des helpers internes plutôt que du comportement observable.

**Questions ouvertes** :
- faut-il ensuite couvrir les erreurs de permission avec des tests spécifiques selon OS ?
- veut-on introduire une couche de fixtures dédiée pour les futures suites d’intégration analyzers/carving ?

## Chantier 13 — Persistance locale de l’historique et des journaux de scan
**Objectif** : Conserver localement l’historique des scans et leurs journaux techniques entre les relances de l’application, puis l’exposer proprement dans l’écran `Historique & Journaux`.

**Pourquoi** :
- l’historique actuel est réel mais encore perdu à la fermeture de l’application ;
- des journaux persistants améliorent la traçabilité demandée par le domaine ;
- l’écran `Historique & Journaux` devient réellement exploitable pour revoir une session passée.

**Périmètre** :
- couvert : archivage local JSON des sessions de scan et de leurs logs, lecture au démarrage, fusion mémoire/disque, consultation des logs d’une session historique dans l’UI ;
- non couvert : persistance des exports, chiffrement local des archives, rotation avancée, synchronisation cloud ;
- dépendances : `src-tauri/src/commands/mod.rs`, `src/pages/HistoryPage.tsx`, i18n, tests Rust ;
- hypothèses : une persistance locale en JSON dans un répertoire de données applicatif suffit pour ce jalon.

**Contraintes** :
- ne jamais écrire sur le support source ;
- écrire uniquement dans un emplacement applicatif local dédié ;
- tolérer un fichier d’historique absent ou corrompu sans bloquer l’app ;
- conserver un wording honnête sur le fait que seuls les scans sont persistés à ce stade.

**Architecture concernée** :
- backend Rust : couche légère d’archive locale des scans ;
- desktop app : chargement et affichage de l’historique persistant ;
- shared contracts : réutilisation des contrats existants, sans promesse supplémentaire sur la récupération.

**Contrats et interfaces** :
- `get_scan_history()` doit fusionner sessions live et sessions persistées ;
- `get_scan_logs(scan_id)` doit pouvoir retourner les logs d’une session passée même si elle n’est plus en mémoire ;
- une session en mémoire doit primer sur sa version persistée si les deux existent.

**UX / UI** :
- l’écran `Historique & Journaux` doit afficher une note claire indiquant que l’historique est stocké localement ;
- l’utilisateur doit pouvoir sélectionner une session passée et consulter ses logs ;
- les états vides et erreurs doivent rester explicites et non anxiogènes.

**Étapes d’implémentation** :
1. ajouter la couche de stockage local JSON côté backend ;
2. persister les snapshots de session aux étapes importantes ;
3. fusionner lecture live + persistée ;
4. afficher les logs d’une session historique dans `HistoryPage` ;
5. ajouter des tests backend sur la persistance ;
6. vérifier les suites.

**Tests et validation** :
- une session persistée est relue correctement depuis le disque local ;
- les logs d’une session persistée restent consultables ;
- `npm run test`, `npm run build`, `cargo test`, `cargo check` passent.

**Risques** :
- corruption du fichier JSON après interruption en écriture ;
- confusion utilisateur si l’on ne précise pas que seuls les scans sont persistés ;
- divergence entre archive persistée et état live sans logique de fusion explicite.

**Questions ouvertes** :
- faut-il persister ensuite aussi l’historique d’export dans la même archive ou dans un fichier séparé ?
- veut-on une action explicite de purge d’historique dans un chantier dédié ?

## Chantier 14 — Historique local des exports et consultation UI
**Objectif** : Étendre la traçabilité locale aux exports réels afin que l’écran `Historique & Journaux` couvre désormais les scans persistés et les exports persistés.

**Pourquoi** :
- l’export est maintenant un vrai flux métier et doit être traçable lui aussi ;
- la persistance actuelle ne couvre que les scans, ce qui laisse le suivi incomplet ;
- un historique d’export local aide à comprendre ce qui a été copié, vers où, et avec quelles erreurs.

**Périmètre** :
- couvert : archive locale JSON des exports, fusion live/disque côté backend, lecture IPC de l’historique d’export, affichage d’une table d’exports et d’un détail d’erreurs dans `HistoryPage`, tests backend de persistance ;
- non couvert : logs d’export détaillés pas-à-pas, purge utilisateur, historique global unifié scans+exports, chiffrement local ;
- dépendances : `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/pages/HistoryPage.tsx`, i18n ;
- hypothèses : un résumé persistant avec erreurs par fichier suffit pour ce jalon sans introduire un journal d’export complet.

**Contraintes** :
- conserver une séparation stricte source/destination ;
- ne jamais écrire hors du répertoire applicatif local pour l’archive ;
- tolérer l’absence ou la corruption de l’archive d’export ;
- rester honnête sur le fait qu’il s’agit d’un historique local de copies de fichiers catalogués.

**Architecture concernée** :
- backend Rust : archive persistante des exports ;
- shared contracts : nouveaux types de résumé d’export ;
- desktop app : affichage des exports passés et de leurs erreurs dans la page historique.

**Contrats et interfaces** :
- `get_export_history()` retourne les exports connus, live et persistés ;
- une session export live doit primer sur sa version persistée ;
- les erreurs par fichier d’un export terminé doivent rester consultables dans l’UI.

**UX / UI** :
- `Historique & Journaux` doit distinguer scans et exports sans ambiguïté ;
- chaque export passé doit afficher la destination, le volume copié et le statut ;
- quand un export contient des erreurs, leurs détails doivent être visibles sans jargon inutile.

**Étapes d’implémentation** :
1. ajouter les types Rust/TS d’historique d’export ;
2. persister les snapshots d’export ;
3. exposer `get_export_history` côté IPC ;
4. enrichir `HistoryPage` avec une section exports ;
5. ajouter des tests backend de persistance ;
6. vérifier les suites.

**Tests et validation** :
- les exports terminés sont persistés et relus ;
- les erreurs d’export persistent avec leur détail ;
- `npm run test`, `npm run build`, `cargo test`, `cargo check` passent.

**Risques** :
- duplication d’information si l’écran historique devient trop chargé ;
- écritures trop fréquentes de l’archive si chaque mise à jour de progression persiste ;
- confusion si l’utilisateur assimile l’historique d’export à une restauration supprimée.

**Questions ouvertes** :
- faut-il ensuite factoriser scan/export dans un historique technique unique ?
- veut-on ajouter une action “ouvrir le dossier de destination” à partir de l’historique d’export ?

## Chantier 15 — Purge locale contrôlée de l’historique scan/export
**Objectif** : Permettre à l’utilisateur de purger explicitement les archives locales d’historique sans toucher aux données source, aux résultats catalogués en mémoire ou aux sessions actives.

**Pourquoi** :
- la persistance locale scan/export est maintenant utile, mais elle doit rester sous contrôle utilisateur ;
- certaines investigations sensibles exigent une suppression explicite de la trace locale après revue ;
- l’écran `Historique & Journaux` mentionnait déjà cette limite comme chantier restant.

**Périmètre** :
- couvert : commande backend de purge locale, suppression atomique des fichiers d’archive scan/export, retour structuré avec compteurs supprimés, exposition IPC, action UI avec confirmation, message de succès/limite, tests backend ;
- non couvert : purge des sessions live en mémoire, purge sélective par session, chiffrement des archives, journal détaillé des exports ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/pages/HistoryPage.tsx`, i18n ;
- hypothèses : purger seulement les archives persistées est le comportement le plus sûr pour ne pas casser un scan ou un export encore consulté dans l’exécution courante.

**Contraintes** :
- ne jamais toucher au disque source ni aux fichiers exportés ;
- ne supprimer que les archives applicatives locales ;
- signaler clairement que des sessions encore chargées en mémoire peuvent rester visibles jusqu’au redémarrage ;
- tracer l’action côté backend.

**Architecture concernée** :
- backend Rust : normalisation du scope, suppression des archives et résumé de purge ;
- shared contracts : type de résultat de purge ;
- desktop app : action utilisateur explicite dans `Historique & Journaux`.

**Contrats et interfaces** :
- `clear_local_history(scope)` accepte `scan`, `export` ou `all` ;
- le résultat doit indiquer combien d’enregistrements scan/export ont été retirés des archives locales ;
- le résultat doit indiquer combien de sessions live restent en mémoire au moment de la purge.

**UX / UI** :
- l’action doit être explicite, destructive visuellement et confirmée avant exécution ;
- le message de retour doit expliquer que seules les archives locales sont concernées ;
- si des sessions live restent présentes, l’écran doit l’indiquer sans ambiguïté.

**Étapes d’implémentation** :
1. ajouter le type Rust/TS de résultat de purge ;
2. implémenter la purge backend des archives locales scan/export ;
3. exposer `clear_local_history` côté IPC ;
4. ajouter le bouton de purge et les messages associés dans `HistoryPage` ;
5. ajouter des tests backend ciblés ;
6. relancer `npm run test`, `npm run build`, `cargo check`.

**Tests et validation** :
- une purge `scan` ne supprime pas l’archive export ;
- une purge `all` supprime les deux archives et retourne les compteurs attendus ;
- `npm run test`, `npm run build`, `cargo check` passent.

**Risques** :
- confusion si l’utilisateur croit purger aussi les sessions live de l’exécution courante ;
- suppression accidentelle si l’action n’est pas suffisamment explicitée ;
- divergence de comportement entre fichier absent et archive vide.

**Questions ouvertes** :
- faut-il ensuite proposer une purge séparée scan/export directement dans l’UI ?
- faut-il ajouter une conservation configurable de l’historique local ?

## Chantier 16 — Journaux techniques détaillés des exports
**Objectif** : Ajouter des journaux techniques d’export, persistés localement et consultables aussi bien pendant l’export qu’après coup dans `Historique & Journaux`.

**Pourquoi** :
- l’export est maintenant un vrai flux métier et chaque décision importante doit rester traçable ;
- l’historique d’export ne conserve aujourd’hui qu’un résumé et des erreurs, sans pas-à-pas technique ;
- en cas d’échec partiel, il faut pouvoir comprendre précisément ce qui a été préparé, copié, vérifié ou ignoré.

**Périmètre** :
- couvert : logs live des exports, persistance locale des logs d’export avec rétrocompatibilité des archives existantes, nouvelle commande IPC `get_export_logs`, consultation UI dans `ExportPage` et `HistoryPage`, tests backend ;
- non couvert : refonte du mode expert, ouverture du dossier de destination, export des logs vers un fichier externe ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/ExportPage.tsx`, `src/pages/HistoryPage.tsx`, i18n ;
- hypothèses : le format de log technique peut réutiliser la structure `TechnicalLogEntry` déjà utilisée pour les scans.

**Contraintes** :
- conserver la séparation stricte source/destination ;
- ne jamais écrire ailleurs que dans les archives locales applicatives ;
- limiter le volume des logs par session pour éviter une croissance non maîtrisée ;
- tolérer un ancien format d’archive d’export sans logs détaillés.

**Architecture concernée** :
- backend Rust : journal technique live/persisté des exports ;
- shared contracts : réutilisation du type de log technique existant ;
- desktop app : affichage des logs d’export actifs et passés.

**Contrats et interfaces** :
- `get_export_logs(export_id)` retourne les logs live si la session est encore en mémoire, sinon les logs persistés ;
- les archives d’export existantes sans logs doivent rester lisibles ;
- les erreurs d’export doivent apparaître à la fois dans le résumé et dans le journal technique.

**UX / UI** :
- `ExportPage` doit afficher un panneau de logs techniques pendant et après l’export ;
- `HistoryPage` doit permettre d’inspecter les logs d’un export passé ;
- si aucun log n’est encore disponible, l’UI doit l’expliquer clairement sans promettre une récupération inexistante.

**Étapes d’implémentation** :
1. ajouter les structures persistées d’export avec logs et la compatibilité legacy ;
2. journaliser les étapes importantes du moteur d’export ;
3. exposer `get_export_logs` côté IPC ;
4. afficher les logs dans `ExportPage` et `HistoryPage` ;
5. ajouter des tests backend ciblés ;
6. relancer `npm run test`, `npm run build`, `cargo check`.

**Tests et validation** :
- les logs d’export sont persistés et relus ;
- une archive legacy d’export reste lisible ;
- `run_export_session` produit des logs techniques significatifs ;
- `npm run test`, `npm run build`, `cargo check` passent.

**Risques** :
- bruit excessif si chaque fichier génère trop d’entrées ;
- réécritures trop fréquentes de l’archive locale ;
- confusion entre erreurs bloquantes et avertissements si les niveaux de log sont mal utilisés.

**Questions ouvertes** :
- faut-il ensuite regrouper scan/export dans un visualiseur technique unique ?
- faut-il offrir une exportation externe du journal d’export pour support avancé ?

## Chantier 17 — Visualiseur technique unifié dans le mode expert
**Objectif** : Faire du `Mode expert` un point d’observation fiable des sessions actives en exposant à la fois les journaux de scan et les journaux d’export sans dépendre d’un autre écran ouvert en parallèle.

**Pourquoi** :
- les logs d’export existent maintenant réellement mais ne remontent pas encore dans `Mode expert` ;
- aujourd’hui, l’écran expert dépend implicitement du fait que `ScanPage` ait déjà alimenté le store pour les journaux de scan ;
- un utilisateur expert doit pouvoir suivre une opération active depuis une seule vue technique.

**Périmètre** :
- couvert : état global des logs d’export dans le store, polling autonome des logs actifs dans `ExpertPage`, affichage séparé scan/export avec statuts et erreurs de chargement, ajustements de `ExportPage`, tests Vitest sur les resets et snapshots ;
- non couvert : fusion chronologique scan/export dans une seule timeline, export externe des journaux, refonte du layout expert complet ;
- dépendances : `PLANS.md`, `src/stores/appStore.ts`, `src/stores/appStore.test.ts`, `src/pages/ExpertPage.tsx`, `src/pages/ExportPage.tsx`, `src/hooks/useIpc.ts`, i18n ;
- hypothèses : conserver deux sous-panneaux explicites, scan et export, est plus lisible qu’une fusion d’événements à ce stade.

**Contraintes** :
- le mode expert doit rester honnête sur la source des événements ;
- ne pas masquer un échec de rafraîchissement de logs derrière un simple état vide ;
- éviter de conserver des logs d’export obsolètes lors d’un changement de support ou d’un nouveau jeu de résultats.

**Architecture concernée** :
- desktop app : store global et visualisation technique active ;
- IPC frontend : réutilisation des endpoints de logs existants ;
- UX expert : vue unifiée mais clairement segmentée.

**Contrats et interfaces** :
- le store doit porter `exportLogs` avec actions de remplacement et de purge ;
- `ExpertPage` doit pouvoir rafraîchir les logs de scan et d’export à partir de `activeScanId` et `activeExportId` ;
- un changement de support ou de résultats doit purger l’état d’export actif et ses logs.

**UX / UI** :
- `Mode expert` doit montrer explicitement quelle session de scan et/ou d’export est active ;
- les journaux de scan et d’export doivent être distingués visuellement sans jargon ambigu ;
- en l’absence de session active, le message doit expliquer qu’aucun flux technique actif n’est disponible.

**Étapes d’implémentation** :
1. étendre le store avec les logs d’export et leurs resets ;
2. brancher `ExportPage` sur ce nouvel état ;
3. rendre `ExpertPage` autonome pour le rafraîchissement des logs actifs ;
4. compléter les libellés i18n ;
5. ajouter/mettre à jour les tests Vitest ;
6. relancer `npm run test`, `npm run build`, `cargo check`.

**Tests et validation** :
- `selectDevice` purge bien l’état export et les logs d’export ;
- `setExportLogs` remplace proprement le snapshot courant ;
- `npm run test`, `npm run build`, `cargo check` passent.

**Risques** :
- duplication de polling entre pages si plusieurs vues sont ouvertes dans une future architecture multipanel ;
- confusion si les panneaux scan/export ne sont pas suffisamment distingués ;
- conservation involontaire de logs d’export après changement de contexte si les resets sont incomplets.

**Questions ouvertes** :
- faut-il ensuite regrouper les événements actifs dans une timeline unique triée par timestamp ?
- faut-il afficher aussi la progression résumée de l’export dans `Mode expert` ?

## Chantier 18 — Timeline technique active unifiée dans le mode expert
**Objectif** : Regrouper les événements techniques actifs de scan et d’export dans une timeline unique triée par horodatage, tout en conservant une indication explicite de leur source.

**Pourquoi** :
- le mode expert affiche désormais les deux flux actifs, mais encore dans des panneaux séparés ;
- pour suivre une opération réelle, il est plus utile de voir l’ordre intercalé des événements scan/export ;
- une timeline unique réduit l’effort mental quand plusieurs opérations techniques se chevauchent.

**Périmètre** :
- couvert : helper frontend pur de fusion/tri des logs, affichage d’une timeline unifiée dans `ExpertPage`, badge de source `scan/export`, tests Vitest ciblés sur l’ordre de fusion ;
- non couvert : fusion des historiques passés dans `HistoryPage`, enrichissement des logs backend avec corrélation inter-session, recherche/filtrage avancés ;
- dépendances : `PLANS.md`, `src/pages/ExpertPage.tsx`, `src/components/scan`, `src/utils`, `src/index.css`, i18n ;
- hypothèses : les sessions actives restent au plus une de scan et une d’export, ce qui permet une fusion simple annotée par source.

**Contraintes** :
- conserver une indication explicite de la source de chaque événement ;
- rester lisible en mode expert sans noyer les événements importants ;
- garder une logique de fusion déterministe et testée.

**Architecture concernée** :
- desktop app : composition frontend des flux techniques actifs ;
- UX expert : visualisation unifiée mais traçable.

**Contrats et interfaces** :
- la fusion doit être stable et triée par `timestampMs` ;
- chaque entrée unifiée doit porter `source` et `sessionId` ;
- en absence de logs, l’UI doit afficher un état vide honnête.

**UX / UI** :
- la timeline unifiée doit apparaître avant les éventuels détails spécifiques ;
- les événements `scan` et `export` doivent être reconnaissables d’un coup d’œil ;
- l’écran doit rester compatible desktop et éviter un empilement confus de panneaux.

**Étapes d’implémentation** :
1. ajouter un helper pur de fusion de logs techniques ;
2. ajouter des tests Vitest sur l’ordre et les métadonnées fusionnées ;
3. créer un panneau UI dédié pour la timeline technique unifiée ;
4. brancher `ExpertPage` sur cette timeline ;
5. compléter les libellés et styles associés ;
6. relancer `npm run test`, `npm run build`, `cargo check`.

**Tests et validation** :
- la fusion respecte l’ordre par timestamp ;
- chaque entrée garde sa source et sa session ;
- `npm run test`, `npm run build`, `cargo check` passent.

**Risques** :
- ambiguïté de lecture si la source n’est pas suffisamment visible ;
- sensation de bruit si trop d’événements mineurs montent en tête de timeline ;
- tri instable si les collisions d’horodatage ne sont pas gérées de façon déterministe.

**Questions ouvertes** :
- faut-il ensuite ajouter des filtres `scan/export/error` dans cette timeline ?
- faut-il afficher la timeline unifiée aussi dans `Historique & Journaux` pour les sessions passées ?

## Chantier 19 — Filtres fins et export local du journal expert
**Objectif** : Rendre la timeline technique experte plus exploitable en ajoutant des filtres par niveau (`info`, `warning`, `error`, `debug`) et une exportation locale du journal filtré.

**Pourquoi** :
- le filtrage actuel par `error` seul reste trop grossier quand la timeline grossit ;
- un utilisateur expert peut vouloir isoler rapidement uniquement les warnings ou les traces debug ;
- exporter le journal filtré localement facilite le partage de contexte technique sans réexposer tout le flux brut.

**Périmètre** :
- couvert : extension du helper de filtrage à tous les niveaux, helper pur de formatage texte du journal, bouton d’export local depuis `ExpertPage`, libellés i18n, tests Vitest ;
- non couvert : sauvegarde via sélecteur natif Tauri, chiffrement du journal exporté, export backend des logs historiques ;
- dépendances : `PLANS.md`, `src/utils/technicalTimeline.ts`, `src/utils/technicalTimeline.test.ts`, `src/pages/ExpertPage.tsx`, i18n ;
- hypothèses : une exportation locale via téléchargement texte du WebView est suffisante pour ce jalon.

**Contraintes** :
- conserver les métadonnées de source et de session dans l’export ;
- ne pas permettre un export vide ambigu ;
- garder un filtrage déterministe et testable.

**Architecture concernée** :
- desktop app : logique frontend pure de filtrage et d’export de la timeline active ;
- UX expert : contrôle plus fin du bruit technique.

**Contrats et interfaces** :
- `filterTechnicalTimeline` doit accepter tous les niveaux de log connus ;
- `formatTechnicalTimelineExport` doit produire un texte lisible et stable ;
- l’export UI doit refléter uniquement les entrées actuellement filtrées.

**UX / UI** :
- les filtres de niveau doivent rester lisibles en desktop ;
- le bouton d’export doit être désactivé si aucune entrée n’est visible ;
- les libellés doivent expliquer qu’il s’agit du journal filtré courant.

**Étapes d’implémentation** :
1. étendre les types et helpers de timeline ;
2. ajouter des tests Vitest de filtrage fin et de formatage ;
3. brancher les nouveaux filtres et le bouton d’export dans `ExpertPage` ;
4. compléter i18n ;
5. relancer `npm run test`, `npm run build`, `cargo check`.

**Tests et validation** :
- le filtrage `warning` / `debug` / `info` fonctionne ;
- le format d’export texte inclut source, niveau, session et message ;
- `npm run test`, `npm run build`, `cargo check` passent.

**Risques** :
- confusion si l’export contient des identifiants de session sans contexte suffisant ;
- multiplication de boutons si les filtres ne sont pas compactés correctement ;
- comportement de téléchargement légèrement différent selon l’environnement desktop.

**Questions ouvertes** :
- faut-il ensuite ajouter un copier-coller direct vers le presse-papiers en plus du téléchargement ?
- faut-il permettre l’export du journal filtré depuis `Historique & Journaux` aussi ?

## Chantier 20 - Sauvegarde native du journal expert

**Objectif** : remplacer le téléchargement WebView du journal technique filtré par une sauvegarde native Tauri avec validation backend de la destination.

**Pourquoi** :
- un téléchargement navigateur masque la destination réelle et ne permet pas de garantir qu’elle n’atterrit pas sur le disque source ;
- le mode expert doit rester aligné avec la promesse de sûreté read-only et de traçabilité des actions ;
- une boîte de dialogue native améliore aussi l’expérience desktop attendue pour ce produit.

**Hypothèses** :
- l’application s’exécute principalement dans le runtime Tauri desktop ;
- en dehors de Tauri, il vaut mieux refuser explicitement l’export local plutôt que retomber sur un téléchargement non validable ;
- la validation existante de destination peut être réutilisée pour un fichier journal choisi dans un répertoire existant.

**Risques** :
- confusion si l’utilisateur annule la boîte de dialogue et pense que l’export a échoué ;
- divergence entre les builds desktop et navigateur de développement ;
- erreurs d’écriture locale si le répertoire choisi n’existe plus ou n’est pas accessible au moment de l’enregistrement.

**Modules impactés** :
- `PLANS.md`
- `src/pages/ExpertPage.tsx`
- `src/hooks/useIpc.ts`
- `src/i18n/locales/en.json`
- `src/i18n/locales/fr.json`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `package.json`
- `src-tauri/Cargo.toml`

**Plan d’exécution** :
1. ajouter le plugin Tauri de dialogue et exposer une commande backend d’écriture de rapport texte ;
2. brancher `ExpertPage` sur le sélecteur natif puis sur la commande backend sécurisée ;
3. afficher un retour utilisateur explicite en succès, indisponibilité runtime ou erreur d’écriture ;
4. ajouter des tests Rust sur l’écriture du rapport ;
5. relancer `npm run test`, `npm run build` et `cargo check`.

**Contrats et interfaces** :
- `save_technical_timeline_report(destination_path, content, source_device_path?)` doit refuser tout contenu vide ;
- si `source_device_path` est fourni, la destination doit passer par `validate_export_destination` avant écriture ;
- `ExpertPage` ne doit proposer aucun fallback de téléchargement implicite hors Tauri.

**Critères de validation** :
- le journal filtré peut être enregistré via une boîte de dialogue native ;
- l’écriture est refusée si la destination ne peut pas être validée ou si le contenu est vide ;
- les messages FR/EN décrivent clairement le succès, l’indisponibilité et l’échec ;
- `npm run test`, `npm run build` et `cargo check` passent.

**Limites connues** :
- ce jalon ne couvre pas encore un sélecteur de destination natif pour l’export depuis `Historique & Journaux` ;
- la destination est choisie fichier par fichier, sans dossier d’exports favoris ni chiffrement du journal.

## Chantier 21 - Récupération supprimée MVP FAT32

**Objectif** : livrer une première verticale réelle de récupération de fichiers supprimés sur un seul filesystem, FAT32, avec imagerie read-only locale, analyse de la structure FAT32, détection d’entrées supprimées et export des fichiers reconstruits.

**Pourquoi** :
- la valeur produit ne progressera plus vraiment tant qu’on reste sur le seul catalogage des fichiers lisibles ;
- FAT32 est le meilleur premier candidat pour un MVP supprimé, car ses entrées de répertoire supprimées sont plus accessibles qu’en NTFS/APFS ;
- cette verticale permet enfin de valider le vrai pipeline forensic : lecture source minimale, travail sur image, analyse filesystem, récupération, export.

**Hypothèses** :
- le MVP se limite à FAT32 ;
- la récupération initiale cible prioritairement les fichiers supprimés dont le point de départ est connu et dont la reconstruction peut être faite de manière conservative ;
- pour cette première tranche, les noms longs FAT32 peuvent ne pas être complètement reconstruits et les fichiers seront parfois présentés via leur nom court 8.3 ;
- l’imagerie peut être déclenchée dans le pipeline de scan supprimé sans exposer encore un workflow autonome complet “Créer une image”.

**Risques** :
- confusion utilisateur si on ne distingue pas assez clairement “fichier lisible catalogué” et “fichier supprimé reconstruit” ;
- faux sentiment d’intégrité si on reconstruit un fichier supprimé sans connaître parfaitement sa fragmentation ;
- lecture brute du device pouvant échouer selon l’OS, les permissions ou le type de support.

**Modules impactés** :
- `PLANS.md`
- `src-tauri/src/lib.rs`
- `src-tauri/src/types/mod.rs`
- `src-tauri/src/core/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/imaging/mod.rs`
- `src-tauri/src/analyzers/mod.rs`
- `src-tauri/src/analyzers/fat32.rs`
- `src/hooks/useIpc.ts`
- `src/types/results.ts`
- `src/types/scan.ts`
- `src/pages/ScanPage.tsx`
- `src/pages/ResultsPage.tsx`
- `src/pages/ExportPage.tsx`
- `src/pages/DiagnosticPage.tsx`
- `src/i18n/locales/en.json`
- `src/i18n/locales/fr.json`

**Architecture concernée** :
- `imaging` : création d’une image locale read-only du support source ;
- `filesystem analyzers` : parseur FAT32 minimal orienté fichiers supprimés ;
- `preview/export` : export de bytes reconstruits depuis l’image au lieu du point de montage ;
- `desktop app` : lancement d’un scan supprimé explicite et affichage honnête des limites de récupération.

**Contrats et interfaces** :
- le scan supprimé FAT32 doit créer une image locale dans un espace de travail distinct du support source ;
- l’analyse FAT32 doit identifier des entrées supprimées candidates et retourner uniquement des résultats dont la reconstruction est tentée de manière conservative ;
- les résultats doivent distinguer un fichier supprimé reconstruit d’un fichier lisible catalogué ;
- l’export doit savoir recopier soit depuis le filesystem monté, soit depuis l’image locale selon le type de résultat ;
- si le support n’est pas FAT32 ou si l’imagerie échoue, l’application doit l’expliquer explicitement.

**UX / UI** :
- le flux doit être exposé comme un scan supprimé FAT32 MVP, pas comme une récupération universelle ;
- les messages doivent rappeler que la reconstruction reste estimative, surtout en cas de fragmentation inconnue ;
- l’utilisateur doit voir qu’une image locale de travail est utilisée pour limiter les lectures répétées sur la source.

**Étapes d’implémentation** :
1. créer les modules `imaging` et `analyzers/fat32` ;
2. implémenter une image locale read-only de travail ;
3. parser le boot sector FAT32, les FATs et les répertoires pour trouver des entrées supprimées ;
4. enrichir les résultats avec les métadonnées nécessaires à l’export depuis image ;
5. brancher un scan supprimé explicite dans l’UI ;
6. adapter l’export ;
7. ajouter des tests unitaires et un test d’intégration sur une image FAT32 synthétique ;
8. relancer `npm run test`, `npm run build`, `cargo check`.

**Critères de validation** :
- un scan supprimé FAT32 peut être lancé depuis l’UI ;
- une image locale de travail est créée sans écrire sur la source ;
- au moins un cas de test d’image FAT32 synthétique produit un fichier supprimé détecté ;
- ce fichier peut être exporté depuis l’image vers une destination sûre ;
- les résultats et messages de l’UI distinguent clairement ce MVP de la récupération complète multi-filesystem.

**Limites connues** :
- FAT32 uniquement pour ce jalon ;
- prise en charge initiale conservative, possiblement limitée aux fichiers supprimés non fragmentés ou reconstruits de manière contiguë ;
- pas de support complet des noms longs FAT32 dans cette première tranche ;
- pas encore de NTFS/exFAT/APFS ni de vrai carving par signatures.

## Chantier 22 - Fidélité des métadonnées FAT32 supprimées

**Objectif** : améliorer le MVP FAT32 supprimé en reconstruisant les noms longs (LFN) quand les entrées associées sont encore présentes dans le répertoire.

**Pourquoi** :
- le MVP actuel retrouve des fichiers supprimés, mais perd une partie importante de leur valeur utilisateur si on ne montre qu’un alias 8.3 ;
- la qualité des résultats dépend autant de la reconstruction des métadonnées que des bytes ;
- cette étape augmente la lisibilité sans changer le périmètre filesystem ni la promesse de sûreté.

**Périmètre** :
- couvert : parsing des entrées LFN FAT32, reconstruction du nom affiché pour les fichiers supprimés, propagation jusqu’à l’UI et tests synthétiques ;
- non couvert : reconstruction garantie si les slots LFN ont été réécrits, support complet des timestamps FAT32 supprimés, deuxième filesystem ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/fat32.rs`, éventuellement messages UI FAT32 ;
- hypothèses : les entrées LFN supprimées restent contiguës au short entry supprimé tant qu’elles n’ont pas été réutilisées.

**Critères de validation** :
- un fichier FAT32 supprimé avec slots LFN encore présents ressort avec son nom long ;
- en absence de LFN, le fallback 8.3 continue à fonctionner ;
- `npm run test`, `npm run build` et `cargo check` restent verts.

## Chantier 23 - Reconstruction conservative des suppressions FAT32 multi-clusters

**Objectif** : éviter de présenter comme complets des fichiers FAT32 supprimés dont seule une partie contiguë et encore libre peut être reconstruite de manière fiable.

**Pourquoi** :
- un fichier supprimé multi-clusters ne doit pas être exporté comme “intact” si les clusters suivants ont déjà été réutilisés ;
- le MVP doit préférer une récupération partielle honnête à une reconstruction optimiste mais trompeuse ;
- cette étape améliore directement la sûreté métier du pipeline supprimé FAT32 déjà en place.

**Périmètre** :
- couvert : calcul de taille réellement reconstructible, intégrité `partial/fragmented` plus fidèle, export partiel depuis image locale, messages UI associés, tests synthétiques ;
- non couvert : résolution générale de fragmentation FAT32, réassemblage non contigu confirmé, deuxième filesystem ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/fat32.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/imaging/mod.rs`, UI résultats/export ;
- hypothèses : si la chaîne FAT supprimée est absente, seule une plage contiguë de clusters encore libres à partir du cluster initial est suffisamment fiable pour ce MVP.

**Critères de validation** :
- un fichier supprimé FAT32 partiellement reconstructible expose une taille récupérable inférieure à la taille attendue ;
- l’export reconstruit seulement les bytes jugés fiables et reste validé côté backend ;
- l’UI laisse voir qu’une taille attendue peut être supérieure à la taille récupérable ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 24 - Horodatages disponibles pour les suppressions FAT32

**Objectif** : remonter les horodatages réellement disponibles dans les entrées FAT32 supprimées afin d’améliorer le triage sans inventer de date de suppression.

**Pourquoi** :
- un nom et une taille ne suffisent pas toujours à identifier le bon fichier supprimé ;
- FAT32 conserve souvent des timestamps de création/modification dans l’entrée supprimée, ce qui augmente la valeur du MVP ;
- il faut rester strictement honnête : FAT32 ne fournit pas un `deleted_at` fiable dans ce workflow.

**Périmètre** :
- couvert : extraction des timestamps FAT32 disponibles depuis les entrées supprimées, propagation Rust/TS, affichage sobre dans `ResultsPage`, tests synthétiques ;
- non couvert : vrai timestamp de suppression, timezone source fiable, enrichissement global des scans mounted-volume, deuxième filesystem ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/fat32.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/hooks/useIpc.ts`, `src/pages/ResultsPage.tsx`, i18n ;
- hypothèses : les horodatages FAT32 présents dans l’entrée supprimée sont encore lisibles tant que cette entrée n’a pas été réécrite.

**Critères de validation** :
- un résultat FAT32 supprimé peut exposer `created_at` et `modified_at` quand ces métadonnées existent ;
- aucun faux `deleted_at` n’est introduit ;
- l’UI distingue clairement ces métadonnées source sans changer la promesse de sûreté ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 25 - Workflow autonome d’imagerie read-only locale

**Objectif** : exposer enfin un workflow autonome `Créer une image` qui réutilise le moteur d’imagerie existant, avec progression, journaux et historique, sans écrire sur la source.

**Pourquoi** :
- l’imagerie existait déjà à l’intérieur du scan FAT32 supprimé, mais pas comme étape autonome accessible à l’utilisateur ;
- sur un support fragile, créer une image locale une seule fois est plus sûr que répéter les lectures directes ;
- ce jalon ferme un manque produit important sans ouvrir un nouveau moteur filesystem.

**Périmètre** :
- couvert : session d’imagerie autonome depuis un support détecté, progression read-only, journaux techniques, historique de session, intégration UI depuis `DevicesPage`, `DiagnosticPage` et `ScanPage` ;
- non couvert : destination personnalisée choisie par l’utilisateur, reprise sur interruption, checksum d’image, montage/analyse automatique de l’image produite ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/imaging/mod.rs`, `src/pages/DevicesPage.tsx`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, i18n ;
- hypothèses : le workflow autonome crée une image locale de travail dans le workspace applicatif, jamais sur le support source.

**Critères de validation** :
- `Create Image` lance une vraie session backend au lieu d’un placeholder ;
- la progression et les logs d’imagerie sont consultables comme les autres sessions ;
- l’historique reflète une session de type `image` terminée ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 26 - Destination native choisie pour l’image disque

**Objectif** : permettre au workflow autonome `Créer une image` d’écrire l’image read-only vers une destination choisie explicitement par l’utilisateur, au lieu du seul workspace applicatif.

**Pourquoi** :
- une image disque utile doit pouvoir être rangée sur un support de destination maîtrisé par l’utilisateur ;
- l’imagerie autonome restait incomplète tant que la sortie était imposée dans un répertoire interne ;
- cette étape rapproche le workflow d’un usage réel tout en conservant les garde-fous de sûreté.

**Périmètre** :
- couvert : sélecteur natif de destination côté UI, nouvelle commande backend dédiée à l’imagerie avec chemin explicite, validation read-only contre le support source, journalisation et historique mis à jour, tests Rust associés ;
- non couvert : reprise sur interruption, checksum d’image, reprise incrémentale, analyse automatique de l’image créée ;
- dépendances : `PLANS.md`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/types/scan.ts`, `src/pages/DevicesPage.tsx`, `src/pages/ScanPage.tsx`, i18n ;
- hypothèses : l’utilisateur choisit un chemin de fichier sur un autre support déjà monté et accessible.

**Critères de validation** :
- `Créer une image` ouvre un sélecteur natif et n’amorce pas l’imagerie sans destination explicite ;
- le backend refuse une destination située sur le même support physique que la source ;
- la session d’imagerie conserve progression, logs et historique, avec le chemin final réel dans les logs ;
- le workflow FAT32 supprimé existant continue d’utiliser son image locale de travail interne sans régression ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 27 - Préflight d’imagerie brute et permissions macOS

**Objectif** : fiabiliser le workflow d’imagerie read-only sur macOS en ciblant le bon device physique pour l’acquisition et en bloquant proprement les cas où l’utilisateur courant ne peut pas lire le device brut.

**Pourquoi** :
- l’imagerie actuelle échoue trop tard avec un simple `Permission denied`, ce qui n’est pas acceptable pour un produit sérieux ;
- sur macOS, un volume monté peut être scannable alors que son device brut reste inaccessible au process courant ;
- il faut distinguer clairement un scan read-only possible d’une imagerie brute impossible sur l’hôte courant.

**Périmètre** :
- couvert : sélection du bon device d’imagerie (whole/raw device), préflight backend de lisibilité, enrichissement du diagnostic avec disponibilité/blocage d’imagerie, désactivation UI des actions d’imagerie impossibles, tests unitaires associés ;
- non couvert : helper privilégié système, élévation automatique de privilèges, entitlement macOS signé, acquisition kernel-level ;
- dépendances : `PLANS.md`, `src-tauri/src/core/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/types/mod.rs`, `src/hooks/useIpc.ts`, `src/types/diagnostic.ts`, `src/pages/DiagnosticPage.tsx`, `src/pages/DevicesPage.tsx` ;
- hypothèses : le build courant tourne sans helper privilégié dédié, donc certains devices bruts resteront non imageables selon les permissions de l’hôte.

**Critères de validation** :
- l’imagerie tente le bon chemin source physique pour un volume monté ;
- un device brut non lisible renvoie un message métier clair et actionnable ;
- le diagnostic expose qu’un scan monté peut être disponible alors que l’imagerie brute est bloquée ;
- l’UI ne pousse plus aveuglément un workflow d’imagerie déjà connu comme bloqué ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 28 - Fallback d’imagerie privilégiée macOS

**Objectif** : permettre à l’application macOS de lancer une acquisition read-only d’un device brut nécessitant des privilèges administrateur, sans introduire d’écriture sur la source et sans dépendre d’un faux helper séparé.

**Pourquoi** :
- le préflight actuel sait expliquer un blocage de permissions, mais il ne transforme pas encore ce constat en capacité produit réelle ;
- sur macOS, la lecture d’un volume monté et la lecture du device brut n’obéissent pas aux mêmes permissions ;
- pour progresser vers un vrai workflow d’imagerie haut de gamme, il faut un fallback contrôlé, traçable et honnête avant le helper signé de niveau distribution.

**Périmètre** :
- couvert : mode helper réutilisant le binaire existant, commande backend choisissant accès direct ou élévation, progression/logs pendant l’imagerie privilégiée, diagnostic enrichi sur le besoin d’élévation, message UI avant prompt administrateur, tests unitaires et d’intégration ciblés ;
- non couvert : helper LaunchDaemon/SMJobBless signé, entitlements Apple de distribution, reprise sur interruption, checksum d’image, support privilégié Windows/Linux ;
- dépendances : `PLANS.md`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/privileged_imager.rs`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/mod.rs`, contrats Rust/TS de diagnostic, `DevicesPage`, `DiagnosticPage`, `ScanPage`, i18n ;
- hypothèses : sur macOS, `osascript` est disponible pour demander une élévation locale ponctuelle et le binaire courant peut être réinvocable en mode CLI helper.

**Critères de validation** :
- un source raw lisible directement reste traité sans élévation ;
- un source raw bloqué par permissions mais éligible au fallback reste “imageable” avec approbation admin ;
- la progression et les logs d’imagerie restent cohérents pendant le fallback privilégié ;
- l’UI annonce le besoin d’approbation administrateur avant le prompt système ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 29 - Garde-fou de démarrage dev contre la fenêtre blanche

**Objectif** : empêcher qu’un lancement direct du binaire desktop en mode développement ouvre une fenêtre blanche silencieuse quand le frontend Vite n’est pas disponible.

**Pourquoi** :
- une fenêtre blanche sans message explicite ralentit le diagnostic et fait perdre confiance dans l’état réel du produit ;
- le build dev dépend de `devUrl`, donc l’application doit échouer proprement si ce frontend manque ;
- ce garde-fou réduit le temps perdu côté développement et évite de confondre une panne de tooling avec un bug UI métier.

**Périmètre** :
- couvert : préflight Rust au démarrage en mode dev, message d’erreur clair avant ouverture de fenêtre, tests unitaires du garde-fou ;
- non couvert : mécanisme de relance automatique de Vite, fallback HTML embarqué, comportement bundle/release ;
- dépendances : `PLANS.md`, `src-tauri/src/lib.rs`, nouveau module de garde de démarrage, éventuellement message natif macOS ;
- hypothèses : en développement, l’URL attendue reste `http://localhost:1420`.

**Critères de validation** :
- lancer le binaire desktop dev sans serveur frontend n’ouvre plus une fenêtre blanche ;
- le message d’erreur indique comment lancer correctement l’application ;
- quand Vite est disponible, le démarrage normal n’est pas dégradé ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 30 - Récupération supprimée MVP exFAT

**Objectif** : étendre la verticale de récupération supprimée read-only à `exFAT`, avec image locale, analyse conservative des entrées supprimées, résultats exportables et UI alignée sur les limites réelles.

**Pourquoi** :
- le périmètre MVP visé mentionne `FAT32` et `exFAT`, mais seule la branche FAT32 est aujourd’hui réellement implémentée ;
- `exFAT` est fréquent sur clés USB, cartes SD et disques externes, donc son absence laisse un vrai trou produit ;
- ce jalon augmente la couverture réelle sans promettre encore un moteur `NTFS` ou du carving par signatures.

**Périmètre** :
- couvert : nouvel analyseur `exFAT` supprimé read-only sur image locale, usage de la bitmap d’allocation pour rester conservative, intégration au scan `carving`, propagation vers résultats/export, microcopies FR/EN et tests synthétiques ;
- non couvert : `NTFS`, carving par signatures, reconstruction avancée de fragmentation non contiguë, preview binaire, analyse de partitions perdues ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/mod.rs`, nouveau `src-tauri/src/analyzers/exfat.rs`, `src-tauri/src/commands/mod.rs`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, `src/pages/HistoryPage.tsx`, i18n ;
- hypothèses : le MVP `exFAT` s’appuie sur une image locale read-only et ne reconstruit que des plages de clusters encore jugées fiables, prioritairement via la bitmap d’allocation et, si disponible, la chaîne FAT.

**Critères de validation** :
- un support `exFAT` propose un workflow supprimé depuis le diagnostic et la page scan ;
- le backend refuse encore les filesystems non pris en charge, mais accepte `FAT32` et `exFAT` pour `carving` ;
- les résultats/export `exFAT` reconstruisent les bytes à partir de l’image locale sans écrire sur la source ;
- des tests synthétiques couvrent au minimum un cas intact et un cas partiel `exFAT` ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 31 - Carving par signatures MVP

**Objectif** : rendre réel un premier workflow de `carving par signatures` read-only, capable d’imager localement une source puis de rechercher quelques formats connus directement dans l’image, avec résultats exportables et messaging honnête.

**Pourquoi** :
- le produit affiche déjà un stage `carving-signatures`, mais aucun moteur réel ne l’exécute encore ;
- ce workflow apporte une vraie valeur sur des volumes où l’analyseur filesystem supprimé n’existe pas encore ou ne retrouve rien ;
- un MVP limité à quelques signatures connues reste beaucoup plus crédible qu’un faux moteur “générique”.

**Périmètre** :
- couvert : moteur Rust de carving signature-based sur image locale read-only, formats `JPEG`, `PNG`, `PDF`, `ZIP`, nouveau type de scan dédié, intégration diagnostic/scan/résultats/export/historique, tests synthétiques backend ;
- non couvert : fragmentation non contiguë avancée, bases de signatures extensibles, `DOCX/XLSX` différenciés au-delà du conteneur ZIP, vidéo lourde, preview binaire riche ;
- dépendances : `PLANS.md`, nouveau module `src-tauri/src/carving/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`, `src/types/scan.ts`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, `src/pages/HistoryPage.tsx`, `src/pages/HomePage.tsx`, i18n ;
- hypothèses : le carving MVP reste conservative, saute les régions déjà taillées pour éviter les doublons évidents, et borne chaque format avec des tailles maximales raisonnables.

**Critères de validation** :
- un scan `signature-carving` peut être lancé depuis l’UI ;
- le backend image la source en lecture seule puis produit des résultats `recovery_method=carving` ;
- les résultats/export reconstruisent les bytes depuis l’image locale, jamais depuis un chemin source inscriptible ;
- les tests couvrent au minimum un cas `JPEG` et un cas `PNG/PDF/ZIP` avec export ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 32 - Récupération supprimée MVP NTFS

**Objectif** : étendre la verticale de récupération supprimée read-only à `NTFS`, avec image locale, lecture conservative des enregistrements `MFT` supprimés, résultats exportables et UI alignée sur les limites réelles.

**Pourquoi** :
- `NTFS` fait partie du périmètre MVP produit initial, mais il n’existe encore aucun analyseur supprimé réel dans le backend ;
- une part importante des disques externes et volumes Windows utiles au produit sont en `NTFS`, donc son absence reste un trou produit majeur ;
- un MVP limité aux cas `resident` et `non-resident` à runlist encore exploitable apporte une vraie valeur sans promettre une récupération magique.

**Périmètre** :
- couvert : nouvel analyseur `NTFS` supprimé read-only sur image locale, lecture du boot sector, scan des enregistrements `MFT`, parsing des attributs `FILE_NAME` et `DATA`, usage conservative du bitmap d’allocation `NTFS`, intégration au scan supprimé, propagation vers résultats/export, microcopies FR/EN et tests synthétiques ;
- non couvert : journal `$LogFile`, USN journal, ADS nommés, fichiers compressés/chiffrés/sparse, réparation de runlists corrompues, partitions perdues, preview binaire ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/mod.rs`, nouveau `src-tauri/src/analyzers/ntfs.rs`, `src-tauri/src/commands/mod.rs`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, `src/types/diagnostic.ts`, i18n ;
- hypothèses : le MVP `NTFS` ne reconstruit que les fichiers dont les attributs `DATA` restent présents dans le `MFT` supprimé et dont les clusters sont encore jugés libres via le bitmap courant.

**Critères de validation** :
- un support `NTFS` propose un workflow supprimé depuis le diagnostic et la page scan ;
- le backend accepte désormais `FAT32`, `exFAT` et `NTFS` pour le scan supprimé ;
- les résultats/export `NTFS` reconstruisent les bytes depuis l’image locale sans écrire sur la source ;
- des tests synthétiques couvrent au minimum un cas `resident` intact et un cas `non-resident` partiel ou intact ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 33 - Preview engine local MVP

**Objectif** : rendre réel un premier moteur d’aperçu local pour les résultats, capable d’afficher du texte lisible inline et d’ouvrir des images/PDF via un asset local sûr, y compris pour des fichiers supprimés reconstruits depuis l’image.

**Pourquoi** :
- le produit expose déjà `preview_available`, mais il ne s’agit encore que d’un badge dérivé de l’extension, sans expérience d’aperçu réelle ;
- un vrai aperçu aide l’utilisateur à valider rapidement si un résultat mérite un export, ce qui change fortement la valeur pratique de l’écran `Résultats` ;
- la prévisualisation doit réutiliser les pipelines read-only existants, pas créer une seconde logique de lecture divergente.

**Périmètre** :
- couvert : nouvelle commande backend de preview, lecture inline des petits fichiers texte (`txt`, `md`, `log`, `json`, `csv`), génération d’asset local temporaire pour `jpg/jpeg/png/gif/webp/pdf`, support des fichiers catalogués et des fichiers supprimés/carvés via image locale, panneau d’aperçu UI, messages FR/EN, tests backend ;
- non couvert : preview vidéo/audio, aperçu hexadécimal expert, OCR, thumbnails multiples, nettoyage automatique avancé du cache de preview, preview DOCX/XLSX native ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, nouveau module backend preview ou helpers dédiés, `src-tauri/src/commands/mod.rs`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/pages/ResultsPage.tsx`, nouveau composant d’aperçu, i18n ;
- hypothèses : les previews restent strictement locales, les assets temporaires sont créés sur le disque applicatif et les gros fichiers texte sont tronqués de manière explicite.

**Critères de validation** :
- cliquer sur un résultat previewable ouvre un vrai panneau d’aperçu dans `Résultats` ;
- les fichiers texte lisibles renvoient un contenu inline tronqué explicitement si nécessaire ;
- les images/PDF issus d’un scan supprimé ou d’un carving sont reconstruits dans un asset local temporaire puis affichables via la WebView ;
- l’aperçu n’écrit jamais sur la source et réutilise les bytes du volume monté ou de l’image locale de récupération ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 34 - Contrôles réels pause / reprise / arrêt des scans

**Objectif** : rendre réels les contrôles `pause`, `resume` et `stop` du workflow de scan, avec propagation backend, statut persistant et UI alignée.

**Pourquoi** :
- l’interface parle déjà d’états `paused` / `cancelled`, mais aucun contrôle opératoire n’existe encore ;
- sur des scans longs ou de l’imagerie, l’utilisateur doit pouvoir interrompre ou suspendre un workflow sans tuer brutalement l’application ;
- ce contrôle améliore à la fois la crédibilité produit et la sécurité opérationnelle sur des supports fragiles.

**Périmètre** :
- couvert : état de contrôle coopératif par session, nouvelles commandes IPC `pause_scan`, `resume_scan`, `cancel_scan`, intégration aux boucles de catalogage et d’imagerie, propagation du statut persistant, boutons UI réels dans `ScanPage`, tests backend ciblés ;
- non couvert : reprise persistée après redémarrage de l’application, arrêt forcé du helper privilégié déjà lancé hors boucle coopérative, annulation instantanée de chaque sous-analyse interne filesystem/carving ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/imaging/mod.rs`, éventuellement `src-tauri/src/carving/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/ScanPage.tsx`, i18n ;
- hypothèses : le contrôle reste coopératif, donc certaines phases déjà lancées peuvent nécessiter quelques checkpoints avant de refléter effectivement une pause ou un arrêt.

**Critères de validation** :
- un scan en cours peut être mis en pause puis repris depuis `ScanPage` ;
- un scan en cours peut être arrêté proprement avec statut `cancelled` et logs explicites ;
- les scans montés et l’imagerie respectent réellement ces contrôles sans écrire sur la source ;
- l’historique et `get_scan_progress` reflètent correctement les statuts `paused` et `cancelled` ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 35 - Détection conservative de partitions perdues MVP

**Objectif** : ajouter un premier workflow sérieux de détection read-only de volumes potentiels perdus, en combinant lecture de table de partitions (`MBR` / `GPT`) et repérage de signatures boot sector `NTFS` / `FAT32` / `exFAT`, puis exposer ces candidats dans le diagnostic et le mode expert sans prétendre reconstruire magiquement un volume complet.

**Pourquoi** :
- le prompt produit cible explicitement les partitions perdues, mais l’application ne sait aujourd’hui afficher que les volumes déjà montés par l’OS ;
- avant toute vraie reconstruction avancée, il faut déjà rendre visibles des volumes plausibles, avec un niveau de confiance et des limites claires ;
- ce jalon crée une base modulaire réutilisable plus tard pour un vrai workflow “scanner un volume retrouvé”.

**Périmètre** :
- couvert : nouveau module backend de détection de volumes potentiels depuis une source read-only accessible, parsing primaire `MBR`, `GPT` primaire/backup, validation par signatures boot sector `NTFS` / `FAT32` / `exFAT`, nouveaux contrats partagés, intégration au diagnostic et à `ExpertPage`, microcopies FR/EN, tests backend ciblés ;
- non couvert : montage virtuel d’un volume retrouvé, reconstruction complète d’une table de partitions écrite sur disque, workflow d’export direct depuis un volume retrouvé, support `APFS/HFS+/EXT4`, scan profond de tout le disque à granularité fine ;
- dépendances : `PLANS.md`, nouveau module backend de détection de partitions, `src-tauri/src/commands/mod.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/types/device.ts`, `src/types/diagnostic.ts`, `src/pages/DiagnosticPage.tsx`, `src/pages/ExpertPage.tsx`, i18n ;
- hypothèses : ce MVP n’inspecte que les sources lisibles par le process courant et reste volontairement conservative ; un candidat signalé n’est pas une preuve qu’un volume sera intégralement récupérable.

**Critères de validation** :
- le backend peut retourner des volumes potentiels avec offset, taille estimée, filesystem probable, méthode de détection et score de confiance ;
- le diagnostic peut signaler un cas `partition-lost` quand des candidats plausibles sont visibles sur une source non montée ;
- `ExpertPage` affiche une table distincte des volumes potentiels retrouvés, séparée des partitions déjà montées ;
- les textes UI expliquent clairement qu’il s’agit d’une détection conservative et non d’une reconstruction garantie ;
- des tests backend couvrent au minimum un cas `MBR + FAT32`, un cas `GPT + NTFS/exFAT` et un cas de signature boot sector sans table exploitable ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 36 - Analyse read-only d’un volume potentiel retrouvé MVP

**Objectif** : permettre de lancer une vraie analyse read-only sur un volume potentiel détecté, en créant d’abord une image locale de travail puis un slice local du candidat retrouvé, avant de réutiliser les analyseurs `NTFS` / `FAT32` / `exFAT` déjà existants.

**Pourquoi** :
- après le chantier de détection, un volume potentiel visible mais non exploitable reste insuffisant côté produit ;
- transformer un candidat retrouvé en source analysable crée enfin une vraie verticale “partition perdue -> résultats” ;
- l’approche par image locale + slice local conserve les garde-fous read-only du projet et évite d’introduire un accès offset bas niveau partout dans les analyseurs existants.

**Périmètre** :
- couvert : nouveau workflow `lost-volume`, commande IPC dédiée, extension des contrats frontend/backend, extraction locale d’un slice de volume candidat, réutilisation des pipelines supprimés `NTFS` / `FAT32` / `exFAT`, entrée UI depuis `Expert`, libellés scan/résultats/export/historique, tests backend et build complet ;
- non couvert : montage virtuel du volume retrouvé, scan catalogue de fichiers visibles depuis un volume perdu, support `APFS/HFS+/EXT4`, optimisation directe par lecture offset sans image intermédiaire, persistance d’un catalogue séparé des candidats hors session active ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/types/mod.rs`, `src/hooks/useIpc.ts`, `src/types/scan.ts`, `src/pages/ExpertPage.tsx`, `src/pages/ScanPage.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, `src/pages/HistoryPage.tsx`, `src/pages/HomePage.tsx`, i18n ;
- hypothèses : le MVP reste limité aux candidats dont le filesystem probable est `NTFS`, `FAT32` ou `exFAT`, et il s’appuie sur une image locale de travail avant analyse.

**Critères de validation** :
- depuis `Expert`, un volume potentiel pris en charge peut lancer un scan dédié ;
- le backend retrouve le candidat, crée une image locale sûre, extrait le slice du volume, puis produit de vrais résultats supprimés si les métadonnées restent lisibles ;
- les écrans `Scan`, `Résultats`, `Export`, `Historique` distinguent ce workflow d’un scan supprimé classique sur volume monté ;
- les tests backend couvrent au minimum un cas `FAT32` retrouvé via `MBR` puis analysé avec succès ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 37 - Parcours guidé direct pour un volume retrouvé

**Objectif** : terminer la verticale “partition perdue” côté produit en permettant au mode guidé de lancer directement l’analyse d’un volume potentiel pris en charge, sans exiger un détour systématique par `Expert`.

**Pourquoi** :
- le backend sait maintenant détecter puis analyser un volume potentiel retrouvé, mais le diagnostic novice n’oriente pas encore directement vers cette action ;
- un utilisateur guidé doit pouvoir comprendre “un volume plausible a été trouvé” puis lancer l’analyse adaptée avec un clic ;
- ce chantier réduit l’écart entre les capacités réelles du moteur et la lisibilité du produit.

**Périmètre** :
- couvert : recommandation backend explicite quand un candidat `NTFS` / `FAT32` / `exFAT` est directement exploitable, extension légère des contrats de recommandation, boutons d’action dans `Diagnostic`, utilitaire frontend partagé pour préparer un `lost-volume` scan, tests backend + unitaires frontend ;
- non couvert : arbitrage expert complexe entre de nombreux candidats concurrents, montage virtuel du volume retrouvé, fusion automatique de plusieurs candidats, support de nouveaux filesystems ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/types/diagnostic.ts`, `src/hooks/useIpc.ts`, `src/pages/DiagnosticPage.tsx`, `src/pages/ExpertPage.tsx`, nouveau helper `src/utils/*`, i18n ;
- hypothèses : quand plusieurs candidats restent plausibles, le produit continue de privilégier l’inspection explicite des offsets et niveaux de confiance avant lancement.

**Critères de validation** :
- le diagnostic peut proposer directement l’analyse d’un volume retrouvé pris en charge quand le cas est suffisamment clair ;
- la liste des volumes potentiels dans `Diagnostic` expose aussi une action directe cohérente avec celle du mode expert ;
- `Diagnostic` et `Expert` réutilisent la même logique de préparation du `scanConfig` pour éviter les divergences ;
- les contrats frontend/backend restent compatibles avec les recommandations existantes ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 38 - Arbitrage guidé conservative entre plusieurs volumes retrouvés

**Objectif** : améliorer le mode guidé quand plusieurs candidats de volumes retrouvés existent, en recommandant automatiquement un candidat seulement s’il se détache clairement, puis en rendant ce choix visible dans `Diagnostic` et `Expert`.

**Pourquoi** :
- le chantier précédent ferme le cas “un seul candidat clair”, mais dès qu’il y a plusieurs candidats le produit redevient trop opaque ;
- un arbitrage conservative aide l’utilisateur novice sans prétendre identifier magiquement le bon volume dans tous les cas ;
- ce jalon réduit les faux négatifs UX tout en gardant une posture prudente quand la situation reste ambiguë.

**Périmètre** :
- couvert : heuristique backend de sélection d’un candidat gagnant seulement quand l’écart est crédible, recommandation `scan-lost-volume` enrichie même avec plusieurs candidats, tri/affichage du candidat recommandé dans `Diagnostic` et `Expert`, utilitaire frontend de classement, tests backend et unitaires frontend ;
- non couvert : fusion de plusieurs candidats, résolution automatique de conflits complexes, validation cryptographique d’un bon volume, montage virtuel, support de nouveaux filesystems ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src/pages/DiagnosticPage.tsx`, `src/pages/ExpertPage.tsx`, `src/utils/*`, i18n ;
- hypothèses : l’heuristique reste volontairement conservative ; en cas de doute, l’app doit continuer à pousser l’inspection explicite plutôt qu’un lancement automatique.

**Critères de validation** :
- une recommandation directe peut apparaître quand un candidat `NTFS` / `FAT32` / `exFAT` domine clairement plusieurs candidats concurrents ;
- aucun candidat n’est auto-recommandé quand plusieurs options restent trop proches ;
- `Diagnostic` et `Expert` affichent le candidat recommandé en tête et le signalent visuellement ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 39 - Catalogue visible + preview/export sur volume retrouvé

**Objectif** : compléter la verticale “volume retrouvé” en cataloguant aussi les fichiers actuellement visibles d’un slice `NTFS` / `FAT32` / `exFAT`, puis en rendant preview et export réellement compatibles avec ces fichiers reconstruits depuis une image locale.

**Pourquoi** :
- un volume potentiel analysé qui ne remonte que les entrées supprimées reste incomplet côté produit alors qu’un utilisateur attend aussi les fichiers encore lisibles du volume retrouvé ;
- preview et export traitent aujourd’hui les fichiers issus d’une image locale uniquement quand `is_deleted=true`, ce qui bloque le cas “visible mais extrait depuis un slice local” ;
- ce jalon ferme une vraie incohérence technique et rapproche le workflow “partition perdue” d’un comportement exploitable bout en bout.

**Périmètre** :
- couvert : listing read-only des fichiers visibles sur slice `FAT32` / `exFAT` / `NTFS`, fusion visible + supprimé dans `lost-volume`, helper backend unifié pour les fichiers alimentés par une image de récupération, compatibilité preview/export/verification, textes UI alignés, tests backend + unitaires ciblés ;
- non couvert : montage virtuel du volume retrouvé, catalogage exhaustif de tous les attributs avancés `NTFS`, support de nouveaux filesystems, reconstruction de fragmentation avancée, nouvelle UI dédiée d’exploration de partitions ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/{fat32,exfat,ntfs}.rs`, `src-tauri/src/commands/mod.rs`, i18n, tests Rust existants ;
- hypothèses : les fichiers visibles d’un volume retrouvé restent exportés/previewés depuis la slice locale et ne doivent jamais nécessiter l’écriture ou le montage du disque source.

**Critères de validation** :
- un scan `lost-volume` pris en charge peut retourner à la fois des fichiers visibles et des entrées supprimées ;
- un fichier visible issu d’une image/slice locale peut être previewé et exporté sans dépendre d’un chemin monté par l’OS ;
- les résultats et messages UI n’induisent plus que le workflow “volume retrouvé” est limité aux seuls fichiers supprimés ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 40 - Vraie arborescence de résultats desktop

**Objectif** : remplacer l’arborescence fictive des résultats par une vraie structure de dossiers/fichiers reconstruite à partir des chemins déjà remontés par le backend, puis l’exposer dans `Résultats` avec une navigation exploitable.

**Pourquoi** :
- `treeRoot` existe déjà dans les types mais reste aujourd’hui une racine plate, ce qui ne tient pas la promesse produit d’une vue arborescente ;
- avec les scans montés, supprimés, carvés et `lost-volume`, les chemins ont maintenant assez de qualité pour offrir une exploration beaucoup plus crédible ;
- une arborescence réelle aide autant le mode novice que l’expert à retrouver un contexte de dossier avant preview/export.

**Périmètre** :
- couvert : helper pur de reconstruction d’arbre depuis `RecoveredFile[]`, tri stable dossiers/fichiers, composant UI de tree view desktop, intégration dans `ResultsPage`, sélection et ouverture preview depuis l’arbre, tests unitaires frontend ;
- non couvert : lazy loading d’un arbre géant côté backend, recherche plein texte dans l’arborescence, drag-and-drop d’export, virtualisation avancée sur très gros catalogues ;
- dépendances : `PLANS.md`, `src/types/results.ts`, nouveau helper `src/utils/*`, nouveau composant `src/components/results/*`, `src/pages/ResultsPage.tsx`, i18n, tests Vitest ;
- hypothèses : l’arborescence reste reconstruite localement côté frontend à partir des résultats déjà chargés, ce qui est suffisant pour le MVP actuel.

**Critères de validation** :
- `RecoveryResult.treeRoot` reflète de vrais dossiers imbriqués au lieu d’une liste plate ;
- `Résultats` affiche une vue arborescente navigable et cohérente avec le tableau ;
- cliquer un fichier dans l’arbre permet au minimum de le sélectionner et d’ouvrir son aperçu quand il est previewable ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 41 - Recherche et virtualisation de l’arborescence

**Objectif** : rendre l’arborescence de résultats réellement exploitable sur de gros catalogues en ajoutant une recherche/filtrage dédiée et un rendu virtualisé des lignes visibles.

**Pourquoi** :
- une vraie arborescence sans recherche devient vite peu pratique sur des scans volumineux ;
- une arborescence riche mais entièrement rendue peut dégrader la fluidité desktop quand beaucoup de nœuds sont ouverts ;
- ce chantier améliore à la fois l’UX immédiate et la robustesse perçue sur des sessions lourdes.

**Périmètre** :
- couvert : filtre texte de l’arborescence, filtres de portée utiles (`all`, `deleted`, `previewable`, `selected`), compteur visible/total, helpers purs de flattening/virtualisation, intégration `useDeferredValue`, rendu virtualisé dans `FileTreePanel`, tests Vitest ;
- non couvert : recherche backend plein texte, indexation persistée, virtualisation adaptative à hauteur variable, drag-and-drop, bookmark/saved filters ;
- dépendances : `PLANS.md`, `src/utils/fileTree.ts`, `src/utils/fileTree.test.ts`, `src/components/results/FileTreePanel.tsx`, `src/pages/ResultsPage.tsx`, i18n ;
- hypothèses : une hauteur de ligne fixe est acceptable pour le MVP de virtualisation et suffit à garder l’interface fluide sur de grands jeux de résultats.

**Critères de validation** :
- l’utilisateur peut rechercher dans l’arborescence par nom/chemin/type ;
- il peut filtrer l’arborescence par portée utile sans casser la logique de sélection/preview ;
- le composant ne rend qu’une fenêtre des lignes visibles tout en gardant le scroll cohérent ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 42 - Filtres et tri avancés des résultats

**Objectif** : rendre `Résultats` réellement exploitable sur des catalogues riches en ajoutant des filtres et un tri desktop partagés entre la table et l’arborescence, au lieu de limiter la vue principale à un simple filtre d’intégrité.

**Pourquoi** :
- le produit promet déjà des filtres par type, taille, date et score, mais la table n’expose encore qu’un sous-ensemble très limité ;
- la table et l’arborescence partent aujourd’hui de jeux de résultats différents, ce qui crée une UX incohérente ;
- ce chantier améliore la lisibilité des scans riches sans ajouter de dette backend ni de logique métier cachée dans les composants.

**Périmètre** :
- couvert : utilitaire pur de filtrage/tri des `RecoveredFile`, recherche texte partagée, filtres `integrity/type/extension/size/score/date`, tri configurable, réutilisation du même sous-ensemble de résultats pour la table et l’arborescence, contrôles UI dans `ResultsPage`, libellés i18n, tests Vitest ;
- non couvert : indexation backend plein texte, sauvegarde persistée de filtres favoris, tri multi-colonnes, facettes dynamiques côté serveur, export implicite du sous-ensemble filtré sans sélection explicite ;
- dépendances : `PLANS.md`, `src/types/results.ts`, nouveau helper `src/utils/*`, `src/pages/ResultsPage.tsx`, `src/utils/fileTree.ts`, i18n, Vitest ;
- hypothèses : les résultats restent chargés localement côté frontend, et un filtrage/tri pur sur `RecoveredFile[]` suffit pour le volume MVP actuel.

**Critères de validation** :
- la table et l’arborescence utilisent la même base filtrée et triée ;
- l’utilisateur peut filtrer par recherche, type, extension, taille, score et fenêtre de date sans casser la sélection ni l’aperçu ;
- le tri est explicite, réversible et cohérent avec les colonnes affichées ;
- la logique critique de filtrage/tri est couverte par des tests unitaires dédiés ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 43 - Visionneuse hexadécimale réelle en mode expert

**Objectif** : remplacer le placeholder de la visionneuse hexadécimale par une vraie inspection locale read-only des octets d’un fichier récupéré, qu’il provienne d’un chemin monté ou d’une reconstruction depuis image.

**Pourquoi** :
- `Expert` affiche encore une zone vide sur un point pourtant central pour un produit de récupération sérieux ;
- le backend sait déjà lire des bytes depuis un chemin source ou des `byte_runs` d’image locale, donc le manque est surtout un manque d’assemblage et de présentation ;
- une vraie vue hex améliore la crédibilité expert sans promettre plus que ce que le moteur sait réellement lire.

**Périmètre** :
- couvert : commande IPC dédiée, lecture segmentée locale depuis fichier monté et depuis image de récupération, pagination simple par offset/fenêtre, affichage hex + ASCII en mode expert, sélection d’un fichier de résultat, libellés i18n, tests backend ciblés ;
- non couvert : édition hex, recherche binaire avancée, inspection brute hors contexte fichier, secteurs arbitraires du disque, annotations persistées, diff binaire entre deux versions ;
- dépendances : `PLANS.md`, `src-tauri/src/{commands,preview,imaging,types}/`, `src/hooks/useIpc.ts`, `src/types/results.ts`, nouveau composant `src/components/expert/*`, `src/pages/ExpertPage.tsx`, i18n ;
- hypothèses : le MVP se limite à une fenêtre locale de bytes d’un fichier déjà présent dans les résultats du scan courant, sans exposer l’accès arbitraire à tout le disque.

**Critères de validation** :
- `Expert` permet de choisir un fichier de résultat courant et d’inspecter une fenêtre d’octets réelle en hexadécimal ;
- le même flux fonctionne pour un fichier catalogué sur volume monté et pour un fichier reconstruit depuis image locale ;
- la navigation par offset reste strictement read-only et bornée ;
- des tests backend couvrent au minimum un cas fichier local et un cas fichier recovery-backed ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 44 - Aperçu documentaire local DOCX/XLSX

**Objectif** : étendre le moteur d’aperçu local pour lire de vrais contenus bureautiques `DOCX` et `XLSX`, sans changer le principe read-only ni prétendre reconstruire un document Office corrompu au-delà de ce qui reste extractible localement.

**Pourquoi** :
- le produit sait déjà afficher texte, image et PDF, mais reste encore faible sur des formats très fréquents en récupération réelle ;
- un technicien ou une PME veut souvent vérifier rapidement un document Word ou un classeur Excel avant export ;
- ce jalon améliore fortement la valeur du moteur de preview sans ouvrir de promesse irréaliste sur les fichiers partiels ou cassés.

**Périmètre** :
- couvert : extraction textuelle locale conservative de `DOCX` et `XLSX`, prise en charge depuis chemin monté et depuis reconstruction locale d’un fichier appuyé sur image, mise à jour des extensions previewables et du MIME, tests Rust ciblés, réutilisation de `ResultsPage` sans refonte UI ;
- non couvert : `PPTX`, rendu fidèle de mise en page, formules Excel évaluées, macros, objets embarqués, reconstruction de documents Office corrompus, conversion PDF ;
- dépendances : `PLANS.md`, `src-tauri/src/{preview,commands,imaging}/`, `src-tauri/Cargo.toml`, i18n si besoin, tests Rust ;
- hypothèses : l’aperçu reste textuel et local ; si l’archive Office est trop partielle ou invalide, l’application doit l’indiquer honnêtement.

**Critères de validation** :
- un `DOCX` local lisible peut être previewé comme texte utile ;
- un `XLSX` local lisible peut exposer au moins un aperçu tabulaire textuel conservative ;
- le même flux fonctionne aussi pour un fichier recovery-backed reconstruit vers un workspace local de preview ;
- les cas corrompus échouent proprement sans planter l’UI ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 45 - Couche AI produit locale et optionnelle

**Objectif** : sortir la logique d'assistance strategique du simple diagnostic heuristique pour en faire une vraie couche produit separee, locale par defaut, testable, et prete a accueillir plus tard un provider cloud strictement optionnel sans coupler l'UI au moteur bas niveau.

**Pourquoi** :
- le produit expose deja des scores et recommandations, mais pas encore une couche AI/product intelligence clairement delimitee ;
- sans contrat dedie, il est difficile de faire evoluer proprement l'explication, la priorisation, et plus tard l'option cloud sans disperser la logique dans `commands` et dans le frontend ;
- ce chantier cree une base credible et industrialisable pour l'assistance utilisateur sans promettre de "magic recovery".

**Périmètre** :
- couvert : module backend `ai` dedie, contrat IPC `AI advisory`, moteur local d'assistance strategique fonde sur le diagnostic et les capacites reelles du build, exposition dans `Diagnostic` et `Parametres`, signalement explicite du mode local et du cloud optionnel non configure, tests backend cibles ;
- non couvert : appel LLM distant reel, envoi de donnees vers un cloud, reconstruction assistee de contenu, regroupement de fragments par modele, scoring neuronal, orchestration multi-agent ;
- dépendances : `PLANS.md`, `src-tauri/src/{ai,commands,lib,types}/`, `src/hooks/useIpc.ts`, `src/types/*`, `src/pages/{DiagnosticPage,SettingsPage}.tsx`, i18n ;
- hypothèses : la premiere version de cette couche IA reste locale et explicable ; elle aide a classer, resumer et recommander, mais ne remplace pas le moteur low-level ni les limites physiques du support.

**Critères de validation** :
- le backend expose un contrat `AI advisory` distinct du diagnostic brut ;
- l'avis IA local fournit au minimum un resume, une confiance, une strategie recommandee, des precautions et des prochaines etapes ;
- l'UI `Diagnostic` affiche cet avis sans devenir bloquante si la couche IA est indisponible ;
- `Parametres` distingue clairement assistance locale disponible et cloud optionnel non configure ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 46 - Recuperation supprimee EXT4 MVP

**Objectif** : ouvrir la phase suivante filesystems avec un premier MVP `EXT4` honnete, en lecture seule, capable d'analyser une image locale et de reconstruire des inodes supprimes orphelins quand leurs blocs apparaissent encore libres dans la bitmap d'allocation.

**Pourquoi** :
- `EXT4` est le meilleur premier candidat de la phase suivante pour etendre la credibilite Linux du produit ;
- le produit couvre deja `NTFS`, `FAT32` et `exFAT`, donc `EXT4` est le prochain vrai palier fonctionnel cote filesystems ;
- un MVP conservative sur inodes orphelins fait progresser le moteur sans promettre de journal replay ni de recuperation de noms magiques.

**Périmètre** :
- couvert : nouvel analyseur backend `EXT4`, lecture du superblock et des group descriptors, inspection des bitmaps bloc/inode, support des inodes reguliers supprimes avec pointeurs directs ou extent tree profondeur 0, reconstruction conservative des seuls blocs encore libres, routage complet du workflow `carving` pour `EXT4`, contrats/app texts/tests mis a jour ;
- non couvert : replay du journal ext4, noms d'origine garantis, extent trees profonds, indirect blocks complexes au-dela du MVP, `EXT3/EXT2`, lost-volume `EXT4`, preview specialisee Linux ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/ext4.rs`, `src-tauri/src/commands/mod.rs`, UI `Diagnostic/Scan/Results/Export`, i18n, tests Rust ;
- hypothèses : le MVP peut retomber sur des noms bases sur l'inode et une extension inferee depuis le contenu quand le nom d'origine n'est plus disponible.

**Critères de validation** :
- un scan supprime `EXT4` peut produire au moins un resultat reconstruit depuis une image locale synthetique ;
- les blocs deja realloues coupent la reconstruction de maniere conservative ;
- l'UI n'annonce pas une recuperation journalisee ou des noms d'origine garantis ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 47 - Bundle de support local exportable

**Objectif** : renforcer le produit pour un usage commercial et supportable en ajoutant un bundle de support local exportable, contenant un manifeste build/runtime, les historiques locaux et les journaux techniques, mais jamais les contenus de fichiers recuperes ni les bytes source.

**Pourquoi** :
- un produit desktop commercialisable a besoin d'un artefact propre pour diagnostiquer les incidents utilisateur sans demander des manipulations manuelles fragiles ;
- le repo sait deja persister historique et logs, donc il manque surtout un assemblage securise et exportable ;
- ce chantier ameliore le support reel, la tracabilite et la preparation a la distribution sans pretendre resoudre a lui seul signature/notarisation.

**Périmètre** :
- couvert : commande backend `support bundle` en ZIP, manifeste JSON, export des historiques scan/export et de leurs logs, validation optionnelle de destination vis-a-vis d'une source active, action UI dans `Parametres`, textes de securite, tests Rust ;
- non couvert : upload cloud du bundle, anonymisation automatique avancee, crash reporting distant, signature notarisee du bundle, attestation cryptographique, auto-update ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/SettingsPage.tsx`, i18n, `zip` crate deja presente ;
- hypothèses : le bundle peut inclure des metadonnees et chemins techniques si l'utilisateur choisit explicitement de l'exporter, mais jamais les donnees source ni les fichiers recuperes eux-memes.

**Critères de validation** :
- l'utilisateur peut exporter un bundle ZIP local depuis `Parametres` ;
- le bundle contient au minimum un manifeste runtime/build et les historiques/logs disponibles ;
- aucune donnee de fichier recupere ou de disque source n'est materialisee dans le bundle ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 48 - Validation post-carving et integrite honnete

**Objectif** : rendre le carving par signatures plus credible en ajoutant une validation post-reconstruction par format, afin de distinguer les candidats vraiment lisibles des candidats seulement contigus mais deja structurellement corrompus.

**Pourquoi** :
- le moteur de carving actuel classe surtout `intact` si un footer est trouve, ce qui est trop optimiste pour un produit serieux ;
- une validation legere sur `PNG`, `PDF` et `ZIP` augmente la valeur produit sans promettre de reconstruction magique ;
- ce chantier fait progresser les cas recovery avances avec un benefice utilisateur direct dans `Resultats` et `Export`.

**Périmètre** :
- couvert : validation post-carving par format, nouveaux statuts d'integrite si la structure est incoherente, rescoring conservative, tests Rust sur cas valides et corrompus, microcopies UI du carving mises a jour si necessaire ;
- non couvert : carving non contigu robuste, regroupement intelligent de fragments, couverture massive de signatures supplementaires, validation binaire exhaustive de tous les formats ;
- dépendances : `PLANS.md`, `src-tauri/src/carving/mod.rs`, `src-tauri/src/commands/mod.rs`, i18n eventuelle, tests Rust ;
- hypothèses : on reste strictement local et read-only, et un fichier contigu mais non validable peut etre marque `corrupt` sans empecher son export si l'utilisateur le souhaite.

**Critères de validation** :
- un carving `ZIP` syntaxiquement invalide n'est plus presente comme `intact` ;
- un carving `PNG` avec CRC ou structure invalide n'est plus presente comme `intact` ;
- un carving `PDF` sans marqueurs de fin cohérents est degrade proprement ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 49 - HFS+ visible-volume MVP

**Objectif** : ouvrir la phase suivante filesystems avec un premier MVP `HFS+` honnete, capable de detecter un volume HFS+ perdu de maniere conservative puis d'en cataloguer les fichiers encore visibles depuis un slice local read-only.

**Pourquoi** :
- `HFS+` est le prochain palier utile cote ecosysteme Apple avant d'attaquer `APFS` ;
- un workflow visible-only depuis slice local apporte une vraie valeur produit sans mentir sur la recuperation supprimee HFS+ ;
- ce chantier renforce le mode expert et la recuperation de volumes perdus avec une base parser reusable pour la suite.

**Périmètre** :
- couvert : detection conservative de volume `HFS+` / `HFSX` depuis volume header direct ou HFS wrapper, nouvel analyseur `HFS+` pour le catalog file et les fichiers visibles, support du workflow `lost-volume` pour `HFS+`, ajustements Diagnostic/Expert/Results/Export et tests Rust/Vitest associes ;
- non couvert : recuperation supprimee `HFS+`, extents overflow du catalog file, resource forks, journal HFS+, HFS classique pur, `APFS` ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/hfsplus.rs`, `src-tauri/src/partitioning/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/utils/potentialVolumeRecovery.ts`, i18n, tests Rust/Vitest ;
- hypothèses : tant qu'un fichier `HFS+` visible ne tient pas dans les extents inline connus ou qu'un fork complexe depasse le MVP, on reste conservative et on n'annonce pas une recuperation complete.

**Critères de validation** :
- un volume `HFS+` brut plausible est detecte comme candidat de volume perdu ;
- le workflow `lost-volume` peut cataloguer au moins un fichier visible `HFS+` depuis un slice local ;
- l'UI n'annonce pas de recuperation supprimee `HFS+` ;
- `npm run test`, `npm run build` et `cargo check` passent.

## Chantier 50 - Smoke tests E2E du shell desktop

**Objectif** : ajouter une premiere couche de tests E2E reproductibles sur le shell desktop/web, afin de securiser la navigation critique, le fallback hors runtime Tauri et les bascules de mode/langue avant de poursuivre les chantiers distribution et filesystems.

**Pourquoi** :
- le produit a maintenant beaucoup de surface UI et plusieurs regressions recentes ont touche le lancement et la navigation ;
- un smoke test sur le shell, les reglages et les garde-fous hors Tauri augmente la confiance sans attendre un harness desktop natif complet ;
- ce chantier fait progresser le durcissement commercial/distribution avec un vrai filet de securite automatisable.

**Périmètre** :
- couvert : integration de Playwright, script dedie `test:e2e`, config de serveur web local, attributs `data-testid` stables sur la navigation et les reglages, smoke test sur accueil -> parametres -> bundle support hors Tauri -> mode expert -> langue francaise -> retour accueil ;
- non couvert : automation Tauri native, packaging signe/notarise, helper privilegie industrialise, scenarios recovery lourds sur peripheriques reels ;
- dépendances : `PLANS.md`, `package.json`, `.gitignore`, `playwright.config.ts`, `e2e/`, composants de navigation et `SettingsPage` ;
- hypothèses : le shell React reste testable proprement en mode navigateur, et le fallback hors Tauri doit etre explicitement valide plutot que contourne.

**Critères de validation** :
- `npm run test:e2e` lance un smoke test qui ouvre l'application et verifie la navigation critique ;
- le test verifie qu'un export de bundle support hors Tauri echoue proprement avec un message pedagogique ;
- le test verifie que le mode expert et le changement de langue restent fonctionnels ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent.

## Chantier 51 - Decoupage du shell par routes

**Objectif** : reduire le poids du bundle initial desktop en chargeant les ecrans lourds a la demande, afin d'ameliorer le demarrage, la navigation initiale et la qualite de distribution du shell.

**Pourquoi** :
- le build actuel emet encore un warning Vite sur un chunk principal > 500 kB ;
- les pages `Resultats`, `Expert`, `Historique` et `Export` embarquent beaucoup de logique qui n'a pas besoin d'etre chargee sur l'accueil ;
- un decoupage par route apporte un gain concret et mesurable sans modifier le moteur recovery.

**Périmètre** :
- couvert : lazy loading des pages du router, fallback de chargement coherente, conservation des tests existants, verification du build apres decoupage ;
- non couvert : code splitting ultra-fin intra-page, refactor complet des composants communs, optimisation des analyzers backend ;
- dépendances : `PLANS.md`, `src/router.tsx`, composant de chargement commun, tests web/E2E ;
- hypothèses : `react-router-dom` et le shell actuel supportent un fallback `Suspense` sans regression UX majeure.

**Critères de validation** :
- le bundle initial baisse et le warning Vite sur le chunk principal disparait ou se reduit nettement ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent ;
- la navigation smoke test continue de passer avec les routes lazy.

## Chantier 52 - Detection conservative APFS

**Objectif** : ouvrir une premiere tranche `APFS` honnete en detectant les containers `APFS` plausibles pendant l'inspection de volumes potentiels, puis en exposant clairement cette detection dans le diagnostic et le mode expert sans pretendre cataloguer ou recuperer automatiquement des fichiers `APFS`.

**Pourquoi** :
- `APFS` reste le gros manque cote ecosysteme Apple ;
- une detection conservative de container apporte une vraie valeur produit pour les cas de partition perdue ou de source brute ambiguë ;
- ce chantier fait progresser le support `APFS` sans vendre une recuperation supprimée ou un browseur de fichiers qui n'existent pas encore.

**Périmètre** :
- couvert : detection de superblock `APFS` (`NXSB`) et indice GPT `APFS`, enrichissement des notes candidates, limitations/recommandations diagnostic explicites pour `APFS`, tests Rust/Vitest associes, microcopy capability si necessaire ;
- non couvert : catalogage de fichiers `APFS`, recuperation supprimee `APFS`, snapshots, arbres B-tree `APFS`, chiffrement `APFS`, fusion/split de containers ;
- dépendances : `PLANS.md`, `src-tauri/src/partitioning/mod.rs`, `src-tauri/src/commands/mod.rs`, i18n diagnostic, tests Rust ;
- hypothèses : une detection conservative de container `APFS` est deja utile meme si l'analyse de son contenu reste un chantier distinct.

**Critères de validation** :
- un container `APFS` plausible est detecte comme candidat de volume potentiel ;
- le diagnostic mentionne explicitement que `APFS` est detecte mais pas encore analysable automatiquement ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent.

## Chantier 53 - Identite de build et rapport d'environnement

**Objectif** : rendre la distribution et le support plus credibles en exposant un vrai rapport `build/runtime` depuis le backend, reutilise par `Parametres` et par le support bundle.

**Pourquoi** :
- aujourd'hui `Parametres` affiche une version statique et masque l'identite reelle du build ;
- le support bundle contient deja un manifeste, mais il ne porte pas encore une surface claire et partagee de build/runtime exploitable par le support ;
- ce chantier fait progresser l'industrialisation sans attendre la signature/notarisation finale.

**Périmètre** :
- couvert : nouveau contrat partage `AppBuildInfo`, commande IPC dediee, fallback frontend hors runtime Tauri pour la version, affichage structure dans `Parametres`, manifeste du support bundle enrichi, tests Rust et smoke web inchanges ;
- non couvert : pipeline de release signe/notarise, auto-update, signature du helper privilegie, telemetrie, packaging CI/CD complet ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/SettingsPage.tsx`, `vite.config.ts`, i18n ;
- hypothèses : une identite de build fiable suffit a ameliorer le support local meme si le packaging signe reste un chantier distinct.

**Critères de validation** :
- `Parametres` affiche un vrai rapport build/runtime quand le backend est disponible ;
- le support bundle contient toujours un manifeste JSON valide enrichi avec cette identite de build ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent.

## Chantier 54 - APFS visible-volume MVP

**Objectif** : passer `APFS` du simple statut "container detecte" a un vrai MVP de catalogage visible-only sur slice local, afin d'analyser un volume retrouve `APFS` en lecture seule quand son catalogue est effectivement lisible.

**Pourquoi** :
- `APFS` reste le plus gros manque fonctionnel sur les cas Apple alors qu'on sait deja detecter des containers plausibles ;
- un browseur visible-only est deja utile pour valider un volume retrouve, previsualiser/exporter des fichiers encore lisibles, et reduire le delta produit sur macOS ;
- ce chantier fait progresser le produit sans promettre une recuperation supprimee `APFS` qui reste de la R&D distincte.

**Périmètre** :
- couvert : nouvel analyzer `APFS` read-only pour le premier volume d'un container, catalogage recursive des fichiers visibles, derivation conservative des extents en `byte_runs` pour preview/export, branchement dans le workflow `lost-volume`, activation UI de l'analyse `APFS`, tests backend associes ;
- non couvert : recuperation supprimee `APFS`, snapshots, chiffrement `APFS`, clones/compression avances, multi-volume complexe, resource forks, support des volumes sans extents exploitables ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/`, `Cargo.toml`, `src-tauri/src/commands/mod.rs`, `src/utils/potentialVolumeRecovery.ts`, i18n diagnostic/capability ;
- hypothèses : un crate Rust read-only `APFS` suffisamment stable permet un MVP credible pour les fichiers visibles, et les tests macOS peuvent generer une image brute APFS locale.

**Critères de validation** :
- un candidat `APFS` peut etre lance depuis le workflow `lost-volume` ;
- le scan catalogue des fichiers visibles `APFS` depuis un slice local avec `byte_runs` utilisables par preview/export ;
- le wording reste honnete sur l'absence de recuperation supprimee `APFS` ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent.

## Chantier 55 - Release preflight et hygiene de packaging

**Objectif** : ajouter un vrai preflight de release local pour rendre la distribution plus fiable, verifier les metadonnees critiques avant build, et documenter les limites restantes cote signature/notarisation.

**Pourquoi** :
- le repo peut deja produire un bundle Tauri, mais il manque un garde-fou explicite avant une release ;
- certaines metadonnees restent trop generiques cote Rust et nuisent a la credibilite packaging ;
- on ne peut pas finaliser une signature/notarisation Apple sans pipeline externe ni certificats, mais on peut verifier tout le prealable local et rendre les manques visibles.

**Périmètre** :
- couvert : script `release-preflight`, verification des versions `package.json` / `tauri.conf.json` / `Cargo.toml`, verification du bundle id, des icones Tauri et des metadonnees package, rapport JSON local, scripts npm associes, README mise a jour, nettoyage des placeholders Cargo ;
- non couvert : signature effective, notarisation Apple, helper privilegie signe, CI/CD de publication, auto-update distribue ;
- dépendances : `PLANS.md`, `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `README.md`, nouveau script Node de preflight ;
- hypothèses : un preflight local strict sur les blocages verifiables et souple sur les prerequis externes est le meilleur compromis pour industrialiser sans promettre un pipeline absent.

**Critères de validation** :
- `npm run release:preflight` produit un rapport lisible et echoue sur les vrais blocages locaux ;
- `npm run release:build` chaine le preflight puis le build Tauri ;
- `README.md` explique clairement la difference entre preflight local et signature/notarisation externes ;
- `npm run test`, `npm run test:e2e`, `npm run build` et `cargo check` passent.

## Chantier 56 - Automation CI et release macOS

**Objectif** : industrialiser la verification et la generation de bundles avec une base GitHub Actions credible, en separant clairement la verification locale/CI de la signature-notarisation qui depend encore de secrets externes.

**Pourquoi** :
- le repo sait deja construire localement un bundle, mais aucun workflow versionne ne documente ou n'automatise ce chemin ;
- les tests `APFS` et le packaging macOS rendent une CI naive multi-OS trompeuse ;
- un pipeline de verification + un workflow de bundle manuel ferment une grosse partie du trou "distribution industrialisable" sans vendre une release notarisee deja finie.

**Périmètre** :
- couvert : workflow CI macOS pour `release:preflight` + tests + build + smoke E2E, workflow de bundle release macOS manuel/tagge, upload d'artefacts `.app` / `.dmg`, documentation du contrat de secrets, nettoyage `.gitignore` pour les artefacts Tauri ;
- non couvert : provisioning des secrets Apple, notarisation effective, publication GitHub Release automatisee, update feed signe, pipeline Windows/Linux complet ;
- dépendances : `PLANS.md`, `.github/workflows/`, `README.md`, `.gitignore`, documentation des secrets release ;
- hypothèses : une CI macOS unique est le bon compromis actuel parce que certaines validations `APFS` sont specifiques a cet environnement et qu'un bundle macOS est la cible packaging la plus mature du repo a ce stade.

**Critères de validation** :
- un workflow versionne verifie le repo sur macOS avec `release:preflight`, `test`, `build`, `cargo check` et `test:e2e` ;
- un workflow versionne peut produire les bundles macOS et publier les artefacts dans GitHub Actions ;
- la documentation explique quels secrets externes seront encore necessaires pour signer/notariser ;
- `npm run release:preflight`, `npm run build` et `npm run test` passent toujours localement.

## Chantier 57 - Manifeste de release et publication GitHub

**Objectif** : transformer le bundle macOS en vraie release publiable avec manifeste, checksums et publication GitHub automatisee sur tag, sans masquer que la signature/notarisation restent conditionnelles.

**Pourquoi** :
- les artefacts existent deja, mais il manque encore une surface versionnee et publiable exploitable par un vrai flux release ;
- checksums et manifeste de release sont des briques utiles pour le support, la verification integrite et une future couche d'update ;
- ce chantier ferme une partie concrete du gap "commercialisable" sans raconter qu'une notarisation Apple est deja operationnelle.

**Périmètre** :
- couvert : script de generation du manifeste/checksums de release a partir des bundles locaux, scripts npm associes, workflow GitHub Release sur tags avec upload du `.dmg`, du manifeste et des checksums, documentation du comportement ;
- non couvert : feed updater Tauri complet, delta updates, notarisation effective, changelog automatique sophistique, publication multiplateforme ;
- dépendances : `PLANS.md`, `package.json`, `scripts/`, `.github/workflows/release-macos.yml`, `README.md`, `.github/RELEASE_SECRETS.md` ;
- hypothèses : un manifeste JSON + SHA-256 et une release GitHub automatisee sont le bon jalon intermediaire avant la vraie distribution signee.

**Critères de validation** :
- un script local genere un manifeste et des checksums coherents pour les bundles macOS produits ;
- le workflow de release peut publier les artefacts sur un tag GitHub ;
- `npm run release:manifest`, `npm run release:preflight`, `npm run build` et `npm run test` passent localement.

## Chantier 58 - Advisory IA orientee resultats et CI multiplateforme

**Objectif** : faire passer la couche IA d'un diagnostic support-only a une advisory de recuperation exploitable sur les resultats reels, puis etendre la verification CI au-dela de macOS avec une base multiplateforme honnete.

**Pourquoi** :
- l'IA actuelle aide a choisir un workflow, mais n'analyse pas encore la qualite concrete des fichiers retrouves, ce qui limite sa valeur produit ;
- le brief initial attend une aide strategique sur l'integrite probable, la priorisation et les limites des cas complexes ;
- le repo dispose d'une automation macOS solide, mais la finition multiplateforme reste incomplete sans une verification Windows/Linux versionnee.

**Périmètre** :
- couvert : nouveau contrat d'advisory IA sur scan/resultats, commande IPC dediee, affichage `ResultsPage`, synthese des lots export-now/review-first/unstable et des signaux complexes (partial/fragmented/carved/errors), tests Rust associes, CI Windows/Linux legere (`build`, `test:ui`, `cargo check`), release macOS mise a jour pour accepter des secrets de signature/notarisation optionnels ;
- non couvert : vrai modele cloud, regroupement automatique de fragments binaires, signature Windows effective, bundle Linux publie, recuperation supprimee APFS/HFS+ ;
- dépendances : `PLANS.md`, `src-tauri/src/ai/mod.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/types/ai.ts`, `src/hooks/useIpc.ts`, `src/pages/ResultsPage.tsx`, `src/components/results/`, i18n, `.github/workflows/` ;
- hypothèses : une advisory resultats purement locale et explicable apporte une vraie valeur produit immediate, et une CI multiplateforme legere est le meilleur compromis avant des releases Windows/Linux completes.

**Critères de validation** :
- `ResultsPage` affiche une synthese IA locale des resultats du scan courant ;
- l'advisory distingue explicitement export immediat, revue manuelle et cas instables/complexes ;
- les workflows versionnes couvrent macOS, Windows et Linux avec des etapes realistes pour chaque plateforme ;
- `npm run test`, `npm run release:preflight`, `npm run build` et `cargo check` passent.

## Chantier 59 - Recuperation ext4 sur extent trees internes

**Objectif** : etendre le MVP de recuperation supprimee `ext4` pour lire des extent trees non inline (depth > 0) au lieu de s'arreter aux seuls extent leaves stockes directement dans l'inode.

**Pourquoi** :
- le moteur `ext4` actuel ne couvre que les petits fichiers ou les arbres d'extents tres simples, ce qui limite fortement les cas de suppression reels ;
- les extent trees internes sont un vrai palier "recovery complexe" sans sortir du cadre read-only ni promettre un replay du journal ;
- cette tranche augmente la couverture produit sur `ext4` avec un risque technique contenu et testable localement.

**Périmètre** :
- couvert : parsing recursif des extent trees `ext4`, lecture conservative des noeuds internes et feuilles depuis l'image locale, garde-fous de profondeur/entrees invalides, fixtures synthetiques depth-1, tests analyseur et integration scan ;
- non couvert : replay du journal `ext4`, noms d'origine, indirect blocks classiques non extents, extent trees profondement corrompus, regroupement automatique de fragments au-dela des pointeurs d'extent valides ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/ext4.rs`, tests Rust dans l'analyseur et `commands/mod.rs` ;
- hypothèses : supporter correctement les extent trees internes depth-1/depth-n raisonnables apporte de vrais gains sur les suppressions `ext4` modernes, tout en restant explicable et robuste.

**Critères de validation** :
- l'analyseur `ext4` recupere correctement un inode supprime dont la racine d'extents pointe vers un bloc feuille externe ;
- le flux de scan `scan-deleted-ext4` continue a produire des resultats/exportables sur ce type de fixture ;
- `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 60 - Carving fragmente conservative et HFS+ supprime MVP

**Objectif** : ajouter une reconstruction de carving conservative sur candidats a un seul gap supprimable, puis ouvrir un MVP de recuperation supprimee `HFS+` via records catalogue residuels encore lisibles dans les leaf nodes.

**Pourquoi** :
- le carving actuel ne sait reconstruire que des plages contigues, ce qui manque une partie des cas simples de fragmentation ou de zones neutres intercalees ;
- `HFS+` est encore limite au visible-only alors qu'un MVP de records catalogue residuels apporte une vraie progression sur les cas macOS legacy ;
- ces deux chantiers avancent concretement sur les "cas recovery complexes" sans promettre du journal replay, du carving arbitraire multi-fragments ou une resurrection magique.

**Périmètre** :
- couvert : heuristique de carving a un seul gap supprimable avec `byte_runs` multiples et validation structurelle, tests dedies ; analyseur `HFS+` supprimé base sur slack/records residuels dans les leaf nodes, wiring direct `scan-deleted-hfsplus`, wiring `lost-volume` HFS+ visible+supprime, i18n/UI associes, tests analyseur + integration ;
- non couvert : carving multi-gaps robuste, regroupement automatique de fragments arbitraires, replay du journal `HFS+`, forks ressources, overflow extents `HFS+`, recuperation supprimee `APFS` ;
- dépendances : `PLANS.md`, `src-tauri/src/carving/mod.rs`, `src-tauri/src/analyzers/hfsplus.rs`, `src-tauri/src/commands/mod.rs`, pages UI et fichiers i18n/types associes ;
- hypothèses : un carving "1 gap supprimable" valide structurellement apporte un vrai gain sans faire croire a un moteur complet de regroupement, et un MVP `HFS+` sur records catalogue residuels est le bon palier avant tout travail `APFS` supprime.

**Critères de validation** :
- le carving peut reconstruire au moins un candidat structurellement valide a partir de deux `byte_runs` separes par un gap neutre ;
- un scan `scan-deleted-hfsplus` produit des resultats supprimés exploitables depuis une image locale ;
- un `lost-volume` `HFS+` remonte visible + supprimé au lieu du seul visible ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 61 - APFS supprime conservatif et carving multi-gaps borne

**Objectif** : ouvrir un MVP de recuperation supprimee `APFS` base sur des inodes/forks encore presents mais non references dans le catalogue courant, puis etendre le carving par signatures pour supprimer de maniere conservative jusqu'a deux gaps neutres quand cela redonne un fichier structurellement valide.

**Pourquoi** :
- `APFS` reste encore limite au visible-only alors que certains cas de suppressions recentes ou d'objets orphelins peuvent etre exploites sans journal replay ni chiffrement ;
- le carving "1 gap" a deja prouve son utilite, mais rate encore des cas simples a trois segments ;
- ces deux tranches font progresser les cas recovery complexes en restant tres explicites sur ce qui n'est pas couvert.

**Périmètre** :
- couvert : scan `APFS` read-only du catalogue courant pour retrouver des fichiers reguliers encore reconstructibles via inode + extents mais sans `dir record` actif, noms synthetiques si le nom d'origine n'est plus fiable, wiring direct `scan-deleted-apfs`, wiring `lost-volume` `APFS` visible+supprime quand de tels candidats existent, heuristique de carving a un ou deux gaps neutres supprimables avec validation structurelle, tests backend et microcopies associees ;
- non couvert : snapshots `APFS`, journal replay, chiffrement `APFS`, clones/compression avances, recuperation supprimee exhaustive `APFS`, carving arbitraire multi-fragments, regroupement intelligent general de fragments ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/apfs.rs`, `src-tauri/src/carving/mod.rs`, `src-tauri/src/commands/mod.rs`, pages UI, types partages et i18n ;
- hypothèses : un MVP `APFS` limite aux objets encore presents dans le catalogue courant est suffisamment honnête pour un premier palier supprimé, et deux gaps neutres couvrent un sous-ensemble utile du carving fragmente sans pretendre a une reassemblage general.

**Critères de validation** :
- l'analyseur `APFS` peut remonter au moins un fichier supprime conservatif depuis un fixture/test dedie quand son inode/extents restent exploitables ;
- un scan `scan-deleted-apfs` produit des resultats/exportables depuis une image locale, et `lost-volume` `APFS` peut afficher visible + supprimé quand applicable ;
- le carving peut maintenant reconstruire au moins un candidat structurellement valide a partir de trois segments et deux gaps neutres ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 62 - NTFS sparse runs et fidelite exFAT fragmente

**Objectif** : etendre la recuperation `NTFS` non-residente aux runlists contenant des plages sparses logiques, puis corriger la fidelite produit des suppressions `exFAT` reconstruites via une chaine FAT fragmentee.

**Pourquoi** :
- certains fichiers `NTFS` restent completement reconstructibles meme quand une partie logique de leur runlist est sparse ; aujourd'hui le parseur ecartait encore ces cas ;
- le moteur `exFAT` sait deja suivre une chaine FAT supprimee valide, mais il surestime encore certains cas en les marquant `intact` alors qu'ils sont physiquement fragmentes ;
- ces deux tranches apportent une vraie progression sur les cas recovery complexes sans inventer d'octets inconnus ni promettre un reassemblage arbitraire.

**Périmètre** :
- couvert : support des runs `NTFS` sparses avec segments zero-fill explicites dans le modele interne, lecture/preview/export depuis image avec preservation des trous logiques, fixture synthetique `NTFS` sparse supprimee, tests analyseur + integration scan/export ; correction `exFAT` pour marquer `fragmented` une suppression reconstruite entierement via une chaine FAT multi-runs, avec fixture/test associes ;
- non couvert : trous logiques generiques pour tous les filesystems, compression/chiffrement `NTFS`, ADS nommes, reassemblage `exFAT` apres clusters reellement inconnus, multi-gap recovery arbitraire ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/analyzers/ntfs.rs`, `src-tauri/src/analyzers/exfat.rs`, `src-tauri/src/commands/mod.rs` ;
- hypothèses : modeliser explicitement les zero-fills `NTFS` reste honnete parce qu'il s'agit de trous logiques declares par le runlist, et la correction d'integrite `exFAT` est un ajustement produit/backend a faible risque.

**Critères de validation** :
- un fichier `NTFS` supprime base sur un runlist sparse peut etre scanne, previsualise et exporte correctement avec son trou logique preserve ;
- un fichier `exFAT` supprime reconstruit entierement via une chaine FAT non contigue est marque `fragmented` et non `intact` ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 63 - HFS+ extents-overflow data fork MVP

**Objectif** : etendre l'analyse `HFS+` visible/supprimee pour suivre de maniere conservative les extents additionnels du data fork via le fichier d'extents overflow.

**Pourquoi** :
- le moteur `HFS+` savait deja parser les catalog records et le slack, mais s'arretait aux huit extents inline du fork ;
- cela excluait des fichiers reels pourtant encore reconstructibles en lecture seule sans journal replay ;
- c'est une vraie progression sur les cas macOS legacy, plus utile maintenant qu'un nouveau vernis UI.

**Périmètre** :
- couvert : parsing du fork `extentsFile` depuis le volume header, lecture conservative de son B-tree leaf, resolution sequentielle des records d'extents overflow pour le data fork, application a la lecture du catalog file et des data forks visibles/supprimes, fixtures HFS+ overflow, tests analyseur, realignement des limitations/microcopies ;
- non couvert : resource forks, journal replay `HFS+`, extents overflow du fichier d'extents lui-meme, corruption lourde du B-tree d'overflow ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/hfsplus.rs`, `src-tauri/src/commands/mod.rs`, i18n ;
- hypothèses : suivre les extents overflow du data fork seulement apporte un vrai gain produit, tout en restant explicable et borné.

**Critères de validation** :
- un fichier visible `HFS+` dont le data fork depasse huit extents inline reste lisible via l'overflow file ;
- un fichier supprime `HFS+` base sur un catalog record residuel peut etre reconstruit quand ses extents additionnels sont resolvables dans l'overflow file ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 64 - HFS+ resource fork sidecar MVP

**Objectif** : etendre le flux `HFS+` visible/supprime pour detecter un resource fork reconstructible, le transporter jusqu'aux resultats, puis l'exporter de maniere explicite sous forme de sidecar brut.

**Pourquoi** :
- l'etape precedente a debloque les extents du data fork, mais le produit continuait de jeter silencieusement les resource forks HFS+ ;
- meme si tous les systemes de destination ne savent pas les remonter nativement, exporter leurs octets sous forme de sidecar brut apporte une vraie valeur aux utilisateurs experts ;
- ce cadrage reste honnete : on preserve les octets du fork ressource sans pretendre recreer automatiquement toute la semantique HFS+ ou Finder.

**Périmètre** :
- couvert : parsing du resource fork dans les catalog records visibles/supprimes, reconstruction conservative via extents inline + overflow file, contrat partage Rust/TypeScript pour signaler la presence d'un resource fork, export d'un sidecar brut `nom.resource-fork.bin`, logs et microcopies dedies, tests analyseur + integration export ;
- non couvert : AppleDouble complet `._nom`, Finder metadata, journal replay `HFS+`, resource forks hors extents resolvables, preview dedie du fork ressource ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/analyzers/hfsplus.rs`, `src-tauri/src/commands/mod.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, i18n ;
- hypothèses : un sidecar brut est preferable a un faux fichier `._nom` incomplet, parce qu'il expose clairement des octets preservables sans survendre une restauration native parfaite.

**Critères de validation** :
- un fichier `HFS+` visible ou supprime avec resource fork reconstructible remonte cette information dans les resultats ;
- l'export ecrit le fichier principal plus un sidecar `nom.resource-fork.bin` quand un resource fork existe ;
- les messages produit disent explicitement qu'il s'agit d'un fork ressource brut et non d'une recreation complete de la semantique HFS+ ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml` et `npm run build` passent.

## Chantier 65 - NTFS named ADS sidecar MVP

**Objectif** : etendre le flux `NTFS` visible/supprime pour detecter des attributs `DATA` nommes simples, les transporter jusqu'aux resultats, puis les exporter sous forme de sidecars bruts cross-platform.

**Pourquoi** :
- le MVP `NTFS` reconstruisait deja le flux de donnees principal, mais jetait silencieusement les flux `ADS` nommes ;
- ces flux peuvent contenir des metadonnees ou des donnees utiles, surtout sur des cas Windows forensics ou bureautiques ;
- un sidecar brut `nom.ads.<stream>.bin` reste honnete, preservable sur tous les OS de destination et n'invente pas une restauration native parfaite des semantiques NTFS.

**Périmètre** :
- couvert : parsing des attributs `DATA` nommes resident ou non-resident simples (`lowest_vcn = 0`), exposition dans les resultats `NTFS` visibles/supprimes quand un flux principal existe deja, transport via les contrats partages, export en sidecars bruts `nom.ads.<stream>.bin`, logs et microcopies dedies, fixtures/tests analyseur + integration export ;
- non couvert : fichiers `ADS-only` sans flux principal, ADS multi-attributs complexes `lowest_vcn > 0`, compression/chiffrement `NTFS`, recreation native de streams sur la destination, preview dedie des ADS ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/analyzers/ntfs.rs`, `src-tauri/src/commands/mod.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/pages/ResultsPage.tsx`, `src/components/results/FileTreePanel.tsx`, `src/pages/ExportPage.tsx`, i18n ;
- hypothèses : exporter les ADS en sidecars nommes est preferable a leur perte silencieuse et plus honnete qu'une fausse recreation transparente des streams NTFS hors volume NTFS.

**Critères de validation** :
- un fichier `NTFS` visible ou supprime avec ADS nomme reconstructible remonte cette information dans les resultats ;
- l'export ecrit le fichier principal plus un sidecar `nom.ads.<stream>.bin` pour chaque ADS reconstructible ;
- les messages produit disent explicitement qu'il s'agit de sidecars ADS bruts et non d'une recreation native de streams NTFS ;
- `npm run test`, `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build` et `npm run test:e2e` passent.

## Chantier 66 - Inspection hex des forks auxiliaires MVP

**Objectif** : permettre au mode expert d'inspecter localement, avant export, les octets d'un resource fork `HFS+` ou d'un flux alterne `NTFS ADS`, au lieu de rendre ces donnees seulement exportables en sidecars.

**Pourquoi** :
- le produit sait maintenant preserv<|vq_16408|>er ces flux auxiliaires a l'export, mais l'utilisateur expert ne peut pas encore les verifier dans l'application ;
- une visionneuse hex read-only locale apporte une vraie valeur forensic sans promettre de preview riche ou de recreation native des semantiques filesystem ;
- c'est un prolongement naturel du travail sidecars `HFS+` et `NTFS`, avec peu de surface supplementaire et une validation claire.

**Périmètre** :
- couvert : nouvelle commande backend read-only pour lire une fenetre hex depuis le data fork principal, un resource fork `HFS+` ou un flux alterne `NTFS ADS` reconstructible ; wiring TypeScript ; selecteur de cible dans `ExpertPage` / `HexViewerPanel` ; microcopies expert associees ; tests backend sur preview hex d'un resource fork `HFS+` et d'un ADS `NTFS` ;
- non couvert : preview texte/image dediee des forks auxiliaires, recreation native `AppleDouble` ou ADS NTFS, inspection arbitraire de secteurs disque, edition des flux ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/ExpertPage.tsx`, `src/components/expert/HexViewerPanel.tsx`, i18n ;
- hypothèses : une inspection hex locale suffit pour un premier palier expert, et les forks auxiliaires restent accessibles uniquement quand ils proviennent d'une image locale reconstructible.

**Critères de validation** :
- le mode expert permet de choisir la cible `primary/resource fork/ADS` quand elle existe ;
- la preview hex d'un resource fork `HFS+` et d'un ADS `NTFS` lit bien les octets attendus ;
- l'interface reste explicite sur le fait qu'il s'agit d'une inspection read-only de flux auxiliaires ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 67 - Preview texte conservatif des forks auxiliaires

**Objectif** : permettre au mode expert d'afficher un preview texte local des resource forks `HFS+` et des `ADS` `NTFS` quand leur contenu ressemble a du texte lisible, avec un fallback explicite vers l'hex quand le contenu reste binaire.

**Pourquoi** :
- l'inspection hex des flux auxiliaires est utile, mais trop basse-niveau pour des cas simples comme `Zone.Identifier` ou certains forks ressources textuels ;
- un preview texte local read-only apporte une vraie valeur expert sans promettre un rendu riche ni une interpretation complete des semantiques filesystem ;
- une detection conservative "text-like ou non" evite de montrer du bruit binaire comme si c'etait un vrai preview.

**Périmètre** :
- couvert : helper backend de detection conservative de contenu text-like depuis image locale, commande IPC dediee pour preview d'un resource fork `HFS+` ou d'un ADS `NTFS`, wiring `ExpertPage` et panneau de preview associe, microcopies EN/FR, tests backend ;
- non couvert : preview image/pdf/doc dedie des forks auxiliaires, recreation native AppleDouble/ADS, interpretation semantique des forks ressources, edition ou export differencie ;
- dépendances : `PLANS.md`, `src-tauri/src/preview/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/ExpertPage.tsx`, `src/components/results/FilePreviewPanel.tsx`, i18n ;
- hypothèses : les previews textuels utiles couvrent deja une partie importante des flux auxiliaires experts, et un fallback propre vers l'hex est preferable a une tentative de rendu trop ambitieuse.

**Critères de validation** :
- un ADS `NTFS` textuel et un resource fork `HFS+` textuel peuvent etre previsualises localement dans `Mode expert` ;
- un flux auxiliaire trop binaire retourne un etat indisponible explicite plutot qu'un faux preview ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 68 - Preview asset conservatif des forks auxiliaires

**Objectif** : etendre le preview expert des flux auxiliaires pour materaliser localement les cas clairement reconnaissables comme `JPEG/PNG/GIF/WEBP/PDF`, au lieu de tomber systematiquement sur un etat indisponible apres l'echec du preview texte.

**Pourquoi** :
- certains flux auxiliaires utiles ne sont pas textuels mais restent clairement visualisables via leur signature binaire ;
- un rendu conservatif par signatures simples apporte une vraie valeur sans promettre une interpretation semantique complete des forks ou des ADS ;
- cela prolonge proprement le palier precedent en gardant un fallback explicite vers l'hex pour tout le reste.

**Périmètre** :
- couvert : sniff conservatif de signatures `JPEG/PNG/GIF/WEBP/PDF` pour flux auxiliaires, materialisation locale read-only si le type est reconnu, wiring du panneau de preview expert avec URLs asset Tauri, tests backend dedies ;
- non couvert : detection generique de formats arbitraires, preview `DOCX/XLSX` des flux auxiliaires, recreation native AppleDouble/ADS, preview riche de donnees binaires non reconnues ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src/pages/ExpertPage.tsx`, `src/hooks/useIpc.ts`, `src/components/results/FilePreviewPanel.tsx` ;
- hypothèses : un sniff par signatures simples reste suffisamment explicite et peu risqué pour un preview expert local.

**Critères de validation** :
- un flux auxiliaire image ou PDF clairement reconnu peut etre previsualise localement dans `Mode expert` ;
- un flux non reconnu continue d'afficher un message explicite et pousse vers l'hex/export brut ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 69 - Export direct des flux auxiliaires depuis le mode expert

**Objectif** : permettre au `Mode expert` d'enregistrer directement, avec un chemin choisi nativement, le payload brut d'un `resource fork` `HFS+` ou d'un `ADS` `NTFS`, sans repasser par un export complet de fichier.

**Pourquoi** :
- le produit sait deja previsualiser et exporter ces flux auxiliaires comme sidecars pendant un export complet, mais l'utilisateur expert ne peut pas encore sauvegarder uniquement le flux qu'il inspecte ;
- un export direct local read-only accelere les usages forensic et support sans melanger cela avec l'export du fichier principal ;
- ce palier reste honnete : on sauvegarde les octets bruts du flux auxiliaire, pas une recreation native complete de la semantique filesystem.

**Périmètre** :
- couvert : commande backend read-only pour sauver un `resource fork` `HFS+` ou un `ADS` `NTFS` reconstructible vers un chemin choisi, validation de destination contre le disque source quand l'information est disponible, wiring IPC TypeScript, bouton natif `Enregistrer` dans `ExpertPage`, messages UI associes, tests backend sur resource fork et ADS ;
- non couvert : recreation native `AppleDouble` ou ADS NTFS sur la destination, export de plusieurs flux auxiliaires en lot, preview semantique avancee des flux auxiliaires ;
- dépendances : `PLANS.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src/hooks/useIpc.ts`, `src/pages/ExpertPage.tsx`, i18n ;
- hypothèses : le payload auxiliaire peut etre materialise de maniere sure depuis l'image locale deja utilisee par les previews experts.

**Critères de validation** :
- un `resource fork` `HFS+` peut etre enregistre directement depuis le `Mode expert` vers un chemin choisi nativement ;
- un `ADS` `NTFS` peut etre enregistre directement depuis le `Mode expert` vers un chemin choisi nativement ;
- l'interface dit explicitement qu'il s'agit d'un payload auxiliaire brut et non d'une recreation native du filesystem ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 70 - Preview documentaire des flux auxiliaires

**Objectif** : permettre au `Mode expert` de previsualiser localement en texte utile un `resource fork` `HFS+` ou un `ADS` `NTFS` quand son payload brut correspond a un document `DOCX` ou `XLSX` valide.

**Pourquoi** :
- les flux auxiliaires experts supportent deja l'hex, le texte brut, les assets simples et l'export direct, mais un payload Office tombait encore inutilement sur un fallback bas-niveau ;
- reutiliser le parseur documentaire local existant augmente la valeur forensic sans promettre de rendu natif complet ni d'interpretation filesystem magique ;
- la detection par contenu ZIP valide est plus honnete qu'un faux heuristique base sur le nom du flux auxiliaire.

**Périmètre** :
- couvert : detection conservative `DOCX/XLSX` par contenu d'archive sur payload auxiliaire, preview texte local depuis image pour ces deux formats, integration dans le flow expert existant, tests backend dedies, realignement microcopy ;
- non couvert : preview `PPTX`, rendu riche Office, reconstitution native `ADS`/`resource fork`, interpretation semantique metier de documents annexes ;
- dépendances : `PLANS.md`, `src-tauri/src/preview/mod.rs`, `src-tauri/src/commands/mod.rs`, i18n ;
- hypothèses : la materialisation temporaire locale d'un payload auxiliaire dans le workspace preview reste acceptable car deja utilisee pour les assets experts existants.

**Critères de validation** :
- un `ADS` `NTFS` portant un `DOCX` valide peut etre previsualise en texte dans `Mode expert` ;
- un `resource fork` `HFS+` portant un `XLSX` valide peut etre previsualise en texte dans `Mode expert` ;
- un payload auxiliaire binaire non-Office garde le comportement existant sans faux positif ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 71 - Preview PPTX local et auxiliaire

**Objectif** : etendre le moteur documentaire local pour supporter `PPTX`, a la fois pour les fichiers principaux et pour les payloads auxiliaires experts quand leur archive correspond bien a une presentation Office.

**Pourquoi** :
- la couverture Office locale etait encore incomplete alors que `DOCX/XLSX` etaient deja pris en charge ;
- certains flux auxiliaires experts peuvent porter des presentations annexes utiles pour l'analyse, et il etait dommage de les rabattre inutilement vers l'hex ;
- un parseur `PPTX` textuel de slides apporte une vraie valeur sans promettre un rendu de mise en page natif.

**Périmètre** :
- couvert : detection conservative `PPTX` par contenu ZIP (`ppt/presentation.xml`, `ppt/slides/slide*.xml`), extraction textuelle simple des slides, integration dans le preview documentaire principal et auxiliaire, MIME `PPTX`, tests backend dedies ;
- non couvert : rendu visuel de diapositives, animations, notes de presentation, contenu multimedia embarque, `PPTX` corrompu ou protege, interpretation semantique avancee ;
- dépendances : `PLANS.md`, `src-tauri/src/preview/mod.rs`, `src-tauri/src/commands/mod.rs` ;
- hypothèses : un preview texte de slides est suffisant pour un premier palier local honnete et maintenable.

**Critères de validation** :
- un fichier principal `PPTX` peut etre previsualise localement sous forme textuelle ;
- un `ADS` `NTFS` ou un autre payload auxiliaire expert portant un `PPTX` valide peut etre detecte et previsualise localement ;
- un ZIP non-Office ne produit pas de faux positif `PPTX` ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 72 - Notes de presentation PPTX

**Objectif** : enrichir le preview `PPTX` local en exposant aussi le texte des `speaker notes` quand `notesSlides/notesSlide*.xml` existent et restent lisibles.

**Pourquoi** :
- le support `PPTX` textuel est deja utile, mais certaines presentations portent l'information critique dans les notes plutot que sur la slide visible ;
- lire ces notes reste un gain produit net sans promettre de rendu PowerPoint complet ;
- ce palier reste maintenable s'il se limite a un parsing conservatif des `notesSlides` presents, sans gestion des medias, themes ou animations.

**Périmètre** :
- couvert : lecture conservative des `notesSlides/notesSlide*.xml` associes aux slides `PPTX`, ajout du texte des notes au preview principal et auxiliaire, tri numerique des slides, tests backend dedies ;
- non couvert : rendu visuel des slides, animations, notes riches, relations OOXML complexes hors numerotation simple, commentaires, medias embarques ;
- dépendances : `PLANS.md`, `src-tauri/src/preview/mod.rs`, `src-tauri/src/commands/mod.rs` ;
- hypothèses : dans un premier palier, faire correspondre `slideN.xml` et `notesSlideN.xml` par indice est suffisamment utile et honnête.

**Critères de validation** :
- un `PPTX` local avec notes de presentation expose le texte visible plus les notes ;
- un payload auxiliaire `PPTX` avec notes suit le meme comportement en `Mode expert` ;
- l'absence de notes garde le comportement actuel sans faux positif ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 73 - Recovery avance V3 et advisory local enrichi

**Objectif** : ouvrir une vraie tranche finale sur les cas recovery complexes en etendant les contrats partages, le carving borne, l'advisory local des resultats et les surfaces UI autour des nouveaux signaux `complexity/source_view/compression/validation`.

**Pourquoi** :
- les signaux recovery restent encore trop pauvres pour distinguer clairement un cas simple, un cas reassemble et un cas derive d'une vue historique ou journalisee ;
- le carving borne a deux gaps a deja montre sa valeur mais laisse encore des cas simples a quatre segments hors du produit ;
- l'advisory local des resultats doit maintenant expliquer la stabilite, les blocages et la strategie d'export en s'appuyant sur ces nouveaux signaux.

**Périmètre** :
- couvert : extension des contrats `RecoveredFile`, `ByteRun` et `AiRecoveryBrief`, carving borne jusqu'a `4` segments / `3` gaps neutres avec statut de validation, rescoring conservative, filtres/resultats/export/expert alignes sur `source_view`, `compression_kind`, `recovery_complexity`, advisory local enrichi et tests associes ;
- non couvert : regroupement arbitraire de fragments binaires, recreation native AppleDouble/ADS, cloud IA, interpretation forensics exhaustive de toutes les structures avancees ;
- dépendances : `PLANS.md`, `src-tauri/src/types/mod.rs`, `src-tauri/src/carving/mod.rs`, `src-tauri/src/ai/mod.rs`, `src-tauri/src/commands/mod.rs`, `src/hooks/useIpc.ts`, `src/types/results.ts`, `src/types/ai.ts`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, `src/components/results/AiRecoveryBriefPanel.tsx`, i18n ;
- hypothèses : les nouveaux signaux restent purement explicatifs et read-only ; en cas de doute sur la reconstruction, on degrade le statut au lieu de promouvoir artificiellement le fichier.

**Critères de validation** :
- un resultat carve peut exposer `assembly_segment_count`, `gap_count`, `validator_status` et `recovery_complexity` ;
- l'advisory resultats distingue `export_now`, `verify_with_preview` et `complex_recovery_review` ;
- `ResultsPage` et `ExportPage` peuvent filtrer/afficher `snapshot`, `journal-derived`, `compressed` et `high-complexity` ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 74 - Release Apple stricte et manifeste multiplateforme

**Objectif** : transformer la pipeline release en workflow explicitement vendable cote macOS, avec preflight strict sur tags, manifeste enrichi, verifications post-build et publication d'artefacts Windows/Linux honnetement marques `unsigned`.

**Pourquoi** :
- la release macOS publie deja des bundles, mais la difference entre bundle genere et bundle signe/notarise reste encore trop implicite ;
- les artefacts Windows/Linux existent en verification, mais pas encore comme pieces publiees et checksummees de la release ;
- un manifeste enrichi et des checks post-build rendent la distribution plus credible pour le support et la vente.

**Périmètre** :
- couvert : preflight strict sur tags `v*` quand les secrets Apple requis manquent, manifeste release enrichi avec etat de signature/notarisation/stapling, checks `codesign/spctl/stapler`, publication d'artefacts Windows/Linux `unsigned` avec checksums, documentation et wording UI de support alignes ;
- non couvert : signature Windows effective, notarisation Linux inexistante, delta updates, rotation automatisee des cles updater, helper privilegie distribue hors secret Apple reel ;
- dépendances : `PLANS.md`, `scripts/release-preflight.mjs`, `scripts/generate-release-manifest.mjs`, `.github/workflows/release-macos.yml`, `.github/workflows/ci.yml`, `README.md`, `src/pages/SettingsPage.tsx`, i18n ;
- hypothèses : la tranche actuelle dispose seulement d'un provisioning Apple ; Windows/Linux sont publies mais explicitement non signes.

**Critères de validation** :
- `release:preflight` echoue sur un tag `v*` si les secrets Apple obligatoires sont absents ;
- le manifeste release expose `signed/notarized/stapled/platform/checksum/artifact_kind` ;
- la release GitHub attache aussi les bundles Windows/Linux generes avec un statut `unsigned` explicite ;
- `npm run release:preflight`, `npm run build`, `npm run test` et `cargo check --manifest-path src-tauri/Cargo.toml` passent.

## Chantier 75 - NTFS LZNT1 conservatif

**Objectif** : rendre les fichiers `NTFS` non residents compresses `LZNT1` effectivement exploitables de bout en bout, de l'analyse deleted-entry jusqu'au preview, a l'export et aux signaux UI/IA.

**Pourquoi** :
- le moteur `NTFS` sait deja traiter resident, sparse et ADS, mais la compression `LZNT1` restait un vrai trou produit sur Windows ;
- exposer `compression_kind` sans permettre la lecture/export reelle du payload compresse reviendrait a afficher un statut sans livrer la fonctionnalite ;
- cette tranche reste honnete car elle ne couvre que les cas conservatives : flux non residents, `lowest_vcn == 0`, `LZNT1` decompressible integralement, sans `EFS` ni cas ambigus sparse+compressed.

**Périmètre** :
- couvert : parse conservative du flag `compressed` sur attribut `DATA` non resident `NTFS`, reconstruction des `byte_runs` physiques stockes, decompression `LZNT1` pour preview/export, propagation de `compression_kind`/`validator_status`/`recovery_complexity` dans les `RecoveredFile`, fixtures et tests analyzers/imaging/commands ;
- non couvert : `EFS`, `lowest_vcn > 0`, decompression partielle, cas `sparse+compressed` ambigus, compression `NTFS` multi-split avancee, recreation native des semantiques Windows ;
- dépendances : `PLANS.md`, `src-tauri/src/analyzers/ntfs.rs`, `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/mod.rs`, UI resultats/export deja prepares aux signaux `compression` ;
- hypothèses : si la decompression ne restitue pas exactement le payload logique attendu, le fichier n'est pas remonte plutot que degrade en faux positif.

**Critères de validation** :
- un fichier supprime `NTFS` compresse `LZNT1` peut etre detecte avec `compression_kind = lznt1` ;
- `read_byte_runs_range` et `materialize_byte_runs` savent lire/exporter un payload `LZNT1` reconstruit ;
- le scan deleted `NTFS` et l'export associe exposent les bons signaux `compression_kind`, `validator_status` et `recovery_complexity` ;
- `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test` et `npm run test:e2e` passent.

## Chantier 76 - Finalisation du découpage progressif des commandes Tauri ✅ TERMINÉ (Sprint 7, 2026-04-18 — `commands/mod.rs` 4 750 → 211 LoC, −95,6 %)

**Objectif** : finir l'extraction progressive des blocs lourds restants de `src-tauri/src/commands/mod.rs` vers des sous-modules specialises (`imaging_cmd.rs`, `scan.rs`, `export.rs`) sans changer les signatures publiques des commandes Tauri ni la logique metier.

**Pourquoi** :
- `commands/mod.rs` reste le principal point chaud de maintenance du backend desktop ;
- le couplage actuel augmente le risque de regressions invisibles dans un domaine ou la tracabilite et la fiabilite priment ;
- une coupe mecanique, verifiee bloc par bloc, prepare les evolutions futures sans toucher au comportement recovery.

**Périmètre** :
- couvert : promotion controlee des helpers internes en `pub(super)`, extraction du bloc imaging restant, extraction progressive des workers/entrees scan restants, extraction des workers/entrees export restants, mise a jour de `docs/refactor-commands.md` selon l'avancement ;
- non couvert : changement de logique recovery, renommage des commandes publiques exposees a Tauri, refonte des types partages, migration des tests inline vers `tests/` ;
- dépendances : `PLANS.md`, `docs/refactor-commands.md`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs`, `src-tauri/src/commands/scan.rs`, `src-tauri/src/commands/export.rs`, `src-tauri/src/commands/state.rs`, `src-tauri/src/lib.rs` ;
- hypothèses : la toolchain Rust locale permet `cargo fmt`, `cargo check` et `cargo test --lib` entre chaque bloc ; si ce n'est plus le cas, on s'arrete apres la derniere coupe validee.

**Contraintes** :
- ne jamais modifier la semantique read-only des flux scan/imaging/export ;
- ne jamais changer les signatures publiques ni casser `tauri::generate_handler!` ;
- preferer des deplacements mecaniques avec visibilite minimale plutot qu'une "amelioration" opportuniste ;
- garder les helpers transverses traces et explicites tant qu'ils ne sont pas extraits proprement vers `state.rs`.

**Architecture concernée** :
- commands : `mod.rs`, `imaging_cmd.rs`, `scan.rs`, `export.rs`, `state.rs` ;
- imaging : workers de creation d'image read-only et helpers privilegies macOS ;
- analyzers / carving : seulement via les appels existants, sans changement de logique ;
- desktop app : aucun changement fonctionnel attendu, uniquement maintien du contrat IPC ;
- shared contracts : inchanges dans cette tranche.

**Contrats et interfaces** :
- toutes les commandes Tauri conservees doivent garder le meme nom, les memes arguments et le meme type de retour ;
- les workers internes de domaine doivent devenir accessibles uniquement via `pub(super)` si un autre sous-module `commands::*` en depend ;
- `start_scan`, `start_imaging` et `start_export` doivent continuer a deleguer au meme pipeline qu'avant extraction ;
- les tests inline existants doivent rester executables apres deplacement des fonctions qu'ils couvrent.

**UX / UI** :
- aucun changement visible voulu cote interface ;
- tout impact utilisateur serait considere comme une regression ;
- si un test manuel est necessaire, il doit verifier qu'un scan, un imaging et un export continuent d'exposer les memes etats qu'avant.

**Étapes d’implémentation** :
1. documenter ce chantier et aligner le suivi avec `docs/refactor-commands.md` ;
2. extraire le bloc imaging restant avec promotion minimale des helpers partages ;
3. valider Rust et TypeScript ;
4. reevaluer si un bloc scan ou export supplementaire peut etre deplace dans la meme session sans augmenter le risque ;
5. mettre a jour le document de refactor avec l'etat reel du chantier.

**Tests et validation** :
- `cargo fmt --manifest-path src-tauri/Cargo.toml` ;
- `cargo check --manifest-path src-tauri/Cargo.toml` ;
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` ;
- `npx tsc --noEmit` ;
- si la coupe touche un pipeline runtime complet : smoke test manuel scan/imaging/export.

**Risques** :
- oubli d'un `use` ou d'une visibilite lors d'un deplacement de fonctions privees ;
- casse subtile des tests inline si une fonction deplacee devient inaccessible ;
- extraction trop ambitieuse dans une seule session, surtout sur le bloc scan qui reste massif.

**Questions ouvertes** :
- faut-il poursuivre `scan` et `export` dans la meme session si `imaging` passe proprement, ou rester strictement sur un bloc valide a la fois ?
- veut-on ensuite deplacer les helpers transverses de `mod.rs` vers `state.rs` dans un chantier separe une fois les domaines sortis ?

**Statut 2026-04-17 (session sprint-4)** :
- **Pass 1 landé** : bloc helpers export (~440 lignes : select_files_for_export, build_source_path, file_uses_recovery_image, export_recovered_file, export_resource_fork_sidecar, export_alternate_data_stream_sidecars, resolve_target_path, verify_exported_file, verify_reconstructed_export + helpers privés) extrait de `commands/mod.rs` vers `commands/export.rs`. Visibilité promue à `pub(crate)` pour permettre le `pub(super) use export::{...}` depuis `mod.rs`. Symboles utilisés uniquement par les tests inline gardés sous `#[cfg(test)] use export::{...}`.
- **Pass 2 landé** : bloc helpers scan (`register_scan_error`, `compute_progress`, `display_parent_path`, `guess_mime_type`, `is_previewable_extension`) déplacé vers `commands/scan.rs` avec imports `InventoryScanSession`, `MAX_SESSION_LOGS` et accès aux helpers restants de `mod.rs` via `super::`.
- **Bilan** : `commands/mod.rs` 8427 → 7961 lignes (−466), `commands/export.rs` 1155 → 1597, `commands/scan.rs` 648 → 737. Le projet Rust compile propre (`cargo check --all-targets` sans warning) et **303 tests `cargo test --lib` verts**.
- **Reste différé** : le bloc scan workers (`run_potential_volume_scan` + `run_deleted_*_scan` + `run_signature_carving_scan` + `run_inventory_scan` soit ~2700 lignes) ainsi que le bloc imaging (`ImagingSourcePlan` + helpers `resolve_imaging_source_plan`, `recommended_imaging_profile*`, `append_imaging_artifact_issue_logs`, `apply_imaging_artifact_*`, `create_read_only_image_with_optional_elevation`, `run_macos_privileged_image_acquisition_for_recovery`, ~900 lignes) — non bloquants fonctionnellement, à faire dans une session refactor dédiée.

**Statut 2026-04-18 (session sprint-5)** :
- **Pass 3 landé** (slice `imaging_helpers`) : bloc imaging (~517 lignes) extrait de `commands/mod.rs` vers un nouveau fichier `commands/imaging_cmd/helpers.rs`. Contenu : `ImagingSourcePlan` (enum + impl), `resolved_imaging_source_path`, `is_raw_device_path`, `is_permission_denied_imaging_error`, `imaging_requires_elevation_fallback`, `recommended_imaging_profile*`, `append_imaging_profile_log`, `imaging_profile_for_session`, `imaging_unreadable_error_count`, `append_imaging_artifact_issue_logs`, `apply_imaging_artifact_issue_metrics`, `apply_imaging_artifact_session_details`, `resolve_imaging_source_plan`, `update_image_acquisition_progress`, `read_u64_report`, `read_image_artifact_report` (macOS), `try_unmount_macos_device` (macOS), `run_macos_privileged_image_acquisition_for_recovery` (macOS), `inspect_potential_volumes_for_diagnostic`, `create_read_only_image_with_optional_elevation`. Visibilité promue à `pub(crate)`, sauf les deux prédicats internes (`is_raw_device_path`, `is_permission_denied_imaging_error`) qui restent privés au module `helpers`. Ré-exportation via `pub(super) use imaging_cmd::helpers::{...}` dans `mod.rs` pour que `scan.rs`, `device.rs` et les tests inline continuent d'atteindre les helpers par `super::<fn>`.
- **Imports nettoyés dans `commands/mod.rs` (pass 3)** : `crate::core` supprimé, `crate::partitioning` déplacé en test-only, `use imaging_cmd::privileged_macos::{...}` supprimé, ajouts dans `mod tests`.
- **Pass 4 landé** (slice `scan_deleted_fat_family`) : `run_deleted_fat32_scan` (~180) + `run_deleted_exfat_scan` (~180) + `run_deleted_ntfs_scan` (~345) déplacés de `mod.rs` vers `scan.rs` en `pub(crate) fn`. Imports ajoutés dans `scan.rs` : `crate::analyzers::{exfat, fat32, ntfs}` + bloc `use super::{append_imaging_artifact_issue_logs, append_scan_log, apply_imaging_artifact_issue_metrics, apply_imaging_artifact_session_details, create_read_only_image_with_optional_elevation, elapsed_seconds, fail_scan_session, finalize_cancelled_scan, imaging_profile_for_session, persist_scan_session, unix_timestamp_ms, update_progress, wait_for_scan_permission}`. Ré-export `pub(super) use scan::{run_deleted_fat32_scan, run_deleted_exfat_scan, run_deleted_ntfs_scan}` dans `mod.rs`. Bilan : **7 444 → 6 737** dans `mod.rs` ; **737 → 1 452** dans `scan.rs`. 325 tests verts.
- **Pass 5 landé** (slice `scan_deleted_unix_family`) : `run_deleted_ext4_scan` (~319) + `run_deleted_hfsplus_scan` (~237) + `run_deleted_apfs_scan` (~180) déplacés vers `scan.rs`. `scan.rs` étend son analyzer block à `{apfs, exfat, ext4, fat32, hfsplus, ntfs}`. `mod.rs` retire `analyzers::ext4` du top-level, déplacé vers `mod tests`. Ré-export étendu dans `mod.rs`. Bilan : **6 737 → 6 001** dans `mod.rs` ; **1 452 → 2 189** dans `scan.rs`. 325 tests verts.
- **Pass 6 landé** (slice `scan_workers_tail`) : `run_potential_volume_scan` (~725) + `run_signature_carving_scan` (~181) + `run_inventory_scan` (~232) déplacés vers `scan.rs` en `pub(crate) fn`, plus les 5 helpers privés `potential_volume_source_snapshot_path`, `potential_volume_slice_path`, `potential_volume_slice_length`, `rebase_slice_offset`, `recovered_file_from_slice` (qui restent `fn` privés à `scan.rs`). Const `QUICK_SCAN_MAX_DEPTH = 2` déplacée de `mod.rs` vers `scan.rs`. Imports ajoutés dans `scan.rs` : `std::fs`, `crate::{carving, imaging}`, types `{ByteRun, FileFork, NamedFileFork, PotentialVolume}`, `super::{filesystem_label, imaging_cmd, ImagingSourcePlan}` pour appeler `imaging_cmd::create_local_image_snapshot` et typer le plan imaging.
- **Nettoyage final de `commands/mod.rs`** : `use crate::{analyzers::..., carving, imaging, types::*}` simplifié en `use crate::types::*;` ; `std::{sync::{Arc, Mutex}, path::{Path, PathBuf}, time::SystemTime}` nettoyé en `std::{fs, io::{Cursor, Write}, path::Path}` (seule la partie support-bundle/report en a encore besoin) ; `use state::{InventoryScanSession, MAX_SESSION_LOGS}` supprimé du top-level ; ré-exports scan réduits à `{guess_mime_type, run_deleted_*_scan, run_inventory_scan, run_potential_volume_scan, run_signature_carving_scan}` ; `compute_progress` déplacé en `#[cfg(test)] use scan::compute_progress;` ; `display_parent_path, is_previewable_extension, register_scan_error` retirés (plus aucune référence). `mod tests` reçoit `crate::analyzers::{apfs, ext4, hfsplus, ntfs}`, `crate::imaging`, `std::path::PathBuf`, `std::sync::{Arc, Mutex}`, `InventoryScanSession`, `MAX_SESSION_LOGS`.
- **Bilan Sprint 5 complet** : `commands/mod.rs` **7 961 → 4 750 lignes** (−3 211, −40 %). `commands/scan.rs` **737 → 3 441 lignes**. `commands/imaging_cmd/helpers.rs` nouveau à **552 lignes**. `cargo check --all-targets` 0 warning. **325 tests Rust verts**, **54 tests UI verts**. `npx tsc --noEmit` propre. Cible `mod.rs < 1 500` non atteinte : reste à sortir support-bundle builder (~200 LoC), recovery/narrative reports, CSV export, lab bundle, rankings potential-volume, helpers d'écriture disque, et surtout les **tests inline (~3 000 LoC)** à migrer vers `tests/` — à faire dans un sprint dédié si prioritaire.
- **A3 (native tauri-driver multi-flux) et B6 (scheduler passif filesystem_memory) reportés** : non démarrés cette session, rester dans le backlog Chantier 76/82.

**Statut 2026-04-18 (session sprint-7 — CLÔTURE Chantier 76)** :
- **T1 landé** : `mod tests` inline (4 243 LoC) migré vers `src-tauri/src/commands/tests.rs` ; `mod.rs` remplacé par `#[cfg(test)] mod tests;`. Zéro changement de visibilité, tests intacts.
- **T2 landé** : support-bundle builder extrait (`SupportBundleManifest` + 6 fns zip/log) vers `src-tauri/src/commands/support_bundle.rs` (145 LoC, `build_support_bundle_archive_bytes` en `pub(crate)`, ré-exporté via `pub(super) use support_bundle::build_support_bundle_archive_bytes;`). Nettoyage des imports orphelins (`serde::Serialize`, `Cursor`, `Write`, `zip::*`) dans `mod.rs`.
- **T3 landé** : `write_text_report_to_path` + `write_binary_file_to_path` (65 LoC) fusionnés dans `commands/state.rs` (`pub(crate)`). Ré-export étendu : `pub(super) use state::{…, write_binary_file_to_path, write_text_report_to_path};`. Imports `fs` / `Path` rendus `#[cfg(test)]` (plus nécessaires en non-test).
- **T4 landé** : ranking potential-volume (`supported_deleted_recovery_filesystem`, `supported_potential_volume_filesystem`, `potential_volume_detection_rank`, `best_supported_potential_volume`, `guided_supported_potential_volume_candidate`) déplacé vers `commands/scan.rs` (3 en `pub(crate)`, 2 privées). Call site `scan.rs:247` simplifié en appel local. Ré-export étendu dans `mod.rs`. `use crate::types::*;` rendu `#[cfg(test)]`.
- **Bilan Sprint 7 complet** : `commands/mod.rs` **4 750 → 211 lignes** (−4 539, **−95.6 %**). Cible historique `< 1 500` atteinte dès T1 puis ramenée ~7× en-dessous. Nouveaux fichiers : `commands/tests.rs` (4 208), `commands/support_bundle.rs` (145). `commands/scan.rs` **3 441 → 3 528** (+87). `commands/state.rs` **1 078 → 1 145** (+67).
- **Portes à chaque tranche** : `cargo fmt` ; `cargo check --all-targets` = 0 warning ; `cargo test --lib` = **334 verts** ; `npx tsc --noEmit` propre ; `npm run test:ui` = **54 verts**.
- **Statut** : **Chantier 76 clos**. `commands/mod.rs` est désormais un fichier d'agrégation (déclarations `mod *;` + ré-exports `pub(super) use` + constante `HEX_PREVIEW_LINE_WIDTH`). Aucune signature publique Tauri changée, aucune logique runtime touchée.
- **Reste ouvert hors périmètre** : l'éclatement de `commands/tests.rs` (4 208 LoC monobloc) par domaine si la maintenabilité devient un point dur plus tard — non prioritaire car n'apporte plus de gain sur la cible LoC de `mod.rs`.

## Chantier 77 - Durcissement produit post-audit et backlog P0/P1/P2

**Objectif** : transformer l'audit complet du produit en plan d'execution concret pour amener l'application d'un build prometteur a un logiciel de recuperation credible, verifiable, honnete et vendable, sans raccourcis de securite ni hardcodes critiques.

**Pourquoi** :
- le backend est deja substantiel et bien teste, mais plusieurs ecarts bloquent aujourd'hui la confiance produit ;
- dans un domaine de recuperation de donnees, une incoherence UX, un faux hardening ou un horodatage faux valent plus qu'un simple bug visuel ;
- le projet ne peut pas pretendre etre "meilleur que le marche" tant que la surete, la tracabilite, la finition produit et la validation comparative ne sont pas fermees proprement.

**Périmètre** :
- couvert : correction des ecarts critiques reveles par l'audit, suppression des hardcodes produit visibles, realignement des defaults UX/i18n avec `AGENTS.md`, durcissement des flux diagnostic/export/reporting, garde-fous sur les actions chiffrement, extension de la validation E2E, formalisation du benchmark produit ;
- non couvert : ajout immediat de nouvelles families de recovery "magiques", promesse marketing de superiorite sans benchmark public, gros refactor backend hors besoin direct ;
- dépendances : `PLANS.md`, `README.md`, `src/pages/*`, `src/components/*`, `src/i18n/*`, `src/stores/appStore.ts`, `src-tauri/src/commands/*`, `src-tauri/src/encryption/mod.rs`, `src-tauri/src/lib.rs`, `e2e/*` ;
- hypothèses : le produit reste prioritairement desktop local, en lecture seule sur la source, avec IA locale ou cloud explicite selon l'ecran et les capacites actives.

**Contraintes** :
- ne jamais court-circuiter un garde-fou de surete via un bouton "principal" plus permissif que la recommandation backend ;
- ne jamais laisser croire qu'une operation sensible est read-only si elle appelle en realite une commande systeme modifiant l'etat d'un volume ;
- ne jamais garder de version, date, URL de paiement ou libelle produit critiques en dur si la valeur reelle est disponible ailleurs ;
- novice et expert doivent rester clairement distingues ;
- anglais par defaut, francais supporte ;
- light mode par defaut, dark mode en variante ;
- toute correction doit rester traçable par tests ou logs.

**Architecture concernée** :
- core / encryption : capacites chiffrement et actions a risque ;
- commands : diagnostic, reporting, export, handlers Tauri enregistres ;
- desktop app : pages `Home`, `Devices`, `Diagnostic`, `Results`, `Export`, `Settings` ;
- shared contracts : recommandations diagnostic, capacites runtime, statuts d'action sensibles ;
- i18n / design system : defaults langue/theme et libelles UI ;
- QA : `vitest`, `playwright`, fixtures E2E, benchmarks de comparaison.

**Contrats et interfaces** :
- le CTA principal de `DiagnosticPage` doit toujours deriver de la recommandation backend la plus sure, jamais d'une heuristique UI parallele ;
- les commandes de rapport doivent utiliser la vraie version app et un horodatage exact, stable et testable ;
- les actions chiffrement doivent etre soit retirees du handler general, soit degradees en flux expert explicitement marques comme sensibles/non-read-only ;
- les composants UI partages ne doivent pas embarquer de texte localise en dur ;
- l'environnement browser preview doit survivre sans API Tauri disponibles ;
- les specs E2E doivent couvrir au minimum un parcours stable `diagnostic -> scan -> resultats -> export/paywall/report`.

**UX / UI** :
- `DiagnosticPage` : si le verdict impose `image-first`, `stop-usage` ou `lab`, l'ecran novice ne doit pas offrir de raccourci plus agressif ;
- `DevicesPage` : fonctionnement stable en preview web, avec message explicite quand un watcher natif n'est pas disponible ;
- `ExportPage` : distinction claire entre "aucun resultat", "paywall", "destination invalide", "export en cours" ;
- `ResultsPage` : rapports/csv telecharges de maniere robuste, avec messages clairs et ouverture native non fragile ;
- composants partages : plus aucun texte UX visible en dur hors fichiers i18n ;
- `Home` / shell : defaults conformes au produit annonce, sans incoherence anglais/francais ou light/dark.

**Étapes d’implémentation** :
1. **P0 - Surete et veracite produit**
   - corriger le CTA principal de [src/pages/DiagnosticPage.tsx](/Users/Artisaul/Desktop/recupere/src/pages/DiagnosticPage.tsx) pour qu'il reutilise la recommandation backend deja calculee, sans logique divergente ;
   - auditer puis decider du sort des commandes chiffrement exposees dans [src-tauri/src/commands/device.rs](/Users/Artisaul/Desktop/recupere/src-tauri/src/commands/device.rs), [src-tauri/src/encryption/mod.rs](/Users/Artisaul/Desktop/recupere/src-tauri/src/encryption/mod.rs) et [src-tauri/src/lib.rs](/Users/Artisaul/Desktop/recupere/src-tauri/src/lib.rs) :
     soit retrait du handler public,
     soit confinement en mode expert avec wording explicite "operation systeme sensible, non read-only" ;
   - remplacer dans [src-tauri/src/commands/mod.rs](/Users/Artisaul/Desktop/recupere/src-tauri/src/commands/mod.rs) la version hardcodee et `chrono_like_now()` par une source de verite versionnee et une date exacte testee ;
   - supprimer le placeholder Stripe de [src/pages/ExportPage.tsx](/Users/Artisaul/Desktop/recupere/src/pages/ExportPage.tsx) : si l'URL n'est pas fournie, l'UI doit afficher un etat bloque explicite plutot qu'ouvrir une fausse URL.
2. **P1 - Cohérence produit et de-hardcoding**
   - realigner les defaults de [src/i18n/index.ts](/Users/Artisaul/Desktop/recupere/src/i18n/index.ts) et [src/stores/appStore.ts](/Users/Artisaul/Desktop/recupere/src/stores/appStore.ts) avec `AGENTS.md` ;
   - de-hardcoder les libelles visibles dans [src/components/common/CustomSelect.tsx](/Users/Artisaul/Desktop/recupere/src/components/common/CustomSelect.tsx), [src/components/results/ResultsToolbar.tsx](/Users/Artisaul/Desktop/recupere/src/components/results/ResultsToolbar.tsx), [src/components/device/SmartDashboard.tsx](/Users/Artisaul/Desktop/recupere/src/components/device/SmartDashboard.tsx), [src/components/layout/SidebarNav.tsx](/Users/Artisaul/Desktop/recupere/src/components/layout/SidebarNav.tsx), [src/components/results/FileGalleryPanel.tsx](/Users/Artisaul/Desktop/recupere/src/components/results/FileGalleryPanel.tsx), [src/components/results/AiAnalysisPanel.tsx](/Users/Artisaul/Desktop/recupere/src/components/results/AiAnalysisPanel.tsx) ;
   - corriger [src/pages/DevicesPage.tsx](/Users/Artisaul/Desktop/recupere/src/pages/DevicesPage.tsx) pour survivre sans event API Tauri ;
   - rendre le flow rapport/csv de [src/pages/ResultsPage.tsx](/Users/Artisaul/Desktop/recupere/src/pages/ResultsPage.tsx) robuste selon l'environnement desktop/web.
3. **P1 - Validation fonctionnelle**
   - corriger les specs cassantes `Playwright` dans [e2e/app-shell.smoke.spec.ts](/Users/Artisaul/Desktop/recupere/e2e/app-shell.smoke.spec.ts), [e2e/devices.spec.ts](/Users/Artisaul/Desktop/recupere/e2e/devices.spec.ts) et [e2e/license-paywall.spec.ts](/Users/Artisaul/Desktop/recupere/e2e/license-paywall.spec.ts) ;
   - ajouter un vrai scenario stable de regression couvrant au moins un parcours de recuperation/consultation sur fixture locale ;
   - s'assurer que `npm run build`, `npm run test:ui`, `npm run test:e2e` et `cargo test --manifest-path src-tauri/Cargo.toml` passent ensemble.
4. **P2 - Mise a niveau marche**
   - etablir une matrice de benchmark interne contre des outils accessibles sans achat supplementaire, avec `PhotoRec` et `TestDisk` comme baseline obligatoire et `DMDE` free ou des suites payantes seulement en evidence bonus, en separant ce qui est comparable de ce qui ne l'est pas encore ;
   - documenter dans `README.md` ce que le produit fait aujourd'hui, ce qu'il estime, et ce qu'il ne pretend pas faire ;
   - preparer les chantiers de differenciation a plus forte valeur :
     imagerie degradee haut de gamme,
     rescue bootable,
     bench public sur corpus,
     UX novice de crise,
     rapports d'audit signables,
     workflows RAID/NAS/VM vraiment exposes.

**Tranche suivante retenue (P2 concret)** :
- exposer un workflow desktop local "ouvrir une image de recuperation" depuis `DevicesPage`, au lieu de garder le support `raw/E01/VMDK/VHD/VHDX` presque invisible ;
- ajouter un registre local des sources importees, sans jamais ecrire sur la source elle-meme ;
- permettre au moteur d'imagerie de normaliser une source fichier lisible vers une image locale read-only, y compris quand la source est un conteneur virtuel supporte par `virtual_disk` ;
- rester explicite sur la limite actuelle : les scans `lost-volume` et l'inspection de partitions retrouvees peuvent rester partiels tant qu'un format virtuel n'est pas d'abord normalise vers une image locale exploitable.

**Modules impactes pour cette tranche** :
- backend : `src-tauri/src/commands/device.rs`, `src-tauri/src/imaging/mod.rs`, nouveau module de registre local des sources importees, eventuellement `src-tauri/src/lib.rs` ;
- frontend : `src/pages/DevicesPage.tsx`, `src/components/device/DeviceCard.tsx`, `src/hooks/ipc/device.ts`, i18n ;
- QA : tests Rust sur le registre / la normalisation d'image, smoke E2E sur l'UI appareils.

**Critères de validation supplementaires** :
- l'utilisateur peut ajouter une image locale compatible depuis l'UI desktop sans hack manuel ;
- la source importee reapparait dans `DevicesPage` comme une source `image` distincte ;
- une source `VMDK` / `VHD` / `E01` importee peut au minimum etre normalisee vers une image locale read-only via le moteur existant ;
- le diagnostic et les scans qui ont besoin d'une source raw reutilisent ensuite cette normalisation de facon transparente ;
- aucune action ne laisse croire qu'un volume virtuel est "monte" ou directement modifiable ;
- la suppression d'une source importee retire seulement son enregistrement local, jamais le fichier source.

**Tranche suivante retenue (P2 UX des sources importees)** :
- rendre l'etat "pret pour analyse" explicite pour chaque source importee, au lieu de laisser un conteneur `E01/VMDK/VHD/VHDX` se normaliser silencieusement au premier ecran qui en a besoin ;
- exposer un statut read-only de preparation locale avec trois etats minimum :
  source directe exploitable,
  cache local requis mais pas encore prepare,
  cache local prepare et reutilisable ;
- ajouter une action explicite "preparer la source pour analyse" depuis `DevicesPage`, avec journalisation d'audit ;
- empecher l'UI novice de lancer le diagnostic ou les signaux avances d'une source virtuelle importee tant que sa preparation locale n'est pas prete.

**Modules impactes pour cette tranche UX** :
- backend : `src-tauri/src/imported_sources/mod.rs`, `src-tauri/src/commands/device.rs`, `src-tauri/src/lib.rs`, eventuellement `src-tauri/src/types/mod.rs` ;
- frontend : `src/types/device.ts`, `src/hooks/ipc/device.ts`, `src/components/device/*`, `src/pages/DevicesPage.tsx`, i18n, browser preview ;
- QA : tests Rust du statut de preparation, smoke E2E de l'etat "prepare / non prepare".

**Criteres de validation supplementaires pour cette tranche UX** :
- une source importee `RAW/IMG/DD/BIN` apparait comme directement exploitable sans etape cache ;
- une source importee `E01/VMDK/VHD/VHDX` signale clairement qu'une preparation locale est requise avant diagnostic approfondi ;
- l'utilisateur peut lancer explicitement cette preparation depuis l'UI et voir l'etat passer a "pret" sans ambiguite ;
- en preview web, cet etat reste testable sans API Tauri reelle ;
- le bouton `Diagnose` et le panneau `Advanced Signals` ne masquent pas une normalisation implicite sur ces sources non preparees.

**Tranche suivante retenue (P2 continuité Diagnostic / Scan)** :
- propager ce meme statut de preparation locale jusque dans `DiagnosticPage` et `ScanPage` afin qu'aucune navigation directe, restauration d'etat ou preview seedee ne declenche une normalisation implicite ;
- bloquer explicitement le chargement du diagnostic heuristique tant qu'une source importee virtuelle n'est pas preparee ;
- bloquer l'auto-start et les workflows de scan tant qu'une source importee virtuelle n'est pas preparee ;
- garder l'action de preparation visible dans ces ecrans, sans renvoyer l'utilisateur vers `DevicesPage` pour terminer le travail.

**Modules impactes pour cette tranche continuity** :
- frontend : `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, nouveau hook partage de statut de preparation, `src/components/device/ImportedSourceStatusPanel.tsx`, i18n ;
- QA : smoke E2E sur les gates `Diagnostic` / `Scan` quand une source importee est encore non preparee.

**Criteres de validation supplementaires pour cette tranche continuity** :
- ouvrir `DiagnosticPage` directement sur une source importee `VMDK/VHD/E01` non preparee n'appelle pas le diagnostic backend ;
- ouvrir `ScanPage` directement sur une source importee `VMDK/VHD/E01` non preparee ne lance aucun auto-start backend ;
- les deux ecrans affichent un message explicite, le statut read-only et une action de preparation locale ;
- apres preparation, ces ecrans peuvent reprendre leur flux normal sans incoherence d'etat.

**Tests et validation** :
- `cargo test --manifest-path src-tauri/Cargo.toml` passe ;
- `npm run build` passe ;
- `npm run test:ui` passe ;
- `npm run test:e2e` passe ;
- verification manuelle desktop :
  `DiagnosticPage` ne propose jamais une action plus risquee que la reco backend ;
  `ExportPage` n'ouvre jamais une URL de paiement factice ;
  `DevicesPage` reste stable en preview navigateur ;
  les rapports affichent la bonne version et la bonne date ;
  l'app demarre en anglais et en light mode par defaut ;
- revue grep :
  plus aucun `test_REPLACE_ME` ni libelle UX localise critique en dur dans les composants partages.

**Risques** :
- corriger les defaults anglais/light peut casser des tests ou captures qui supposaient le francais/dark ;
- retirer ou requalifier les actions chiffrement peut frustrer certains usages experts mais reste necessaire pour l'honnetete produit ;
- rendre les rapports plus stricts peut exposer d'autres zones qui supposaient des dates/versions approximatives ;
- l'ajout d'un benchmark public peut montrer que certaines fonctions concurrentes restent absentes : c'est une dette saine a rendre visible.

**Critères de validation** :
- aucune action novice ne contourne les garde-fous du diagnostic ;
- aucune commande sensible non read-only n'est exposee comme banale ou "safe" ;
- aucun placeholder critique produit n'est encore atteignable en runtime ;
- les tests full stack passent ;
- le produit ne se presente plus comme "meilleur que le marche" sans preuves comparatives ;
- un backlog clair de differenciation existe pour ce qui manque encore.

**Limites connues** :
- meme apres fermeture de ce chantier, cela ne suffira pas a prouver objectivement une superiorite sur le marche sans benchmark reproductible ;
- le produit restera en dessous des leaders sur certaines dimensions tant qu'il n'aura pas de workflow rescue bootable, d'imagerie avancee degradee et de preuves comparatives publiees ;
- la suppression des hardcodes UI ne remplace pas une vraie campagne QA materielle sur disques reels et images corrompues.

**Questions ouvertes** :
- veut-on conserver les commandes chiffrement uniquement pour un futur `Mode expert laboratoire`, ou les sortir completement du produit desktop generaliste ;
- prefere-t-on bloquer totalement le paywall sans URL live ou basculer sur un formulaire "contact sales / waitlist" ;
- faut-il mesurer la superiorite produit d'abord sur la surete/UX, ou d'abord sur la couverture recovery brute ;
- veut-on ouvrir un document benchmark separe avec corpus, protocoles et metriques avant toute nouvelle promesse marketing ?

**Tranche suivante retenue (P2 continuity preview prepare -> diagnostic -> scan)** :
- rendre l'etat `prepare` persistant et mutable dans le browser preview afin qu'une source importee preparee depuis `DevicesPage`, `DiagnosticPage` ou `ScanPage` reste prete sur l'ecran suivant ;
- fournir un fallback preview explicite et honnete pour `fetchDiagnostic()` sur les sources image deja preparees, sans faire croire qu'un diagnostic desktop reel a eu lieu ;
- fournir un fallback preview de demarrage de scan et de progression minimale jusqu'aux resultats afin de valider le workflow UI complet `prepare -> diagnostic -> scan -> results` sans API Tauri ;
- conserver des garde-fous stricts : aucune normalisation implicite, aucune ecriture sur la source, et aucune promesse de moteur desktop reel en preview.

**Modules impactes pour cette tranche preview continuity** :
- frontend : `src/utils/browserPreviewSeed.ts`, `src/hooks/ipc/device.ts`, `src/hooks/ipc/diagnostic.ts`, `src/hooks/ipc/scan.ts`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, eventuellement `src/types/*` ;
- QA : nouveau scenario E2E couvrant la preparation d'une source importee suivie d'un diagnostic et d'un scan preview jusqu'aux resultats.

**Criteres de validation supplementaires pour cette tranche preview continuity** :
- en preview navigateur, cliquer `Prepare for Analysis` met a jour un etat local persistant reutilisable par les autres ecrans ;
- une source importee preparee peut charger un diagnostic preview explicite sans erreur IPC Tauri ;
- une recommandation de diagnostic sur cette source peut ouvrir `ScanPage` puis lancer un scan preview minimal jusqu'aux resultats ;
- les messages UI indiquent clairement qu'il s'agit d'un fallback preview et non d'une preuve de recuperation desktop.

**Tranche suivante retenue (P2 imaging resume / interruption tolerance)** :
- rendre l'imagerie read-only capable de reprendre explicitement une image partielle locale au lieu de recommencer silencieusement a zero apres interruption ;
- tracer cette reprise avec des metadonnees locales minimales, verifiees avant reutilisation, afin d'eviter de reprendre un fragment stale ou incoherent ;
- propager l'information de reprise jusque dans les contrats scan/history pour que l'UI puisse montrer qu'une session a reemploye des octets deja captures ;
- garder une posture sure : jamais d'ecriture sur la source, jamais de reprise implicite non verifiable, et redemarrage propre si le checkpoint local ne correspond pas a la source demandee.

**Modules impactes pour cette tranche imaging resume** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/types/mod.rs` ;
- frontend : `src/types/scan.ts`, `src/hooks/ipc/scan.ts`, `src/pages/ScanPage.tsx`, `src/pages/HistoryPage.tsx`, i18n ;
- QA : tests Rust de reprise d'image partielle, smoke UI ou E2E sur l'affichage d'une session d'imagerie reprise.

**Criteres de validation supplementaires pour cette tranche imaging resume** :
- une image `.partial` coherente peut etre reprise proprement vers la destination finale sans recopier les premiers octets deja presents ;
- un checkpoint local stale ou incoherent est jete et l'imagerie repart proprement depuis zero ;
- l'historique et l'ecran de scan indiquent quand une session a repris une capture locale partielle et combien d'octets ont ete reutilises ;
- les validations existantes (`cargo test`, `npm run build`, `npm run test:ui`, `npm run test:e2e`) restent vertes.

**Statut 2026-04-09** :
- tranche fermee ;
- backend : reprise explicite d'image `.partial` avec checkpoint JSON local, rejet des checkpoints incoherents, et journalisation de la reprise ;
- frontend : propagation `resume_from_bytes` jusque dans `ScanPage` et `HistoryPage`, avec details visibles pour les sessions d'imagerie reprises ;
- QA : deux tests Rust de reprise/stale-checkpoint ajoutes et un smoke E2E `History` sur une session d'imagerie reprise.

**Tranche suivante retenue (P2 cautious imaging profile)** :
- introduire un profil d'imagerie read-only `cautious` pour les sources dont l'etat materiel ou le niveau de risque impose une lecture plus prudente qu'une copie standard ;
- faire remonter ce profil dans le diagnostic afin que l'UI annonce explicitement la strategie choisie avant de lancer une image ou un scan image-backed ;
- appliquer ce profil dans le backend d'imagerie avec des lectures plus petites et un nombre limite de retries au lieu d'une lecture agressive unique ;
- garder une posture honnete : aucune promesse de contournement magique des secteurs defectueux, aucune reprise silencieuse d'erreur, et aucune ecriture sur la source.

**Modules impactes pour cette tranche cautious imaging profile** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs`, `src-tauri/src/commands/scan.rs`, `src-tauri/src/types/mod.rs` ;
- frontend : `src/types/diagnostic.ts`, `src/types/scan.ts`, `src/hooks/ipc/diagnostic.ts`, `src/pages/DevicesPage.tsx`, `src/pages/DiagnosticPage.tsx`, `src/pages/ScanPage.tsx`, eventuellement un helper partage pour deriver le profil depuis l'etat du support, i18n ;
- QA : tests Rust sur les options d'imagerie prudente et smoke UI/E2E sur l'affichage du profil prudent.

**Criteres de validation supplementaires pour cette tranche cautious imaging profile** :
- un support `degraded`, `failing` ou `unresponsive` n'est plus image en mode implicite "standard" sans l'annoncer a l'utilisateur ;
- le diagnostic annonce clairement quand le profil `cautious` est recommande et pourquoi ;
- le backend d'imagerie utilise effectivement des lectures plus petites et des retries limites quand ce profil est actif ;
- les logs de session permettent de retracer qu'un profil prudent a ete applique ;
- les validations existantes (`cargo test`, `npm run build`, `npm run test:ui`, `npm run test:e2e`) restent vertes.

**Statut 2026-04-09** :
- tranche fermee ;
- backend : profil `cautious` branche de bout en bout dans les sessions de scan, l'imagerie standalone, les scans image-backed, et le helper privilegie macOS avec propagation explicite `--profile` ;
- diagnostic et contrats partages : `DiagnosticResult` expose maintenant le profil d'imagerie recommande et la raison i18n correspondante, cote Rust comme cote TypeScript ;
- frontend : `DevicesPage`, `DiagnosticPage`, `ScanPage` et `ExpertPage` propagent et affichent explicitement le profil prudent ; les workflows `lost-volume` conservent aussi cette information ;
- QA : un test Rust cible les parametres du profil prudent, un smoke Playwright verifie la remontee `diagnostic -> scan`, et la validation complete reste verte.

**Tranche suivante retenue (P2 sick-disk imaging / unreadable ranges)** :
- ajouter une tolerance explicite aux lectures irrécupérables quand le profil d'imagerie `cautious` est actif, afin qu'une acquisition read-only puisse continuer sur un support malade au lieu d'echouer au premier bloc encore illisible ;
- neutraliser ces zones de facon honnete dans l'image locale finale en ecrivant des octets nuls a la place des donnees introuvables, sans jamais pretendre reconstruire les secteurs physiques perdus ;
- tracer precisement ces plages illisibles dans le backend, les journaux techniques, la progression live et l'historique afin que l'utilisateur sache combien d'octets et combien de zones ont ete sautes ;
- garder une posture stricte : `standard` reste fail-fast, `cautious` seul peut poursuivre avec zero-fill trace, et toute session degradee doit rester explicitement marquee comme partielle cote UI.

**Hypotheses pour cette tranche sick-disk imaging** :
- la meilleure premiere marche "disque malade" n'est pas un clone type `ddrescue` complet, mais une continuation read-only sure avec cartographie des trous de lecture ;
- l'image finale peut rester exploitable pour des analyses conservatives meme si certaines zones sont neutralisees a zero, a condition que ces trous soient traces et annonces ;
- les informations les plus utiles a exposer a court terme sont : nombre de plages illisibles, total d'octets neutralises, et quelques logs d'offsets, plutot qu'une interface complete de carte hex visuelle des erreurs ;
- cette tranche doit rester testable localement avec des lecteurs fautifs synthétiques, sans dependre d'un vrai disque defectueux.

**Risques pour cette tranche sick-disk imaging** :
- une continuation avec zero-fill peut donner une impression de succes si l'UI ne rappelle pas clairement que l'image contient des trous irrécupérables ;
- si les logs d'offsets sont trop verbeux, on peut polluer l'historique et les bundles de support ;
- il faut eviter qu'un scan catalogue classique soit automatiquement vendu comme "sick-disk aware" alors que seule l'imagerie image-backed est concernee ;
- les tests preview doivent rester honnetes et ne pas simuler un rescue materiel plus sophistique que ce qui existe vraiment.

**Modules impactes pour cette tranche sick-disk imaging** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/state.rs`, `src-tauri/src/types/mod.rs` ;
- frontend : `src/types/scan.ts`, `src/hooks/ipc/scan.ts`, `src/pages/ScanPage.tsx`, `src/pages/HistoryPage.tsx`, eventuellement `src/utils/browserPreviewSeed.ts`, i18n ;
- QA : tests Rust sur lecteurs fautifs / plages illisibles, et smoke E2E sur l'affichage d'une session d'imagerie degradee.

**Plan d'execution pour cette tranche sick-disk imaging** :
1. etendre le moteur d'imagerie pour distinguer `lecture ok`, `EOF` et `plage irrécupérable` en mode `cautious` ;
2. ecrire des octets nuls pour ces plages irrécupérables, accumuler un resume de session (`unreadable_ranges_count`, `unreadable_bytes`) et produire des logs techniques bornes ;
3. propager ces informations dans `ScanProgress`, `ScanSessionSummary`, les mappings IPC TS et l'historique UI ;
4. signaler clairement dans `ScanPage` et `HistoryPage` qu'une image a ete creee avec des zones illisibles neutralisees ;
5. ajouter des tests backend cibles et un smoke preview/E2E sur l'affichage degrade.

**Criteres de validation supplementaires pour cette tranche sick-disk imaging** :
- une imagerie `cautious` peut continuer malgre une lecture irrécupérable en zero-fillant uniquement la plage fautive et en poursuivant ensuite ;
- une imagerie `standard` continue d'echouer sur la meme erreur au lieu de masquer le probleme ;
- la progression et l'historique affichent combien de plages et combien d'octets ont ete neutralises ;
- les logs techniques permettent de retracer qu'une acquisition est partielle a cause de zones illisibles ;
- les validations existantes (`cargo test`, `npm run build`, `npm run test:ui`, `npm run test:e2e`) restent vertes.

**Limites connues de cette tranche sick-disk imaging** :
- ce n'est pas encore un cloneur multi-pass type `ddrescue` avec carte d'erreurs persistante et strategies de taille de bloc adaptatives ;
- les octets nuls dans l'image finale representent une neutralisation conservative, pas une reconstruction des donnees manquantes ;
- il n'y a pas encore de visualisation detaillee de carte de secteurs ni de reprise specialisee des zones illisibles en passes successives.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : l'imagerie `cautious` continue maintenant au-dela des plages irrécupérables en zero-fill borné, accumule `unreadable_ranges_count` / `unreadable_bytes`, journalise des offsets d'echantillon, et propage ces metriques jusque dans les sessions de scan et le helper privilegie macOS ;
- frontend : `ScanPage` et `HistoryPage` signalent explicitement les acquisitions degradees, avec badge, details et rappel honnete sur les trous source neutralises ;
- QA : deux tests Rust valident `cautious continue` vs `standard fail-fast`, et un smoke Playwright `History` couvre l'affichage d'une session d'imagerie degradee avec segments illisibles journalises ;
- validation complete : `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 imaging session incident report)** :
- permettre l'export d'un rapport texte natif pour une session d'imagerie locale, en particulier quand elle a repris une image partielle ou neutralise des zones source illisibles ;
- generer ce rapport cote backend a partir de l'historique persistant et des journaux techniques afin de garder une trace fidele, exportable et non dependante d'un assemblage fragile dans React ;
- exposer ce rapport depuis `HistoryPage` avec un flux desktop simple, tout en gardant un fallback preview honnete qui n'annonce pas de sauvegarde native indisponible ;
- rester strict sur la formulation : le rapport doit parler de zero-fill, d'octets neutralises et de trace technique, jamais de donnees "reparees" ou "reconstruites" physiquement.

**Hypotheses pour cette tranche imaging session incident report** :
- le besoin immediat n'est pas une carte hex graphique complete, mais un rapport texte fiable et partageable pour support, lab et audit ;
- les donnees deja persistantes (`ScanSessionSummary` + logs) suffisent pour une premiere version a haute valeur ;
- ce rapport doit etre disponible pour toutes les sessions d'imagerie, mais sa valeur est maximale quand une session a repris une image partielle ou rencontre des zones illisibles.

**Risques pour cette tranche imaging session incident report** :
- si le rapport omet les limites, il peut faire paraitre l'imagerie degradee plus "reussie" qu'elle ne l'est vraiment ;
- si l'export n'est disponible qu'en desktop, l'UI preview doit l'indiquer clairement sans laisser croire que le fichier a ete sauvegarde ;
- il faut eviter de dupliquer de la logique de formatage critique entre backend et frontend.

**Modules impactes pour cette tranche imaging session incident report** :
- backend : `src-tauri/src/commands/scan.rs`, `src-tauri/src/commands/export.rs` ou `src-tauri/src/lib.rs` selon le point d'exposition, plus tests associes ;
- frontend : `src/hooks/ipc/scan.ts` ou `src/hooks/ipc/export.ts`, `src/pages/HistoryPage.tsx`, i18n ;
- QA : tests Rust de generation de rapport et smoke Playwright sur le bouton d'export depuis l'historique.

**Criteres de validation supplementaires pour cette tranche imaging session incident report** :
- une session d'imagerie historique peut produire un rapport texte natif contenant au minimum identite de session, statut, octets copies, reprise eventuelle, compteurs de zones illisibles et journaux techniques ;
- le rapport rappelle explicitement que les octets zero-fill representent des donnees non lues et non reconstruites ;
- `HistoryPage` permet d'exporter ce rapport uniquement dans un flux coherent avec l'environnement courant ;
- les validations existantes (`cargo test`, `npm run build`, `npm run test:ui`, `npm run test:e2e`) restent vertes.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : `generate_imaging_session_report` genere maintenant un rapport texte natif a partir d'une session live ou persistante et de ses journaux techniques, avec resume de reprise, compteurs de zones illisibles et rappel explicite sur les octets zero-fill non reconstruits ;
- frontend : `HistoryPage` propose l'export du rapport pour les sessions d'imagerie pertinentes et signale honnetement la limite du runtime navigateur quand la sauvegarde native n'est pas disponible ;
- QA : deux tests Rust couvrent `rapport imaging ok` et `rejet non-imaging`, plus un smoke Playwright couvre le bouton d'export depuis l'historique en preview ;
- validation complete : `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 targeted rescue passes / ddrescue-like refinement)** :
- ajouter, en mode `cautious`, une ou plusieurs passes de reprise ciblees sur les plages zero-fill deja identifiees lors du premier balayage, avec granularite plus fine que la passe initiale ;
- reecrire localement dans l'image les sous-blocs finalement relus pendant ces passes de reprise, afin de reduire le volume final des trous non lus au lieu de conserver integralement le zero-fill large de premiere passe ;
- journaliser distinctement ce qui a ete recupere pendant les passes de reprise et ce qui reste irrécupérable a la fin, pour se rapprocher d'un comportement de type `ddrescue` sans pretendre reconstruire les octets jamais relus ;
- conserver une posture stricte : pas d'ecriture sur la source, pas de "magic reconstruction", et pas de promesse de parite complete avec `ddrescue` tant qu'il n'existe pas encore de mapfile persistante et de strategies adaptatives plus riches.

**Hypotheses pour cette tranche targeted rescue passes** :
- une premiere valeur forte est d'affiner les trous deja detectes avec des tailles de lecture plus petites (`1 KiB`, puis `512 B`) plutot que de rester sur un zero-fill large de `4 KiB` ;
- cette refinement pass peut etre appliquee juste apres la passe principale avec les abstractions `Read + Seek` deja presentes pour les sources brutes et virtuelles ;
- les journaux techniques et les rapports existants sont des surfaces suffisantes pour rendre visible le gain obtenu, sans devoir concevoir tout de suite une UI de carte d'erreurs complete.

**Risques pour cette tranche targeted rescue passes** :
- il faut eviter toute boucle de retries trop agressive qui rallongerait fortement l'imagerie sur un disque mourant ;
- un compteur final `unreadable_bytes` plus faible ne doit pas masquer qu'il y a eu des erreurs de lecture initiales ;
- l'algorithme doit rester lisible et testable, sinon on se rapproche d'une reimplementation opaque plutot que d'un moteur industriel traçable.

**Modules impactes pour cette tranche targeted rescue passes** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs` ;
- frontend : eventuellement aucun nouvel ecran, mais les journaux/rapports doivent remonter les octets raffines pendant les passes de reprise ;
- QA : tests Rust sur un lecteur fautif avec sous-plage relisible apres refinement, et validation complete.

**Criteres de validation supplementaires pour cette tranche targeted rescue passes** :
- une passe `cautious` peut remplacer localement une partie d'un large zero-fill initial quand des sous-blocs deviennent lisibles avec une granularite plus fine ;
- le compteur final `unreadable_bytes` correspond aux trous restants apres refinement, pas uniquement a la premiere estimation grossiere ;
- les journaux signalent explicitement les octets recuperes pendant les passes de reprise ciblees ;
- `cargo test`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : l'imagerie `cautious` execute maintenant des passes de reprise ciblees en sous-blocs (`1 KiB`, puis `512 B`) sur les plages zero-fill detectees pendant la premiere passe, et reecrit localement les octets finalement relus dans l'image ;
- moteur : `ImageArtifact` trace desormais les octets recuperes pendant refinement et le nombre de passes executees, tandis que le compteur final `unreadable_bytes` represente bien les trous restants apres refinement ;
- journaux : les sessions d'imagerie annoncent explicitement les passes de reprise ciblees et le volume d'octets recupere apres le balayage initial ;
- QA : un test Rust prouve qu'une grande zone zero-fill initiale peut etre reduite par les passes fines, en plus des tests existants `cautious continue` et `standard fail-fast` ;
- validation complete : `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 persistent rescue map / resumable targeted passes)** :
- enrichir le checkpoint local d'imagerie avec une vraie carte persistante des plages encore illisibles, ainsi que l'etat des passes de reprise deja executees ;
- permettre a une reprise ulterieure d'une image partielle coherente de reutiliser cette carte pour poursuivre les passes ciblees au lieu de repartir d'un simple resume global d'octets ;
- conserver une trace exacte de ce qui restait irrécupérable avant interruption et de ce qui a encore ete recupere apres redemarrage ;
- rester honnete : cette rescue map locale sert a reprendre les lectures ciblées, pas a “reconstruire” des secteurs jamais relus.

**Hypotheses pour cette tranche persistent rescue map** :
- la meilleure prochaine marche vers `ddrescue` est une map locale persistante exploitable par les reprises futures, meme si elle reste en JSON et non encore dans le format binaire historique de `ddrescue` ;
- le checkpoint actuel de `.partial` est deja le bon point d'ancrage pour stocker ces informations sans introduire un nouveau registre global ;
- une reprise est surtout utile quand l'application a ete interrompue apres ou pendant les passes de refinement, pas seulement pendant la copie lineaire initiale.

**Risques pour cette tranche persistent rescue map** :
- il faut eviter toute incoherence entre la taille du `.partial` et la carte des plages si le processus est interrompu brutalement ;
- plus le checkpoint devient riche, plus il faut rester tolerant aux anciens formats pour ne pas casser les resumes existants ;
- il ne faut pas laisser croire qu'une map JSON locale equivaut deja a toute la sophistication d'un `mapfile` `ddrescue`.

**Modules impactes pour cette tranche persistent rescue map** :
- backend : `src-tauri/src/imaging/mod.rs` principalement, plus journaux/tests si besoin ;
- QA : tests Rust sur reprise apres interruption avec plage illisible persistante et passes ciblees poursuivies apres redemarrage.

**Criteres de validation supplementaires pour cette tranche persistent rescue map** :
- une interruption sur une image partielle conserve les plages encore illisibles et l'etat des passes ciblees deja faites ;
- une reprise ulterieure reutilise cette carte persistante pour poursuivre le refinement au lieu de repartir comme si aucun trou n'avait ete catalogue ;
- les checkpoints existants plus anciens restent lisibles grace a des valeurs par defaut raisonnables ;
- `cargo test`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : le checkpoint `.partial` persiste maintenant non seulement le resume global d'octets, mais aussi la carte des plages encore illisibles et l'etat des passes ciblees deja executees ;
- reprise : une imagerie `cautious` redemarree peut reutiliser cette rescue map locale pour poursuivre les passes de refinement restantes au lieu de repartir comme si aucun trou n'avait deja ete catalogue ;
- compatibilite : les checkpoints plus anciens restent lisibles grace a des valeurs par defaut `serde`, ce qui preserve les resumes existants sans migration destructive ;
- QA : un nouveau test Rust couvre la reprise d'une image partielle deja complete avec une plage residuelle persistée et verifie que seule la suite utile des passes fines est rejouee ;
- validation complete : `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 ddrescue-style mapfile export / persisted session map)** :
- persister dans le resume de session d'imagerie les informations necessaires pour regenerer une carte de secours fidele apres completion, y compris les plages encore illisibles et les gains obtenus pendant les passes ciblees ;
- exposer un export texte natif d'une carte `ddrescue-style`, structuree comme un mapfile, afin de rendre l'etat final de l'imagerie interoperable, auditable et partageable hors de l'application ;
- afficher ces metriques dans l'historique desktop sans noyer l'utilisateur novice, et garder un fallback preview honnete quand l'export natif n'est pas disponible ;
- rester strict sur le vocabulaire : cette carte represente des blocs finis, non lus ou bad-sector selon l'etat observe, jamais des donnees "reconstruites" si la source n'a pas ete relue avec succes.

**Hypotheses pour cette tranche ddrescue-style mapfile export** :
- la prochaine marche la plus utile apres la rescue map persistante est de rendre cette carte exportable et conservable dans l'historique, pas seulement exploitable en reprise locale ;
- une compatibilite de structure `ddrescue-style` apporte une vraie valeur d'audit et d'interoperabilite meme si l'algorithme interne n'implemente pas encore toutes les phases adaptatives de `ddrescue` ;
- les sessions d'imagerie disposent deja des principaux compteurs ; il faut maintenant persister les plages residuelles elles-memes pour ne pas reduire le mapfile a des statistiques.

**Risques pour cette tranche ddrescue-style mapfile export** :
- il ne faut pas appeler "compatible ddrescue" un export qui ne respecterait pas au moins la structure textuelle et la semantique des statuts de bloc ;
- persister trop de details de carte dans l'historique peut gonfler les archives si on ne garde pas une representation compacte et fusionnee ;
- l'UI doit rendre visible la valeur du mapfile sans faire croire qu'il s'agit d'un outil magique de reconstruction.

**Modules impactes pour cette tranche ddrescue-style mapfile export** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/state.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/scan.rs`, `src-tauri/src/types/mod.rs`, `src-tauri/src/lib.rs` ;
- frontend : `src/types/scan.ts`, `src/hooks/ipc/scan.ts`, `src/pages/HistoryPage.tsx`, i18n et browser preview ;
- QA : tests Rust de generation de mapfile et smoke Playwright sur le bouton d'export mapfile depuis l'historique.

**Criteres de validation supplementaires pour cette tranche ddrescue-style mapfile export** :
- une session d'imagerie historisee conserve assez d'informations pour regenerer une carte de secours sans relire la source ;
- l'export produit un texte `ddrescue-style` avec ligne de statut et blocs contigus, marquant les zones finalisees en `+`, les trous zero-fill restants en `-`, et les zones non tentees en `?` quand la session ne couvre pas encore toute l'etendue connue ;
- l'historique desktop permet d'exporter cette carte et d'afficher les metriques associees, tandis que le navigateur preview annonce clairement la limite native ;
- `cargo test`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : les sessions d'imagerie persistent maintenant `total_bytes`, `rescued_after_retry_bytes`, `retry_passes_completed` et la liste complete des plages encore illisibles, ce qui permet de regenerer une carte fidele depuis l'historique sans relire la source ;
- export : `generate_imaging_rescue_map` produit un texte `ddrescue-style` avec commentaires de contexte, ligne de statut et blocs contigus `+` / `-` / `?` selon l'etat observe de la session ;
- frontend : `HistoryPage` affiche les nouvelles metriques d'imagerie et permet d'exporter a la fois le rapport d'incident et la rescue map, avec un fallback preview honnete quand le runtime natif n'est pas disponible ;
- QA : deux tests Rust couvrent la generation et le rejet du rescue map export, et le smoke Playwright `History` couvre le bouton `Export Rescue Map` en preview ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 bidirectional targeted rescue passes)** :
- rendre les passes ciblees de reprise sensibles a la direction, en alternant au moins une passe forward et une passe reverse sur les plages encore illisibles ;
- conserver une sequence de passes persistante et resumable, afin qu'une interruption n'efface pas le fait qu'une passe inverse a deja ete executee ;
- journaliser et rapporter ce comportement comme une strategie de relance prudente inspiree de `ddrescue`, sans vendre une parite complete ni une reconstruction magique ;
- rester compatibles avec les checkpoints existants en gardant un compteur de passes monotone et des valeurs par defaut tolerantes.

**Hypotheses pour cette tranche bidirectional targeted rescue passes** :
- l'inversion de direction entre passes est une des prochaines marches les plus concretes vers un comportement `ddrescue-like` qui augmente la recuperabilite reelle, pas seulement l'interoperabilite ;
- il suffit de faire varier l'ordre de parcours des chunks et des plages pour obtenir une implementation utile sans devoir introduire tout de suite un nouveau type de mapfile interne ;
- la structure actuelle `retry_passes_completed` peut porter cette evolution tant que la sequence de passes reste fixe et deterministic.

**Risques pour cette tranche bidirectional targeted rescue passes** :
- si l'ordre reverse est implemente de facon approximative, on peut annoncer une strategie bidirectionnelle sans reel effet sur les lectures ;
- les tests existants de refinement doivent rester stables meme si le nombre total de passes augmente ;
- il faut eviter de rendre l'algorithme opaque ; la sequence de passes doit rester explicite et traçable.

**Modules impactes pour cette tranche bidirectional targeted rescue passes** :
- backend : `src-tauri/src/imaging/mod.rs`, eventuellement les logs/rapports dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : tests Rust de refinement directionnel et validation complete.

**Criteres de validation supplementaires pour cette tranche bidirectional targeted rescue passes** :
- l'imagerie `cautious` alterne effectivement des passes ciblees forward et reverse sur les plages restantes ;
- une reprise avec checkpoint conserve correctement le numero de passe deja execute et ne rejoue pas toute la sequence depuis zero ;
- au moins un test Rust prouve qu'une passe reverse rescousse un sous-bloc qu'une sequence purement forward ne recupererait pas dans le meme plan de passes ;
- `cargo test`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-10** :
- tranche fermee ;
- backend : le moteur `cautious` execute maintenant une sequence de passes ciblees explicite et bidirectionnelle (`forward` puis `reverse` en `1 KiB`, puis `forward` et `reverse` en `512 B`) sur les plages encore illisibles ;
- reprise : `retry_passes_completed` continue a piloter la reprise de checkpoint sans migration destructive, ce qui permet de redemarrer au bon numero de passe meme avec la sequence directionnelle enrichie ;
- audit : les journaux et le rapport d'imagerie indiquent maintenant que les passes ciblees alternent la direction quand c'est pertinent, au lieu de laisser entendre une simple relance homogene ;
- QA : un nouveau test Rust prouve qu'un chunk differe n'est recupere que grace a la passe reverse, en plus des tests existants sur refinement, reprise et zero-fill ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 external rescue map import / prefix-compatible checkpoint seeding)** :
- accepter l'import d'une `rescue map` externe de type `ddrescue-style` pour reamorcer la reprise locale d'une image partielle deja existante, sans jamais pretendre que la carte seule suffit a recreer les donnees manquantes ;
- convertir cette carte importee en checkpoint `.partial.json` local exploitable par le moteur d'imagerie existant, en reutilisant les plages encore problematiques comme base des prochaines passes `cautious` ;
- borner explicitement la compatibilite a un sous-ensemble honnete : map contigue, blocs valides, et couverture prefix-compatible avec un fichier `.partial` local deja coherent ;
- exposer ce flux dans l'UI d'imagerie desktop avec un selecteur optionnel de mapfile et une microcopy qui rappelle les limites de compatibilite.

**Hypotheses pour cette tranche external rescue map import** :
- la marche la plus utile apres l'export de `rescue map` et les passes bidirectionnelles est de reutiliser une carte externe deja disponible, y compris un mapfile `ddrescue`, au lieu de repartir d'une reprise purement interne ;
- notre moteur ne sait reprendre de maniere sure qu'une image partielle de type prefixe contiguous ; il faut donc rejeter les mapfiles qui decrivent deja des zones copiees hors de ce prefixe plutot que d'annoncer une compatibilite plus large que la realite ;
- reinitialiser `retry_passes_completed` apres import externe est acceptable et plus honnete, car les numeros de passe `ddrescue` ne correspondent pas directement a notre sequence fixe de passes ciblees.

**Risques pour cette tranche external rescue map import** :
- il ne faut pas laisser croire qu'un mapfile seul permet de sauter des octets si le fichier `.partial` correspondant n'existe pas ou ne contient pas deja les donnees capturees ;
- un mapfile externe peut decrire des etats plus riches (`*`, `/`, `%`, directions de passe) que notre reprise locale ; il faut les normaliser prudemment ou les rejeter clairement ;
- un import mal valide pourrait fabriquer un checkpoint incoherent avec la source, la longueur attendue ou la taille reelle du fichier partiel.

**Modules impactes pour cette tranche external rescue map import** :
- backend : `src-tauri/src/imaging/mod.rs`, `src-tauri/src/commands/imaging_cmd.rs`, `src-tauri/src/lib.rs` et eventuellement les types/rapports si de nouvelles metriques d'import sont journalisees ;
- frontend : `src/types/scan.ts`, `src/hooks/ipc/imaging.ts`, `src/pages/ScanPage.tsx`, i18n et eventuel preview guard ;
- QA : tests Rust de parsing/normalisation/import de mapfile et validation complete applicative.

**Criteres de validation supplementaires pour cette tranche external rescue map import** :
- un mapfile `ddrescue-style` valide peut etre parse avec ses commentaires, sa ligne de statut et ses blocs contigus `?`, `*`, `/`, `-`, `+` ;
- l'import est refuse si la carte n'est pas contigue, si elle n'est pas prefix-compatible pour notre moteur, ou si aucun fichier `.partial` cohérent n'existe deja au chemin de destination ;
- une imagerie relancee avec une carte importee reutilise bien les plages residuelles du checkpoint seedé au lieu de redemarrer comme une copie vierge ;
- l'UI desktop permet de choisir, visualiser et retirer une `rescue map` optionnelle avant un lancement d'imagerie, avec un message clair sur la necessite d'une image partielle deja existante ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : l'imagerie sait maintenant parser une `rescue map` externe `ddrescue-style`, normaliser les statuts `?`, `*`, `/`, `-`, `+`, et convertir une carte prefix-compatible en checkpoint `.partial.json` local exploitable par la reprise existante ;
- surete : l'import est refuse si le mapfile n'est pas contigu, s'il decrit des blocs copies apres le premier bloc non tente, ou si le fichier `.partial` local correspondant n'existe pas / ne matche pas exactement le prefixe reutilisable ;
- reprise : `start_imaging` peut desormais reutiliser ce checkpoint seedé avant de relancer l'imagerie prudente, tout en reinitialisant honnetement le compteur interne de passes ciblees plutot que de pretendre mapper 1:1 les passes `ddrescue` ;
- frontend : `ScanPage` expose une rescue map optionnelle pour le workflow d'imagerie desktop, avec selection/retrait explicites et une microcopy qui rappelle qu'une carte seule ne reconstruit aucun octet manquant ;
- QA : trois nouveaux tests Rust couvrent le parsing des statuts de reprise, le seeding de checkpoint puis la reprise reelle, et le rejet des layouts hors prefixe ; la validation complete `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` est verte.

**Tranche suivante retenue (P2 sparse logical-length rescue-map reuse)** :
- etendre l'import externe pour accepter aussi des layouts `ddrescue` non strictement prefixes quand le fichier `.partial` local a deja sa longueur logique complete et peut donc etre repatche a des offsets arbitraires ;
- reutiliser dans ce cas tous les blocs non finalises du mapfile comme cibles de reprise `cautious`, y compris les anciens `?`, sans les presenter comme des octets deja recuperes ;
- continuer a rejeter explicitement les cas que notre moteur ne sait pas representer de facon sure, notamment les fichiers partiels trop courts par rapport a une carte sparse ou les couples carte/fichier qui divergent sur la longueur logique ;
- garder une communication honnete cote UI : prise en charge etendue des sorties sparse de type `ddrescue`, mais pas encore parite generale avec tous les workflows et heuristiques externes.

**Hypotheses pour cette tranche sparse logical-length rescue-map reuse** :
- un grand nombre de sorties `ddrescue` hors prefixe restent reutilisables de facon sure si le fichier cible local expose deja toute sa longueur logique, meme si certains blocs intermediaires sont encore vides ;
- notre moteur n'a pas besoin d'un nouveau format de destination pour ce palier : si la taille logique du `.partial` couvre deja tout le domaine du mapfile, les passes ciblees existantes peuvent repatcher les trous restants ;
- les blocs `?` importes dans ce mode ne doivent pas etre presentes comme "illisibles" a l'utilisateur avant nos propres tentatives, mais ils peuvent etre traites comme plages de reprise internes pour nos passes suivantes.

**Risques pour cette tranche sparse logical-length rescue-map reuse** :
- il ne faut pas confondre "fichier de longueur logique complete" et "fichier completement image" ; de nombreux octets peuvent encore rester vides ou zero-fill ;
- si on accepte un mapfile sparse alors que le `.partial` local est plus court que son domaine, on risque de fabriquer une reprise incoherente ;
- il faut garder des erreurs tres concretes pour que l'utilisateur comprenne pourquoi certains couples `mapfile + cible partielle` restent refuses.

**Modules impactes pour cette tranche sparse logical-length rescue-map reuse** :
- backend : `src-tauri/src/imaging/mod.rs` principalement ;
- frontend : microcopy dans `src/pages/ScanPage.tsx` et i18n si la borne de compatibilite visible change ;
- QA : nouveaux tests Rust d'import sparse et validation complete.

**Criteres de validation supplementaires pour cette tranche sparse logical-length rescue-map reuse** :
- un mapfile avec des blocs `+` apres le premier `?` peut etre reutilise si, et seulement si, le `.partial` local couvre deja toute la longueur logique de la carte ;
- dans ce mode, la reprise relance bien des passes ciblees sur tous les blocs non finalises sans redemarrer comme une copie vierge ;
- les couples incoherents `mapfile sparse + partial trop court` restent refuses avec un message explicite ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : l'import externe accepte maintenant non seulement les cartes prefix-compatibles, mais aussi les sorties `ddrescue` sparse / hors ordre lorsque le `.partial` local a deja la longueur logique complete du domaine mappe ;
- reprise : dans ce mode, Recupere reutilise tous les blocs non finalises du mapfile comme cibles de passes `cautious`, sans repasser par une copie append-only fictive ;
- surete : les couples `mapfile sparse + partial trop court` ou divergents restent rejetes explicitement, ce qui evite d'annoncer une compatibilite trop large avec des cibles non representables localement ;
- frontend : la microcopy du choix de rescue map annonce maintenant cette compatibilite etendue tout en rappelant que la carte ne reconstruit pas les octets a elle seule ;
- QA : un nouveau test Rust couvre l'acceptation d'un layout sparse logique complet, en plus du rejet des partials trop courts ; la validation complete `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` est verte.

**Tranche suivante retenue (P2 adaptive trim/scrape retry passes)** :
- enrichir la sequence des passes `cautious` avec des strategies plus adaptatives, en ajoutant au moins des passes de trimming par bords et des blocs plus fins sur les plages residuelles ;
- conserver une sequence deterministe et resumable via `retry_passes_completed`, afin qu'une interruption ne perde pas l'etat de progression des relances fines ;
- journaliser cette evolution comme une strategie de reprise plus proche d'un schema `ddrescue` de trimming/scraping, sans annoncer une equivalence complete avec toutes ses heuristiques ;
- garder le moteur strictement read-only sur la source, et ne jamais presenter les octets encore manquants comme "reconstruits".

**Hypotheses pour cette tranche adaptive trim/scrape retry passes** :
- apres les reprises directionnelles et la reutilisation des mapfiles externes, la marche la plus utile est de mieux exploiter les bords des plages encore en echec et de descendre plus finement en taille de bloc ;
- une sequence fixe de passes `edge-trim` puis `fine scrape` reste compatible avec le compteur monotone `retry_passes_completed` et evite d'introduire un ordonnanceur opaque ;
- un test synthétique bien choisi peut montrer un vrai gain réel là ou les passes actuelles 1024/512 echouent encore.

**Risques pour cette tranche adaptive trim/scrape retry passes** :
- il ne faut pas transformer la sequence de reprise en boite noire difficile a auditer ;
- des passes trop nombreuses ou trop fines peuvent allonger la phase de reprise sans benefice si elles ne sont pas ciblees ;
- les attentes de tests existants sur le nombre de passes executees devront etre mises a jour avec soin.

**Modules impactes pour cette tranche adaptive trim/scrape retry passes** :
- backend : `src-tauri/src/imaging/mod.rs`, plus les journaux/rapports dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : nouveaux tests Rust de trimming/scraping et validation complete.

**Criteres de validation supplementaires pour cette tranche adaptive trim/scrape retry passes** :
- la sequence `cautious` inclut des passes plus fines et au moins une strategie de trimming par bords sur les plages residuelles ;
- `retry_passes_completed` continue a permettre une reprise propre sans rejouer toute la sequence ;
- au moins un test Rust prouve qu'une plage encore irreductible avec les passes 1024/512 peut etre partiellement ou totalement reduite par les nouvelles passes adaptatives ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Tranche suivante retenue (P2 center-out island scraping passes)** :
- ajouter une passe deterministe de probing center-out sur les plages encore residuelles, afin de recuperer des ilots lisibles au milieu d'une zone qui reste opaque en parcours sequentiel et edge-trim ;
- conserver une sequence fixe, resumable et auditable via `retry_passes_completed`, sans introduire d'heuristique aleatoire ni d'ordonnanceur opaque ;
- journaliser cette evolution comme une strategie de `scraping` plus proche des moteurs de type `ddrescue`, tout en rappelant qu'elle ne "reconstruit" jamais les octets qui n'ont pas ete relus ;
- laisser les segments recuperes au milieu d'une plage se repropager naturellement en sous-plages residuelles, pour que les passes suivantes travaillent sur des zones deja reduites.

**Hypotheses pour cette tranche center-out island scraping passes** :
- apres les passes forward/reverse, edge-trim et fine scrape, le prochain gain concret vient de la capacite a decouvrir des ilots lisibles au milieu des plages restantes au lieu de ne travailler que par bords ;
- une passe `center-out` a taille de bloc fixe reste comprehensible, traçable et compatible avec notre mecanisme de reprise monotone ;
- un test synthetique peut demontrer un vrai gain la ou les passes sequentielles et edge-trim echouent encore integralement sur le coeur d'une plage.

**Risques pour cette tranche center-out island scraping passes** :
- une implementation trop agressive pourrait multiplier les seeks sans benefice si elle n'est pas strictement bornee a la fin de la sequence `cautious` ;
- il faut eviter d'annoncer une sophistication "type ddrescue" si la passe n'apporte pas un gain prouve sur un cas realiste ;
- les journaux doivent rester lisibles et ne pas noyer l'utilisateur sous des details de micro-strategie.

**Modules impactes pour cette tranche center-out island scraping passes** :
- backend : `src-tauri/src/imaging/mod.rs`, plus les journaux/rapports dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : nouveaux tests Rust couvrant la decouverte d'un ilot lisible central et validation complete.

**Criteres de validation supplementaires pour cette tranche center-out island scraping passes** :
- la sequence `cautious` inclut au moins une passe `center-out` apres les passes plus grossieres ;
- une plage encore totalement illisible apres les passes forward/reverse/edge-trim peut etre reduite quand un ilot central devient lisible a la bonne granularite ;
- `retry_passes_completed` continue a permettre une reprise propre avec la nouvelle sequence fixe ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : la sequence `cautious` inclut maintenant une passe finale `center-out` en blocs de `64 B`, qui sonde le milieu des plages residuelles pour recuperer des ilots lisibles que les parcours forward/reverse et edge-trim ne trouvaient pas ;
- reprise : `retry_passes_completed` reste monotone et resumable malgre l'allongement de la sequence, ce qui conserve la compatibilite des checkpoints `.partial.json` existants ;
- audit : les journaux et le rapport d'imagerie indiquent maintenant explicitement l'alternance directionnelle, le trimming par bords, le probing d'ilots centraux et le scraping fin, sans promettre de reconstruction magique ;
- QA : un nouveau test Rust synthetique prouve qu'un ilot lisible central est recupere au milieu d'une plage encore opaque jusque-la, et les assertions existantes sur les comptes de passes ont ete realignees proprement ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 small-gap prioritized micro-scrape scheduler)** :
- etendre la sequence `cautious` avec des micro-passes fixes `32 B` puis `16 B`, afin d'aller chercher des octets encore lisibles a une granularite trop fine pour la passe `center-out` en `64 B` ;
- introduire un ordre explicite des plages par passe, avec priorisation des plus petits trous residuels en fin de sequence pour mieux exploiter les zones deja reduites ;
- conserver un ordonnanceur deterministe, auditable et resumable via `retry_passes_completed`, sans basculer vers une heuristique opaque ou aleatoire ;
- continuer a documenter honnetement qu'il s'agit d'une relance de lecture plus agressive sur les residuels, et non d'une recreation magique des donnees.

**Hypotheses pour cette tranche small-gap prioritized micro-scrape scheduler** :
- apres le `center-out` en `64 B`, il reste des cas realistes ou seule une granularite `32 B` ou `16 B` permet de relire une petite zone stable ;
- donner la priorite aux petits trous residuels en fin de chaine augmente les chances de completer des plages presque refermees avant de rebalayer de grandes zones toujours opaques ;
- on peut garder une sequence courte, fixe et comprehensible tout en ajoutant ce gain de finesse.

**Risques pour cette tranche small-gap prioritized micro-scrape scheduler** :
- trop de micro-passes peuvent faire exploser le nombre de seeks si elles ne sont pas reservees a la fin du pipeline ;
- il ne faut pas faire croire que cette finesse equivaut deja a toute la sophistication d'un `ddrescue` complet avec politiques de temps, split adaptatif et reprise externe exhaustive ;
- les tests existants avec comptage exact de passes devront etre reajustes proprement.

**Modules impactes pour cette tranche small-gap prioritized micro-scrape scheduler** :
- backend : `src-tauri/src/imaging/mod.rs`, plus les messages d'audit dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : nouveaux tests Rust de micro-scrape et validation complete.

**Criteres de validation supplementaires pour cette tranche small-gap prioritized micro-scrape scheduler** :
- la sequence `cautious` inclut des micro-passes `32 B` puis `16 B` a la fin de la chaine ;
- les passes fines de fin de sequence peuvent ordonner les plages par taille croissante au lieu de conserver uniquement l'ordre source ;
- au moins un test Rust prouve qu'un sous-bloc lisible uniquement en `32 B` ou `16 B` est maintenant recupere alors qu'il restait perdu avec la sequence precedente ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : le moteur `cautious` execute maintenant une sequence fixe enrichie jusqu'aux micro-passes `32 B` puis `16 B`, avec priorisation des plus petits trous residuels sur les phases fines au lieu d'un simple balayage uniforme ;
- performance : les deux passes `1024 B` conservent le comportement de retry prudent complet, mais les passes fines suivantes fonctionnent en mode `probe` rapide, ce qui rend le micro-scrape exploitable sans rallonger artificiellement chaque session ;
- audit : les logs et le rapport d'imagerie annoncent explicitement l'alternance directionnelle, le trimming, le probing central, la priorisation des petits residuels et le micro-scrape fin ;
- QA : un nouveau test Rust synthetique prouve qu'un chunk lisible uniquement en `32 B` est maintenant recupere, et toute la suite d'imagerie synthetique reste verte avec les nouveaux comptes de passes ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 local neighbor-zoom follow-up scheduler)** :
- lorsqu'un chunk est relu avec succes pendant une passe fine, lancer immediatement un zoom local deterministe sur ses voisins les plus probables au lieu d'attendre la passe globale suivante ;
- conserver un comportement strictement resumable et auditif : le zoom local doit rester borne, previsible et compatible avec `retry_passes_completed` ;
- exploiter ce zoom surtout sur les passes fines de type `center-out` et micro-scrape, ou une petite poche lisible peut indiquer qu'un voisin immediat est aussi recuperable ;
- rester honnete dans la communication : il s'agit d'une meilleure exploitation de succes de lecture reels, pas d'une recreation artificielle des octets manquants.

**Hypotheses pour cette tranche local neighbor-zoom follow-up scheduler** :
- lorsqu'une poche lisible reapparait dans une plage degradee, ses voisins immediats sont souvent les candidats les plus utiles a sonder avant de repartir sur des zones plus lointaines ;
- un zoom local borne autour d'un succes peut apporter un vrai gain de recuperation sans introduire de boucle non deterministe ;
- cette strategie est plus proche d'un comportement de rescue adaptatif que la seule repetition de passes globales fixes.

**Risques pour cette tranche local neighbor-zoom follow-up scheduler** :
- il faut eviter de reprocesser indefiniment les memes zones avec des zooms qui se chevauchent ;
- le zoom local ne doit pas faire exploser le nombre de seeks ni complexifier a l'exces les checkpoints de reprise ;
- le comportement doit rester suffisamment simple pour etre testable sur des readers synthetiques.

**Modules impactes pour cette tranche local neighbor-zoom follow-up scheduler** :
- backend : `src-tauri/src/imaging/mod.rs` principalement, plus l'audit dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : nouveau test Rust demontrant que des chunks voisins ne sont recuperes que grace au zoom immediat.

**Criteres de validation supplementaires pour cette tranche local neighbor-zoom follow-up scheduler** :
- au moins une passe fine programme un zoom immediat sur les voisins d'un chunk relu avec succes ;
- le zoom est borne et deterministe, sans casser la sequence fixe de passes ni `retry_passes_completed` ;
- un test Rust prouve qu'un voisin lisible temporairement apres un premier succes est bien capture grace au zoom local alors qu'il serait autrement perdu ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : les passes fines `64 B` puis `32 B` savent maintenant lancer un zoom local borne sur les voisins immediats d'un chunk relu avec succes, au lieu d'attendre la prochaine passe globale sur toute la plage ;
- ordonnancement : le zoom local reste deterministe et resumable, et il est volontairement borne aux chunks pivots de la passe pour eviter une cascade infinie de sous-zooms ;
- performance/surete : le moteur ne multiplie pas aveuglement les retries ; il reordonne localement autour d'un succes reel sans changer la posture read-only ni casser `retry_passes_completed` ;
- audit : les journaux et le rapport d'imagerie annoncent maintenant explicitement le zoom autour des poches fraichement recuperees en plus du trimming, du probing central et du micro-scrape ;
- QA : un nouveau test Rust synthetique prouve que deux voisins `32 B` ne sont recuperes que si le moteur les sonde immediatement apres un succes `64 B`, ce qui valide le scheduler local ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

**Tranche suivante retenue (P2 adaptive local split after progress)** :
- lorsqu'une progression reelle apparait dans une plage residuelle, decouper localement les sous-plages adjacentes encore opaques en morceaux plus fins au lieu d'attendre le passage global suivant ;
- garder ce split strictement borne, deterministe et attaché a la passe courante, afin de ne pas casser `retry_passes_completed` ni transformer le moteur en boucle opaque ;
- reserver ce comportement aux passes fines ou une poche relue est un signal utile, pas aux grosses passes initiales ;
- continuer a documenter honnetement qu'il s'agit d'un raffinement de lecture adaptatif sur des donnees effectivement relues, jamais d'une reconstruction artificielle.

**Hypotheses pour cette tranche adaptive local split after progress** :
- lorsqu'une passe fine relit une poche utile, les sous-plages immediatement adjacentes peuvent devenir recuperables a une granularite encore plus fine sans attendre la passe suivante ;
- un split local borne autour d'une progression concrete peut augmenter la recuperation utile tout en restant comprehensible et testable ;
- cette strategie rapproche le moteur d'un comportement de rescue adaptatif plus efficace sur les zones instables que la seule succession de passes fixes.

**Risques pour cette tranche adaptive local split after progress** :
- il faut eviter un arbre de split explosif qui ferait diverger le nombre de seeks ;
- les splits immediats ne doivent pas dupliquer inutilement des chunks deja planifies par le zoom voisin ou les passes suivantes ;
- le comportement doit rester simple enough pour etre prouve par des readers synthetiques et audite dans les logs.

**Modules impactes pour cette tranche adaptive local split after progress** :
- backend : `src-tauri/src/imaging/mod.rs` principalement, plus les messages d'audit dans `src-tauri/src/commands/mod.rs` et `src-tauri/src/commands/scan.rs` ;
- QA : nouveau test Rust demontrant qu'une sous-plage n'est recuperee qu'apres split adaptatif immediat.

**Criteres de validation supplementaires pour cette tranche adaptive local split after progress** :
- au moins une passe fine sait planifier un split local immediat plus fin apres un succes sur un chunk pivot ;
- le split est borne et deterministe, sans casser la sequence globale ni `retry_passes_completed` ;
- un test Rust prouve qu'une sous-plage n'est recuperee que grace a ce split adaptatif immediat alors qu'elle resterait perdue en attendant la passe globale suivante ;
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` restent verts.

**Statut 2026-04-11** :
- tranche fermee ;
- backend : lorsqu'une passe fine obtient une lecture partielle utile sur un chunk pivot, le moteur sait maintenant splitter immediatement la queue residuelle en sous-chunks plus fins au lieu d'attendre la passe globale suivante ;
- ordonnancement : ce split adaptatif reste borne a la passe courante, deterministic et compatible avec `retry_passes_completed`, sans arbre de subdivision non borne ;
- interaction avec le zoom local : le scheduler combine maintenant trois signaux utiles sur les passes fines, a savoir le succes plein (zoom voisins), le succes partiel (split local de la queue residuelle) et la priorisation des petits trous restants ;
- audit : les journaux et le rapport d'imagerie indiquent explicitement que le moteur peut convertir une progression partielle en retries locaux plus fins, en plus du trimming, du probing central, du zoom local et du micro-scrape ;
- QA : un nouveau test Rust synthetique prouve qu'une lecture `32 B` ne rend d'abord que `16 B`, puis que les `16 B` restants ne sont recuperes que grace au split adaptatif immediat ;
- validation complete : `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, `npm run test:ui` et `npm run test:e2e` sont verts.

## Chantier 79 — Passage au plus haut panier
**Objectif** : Structurer le plan de montée en gamme de Récupère jusqu'à un niveau top-tier comparable aux meilleures suites desktop de récupération, sans sacrifier la sûreté, l'honnêteté produit et la traçabilité.

**Pourquoi** :
- l'audit marché du `2026-04-11` montre que le produit est déjà fort en sûreté et architecture, mais encore en dessous des leaders sur la preuve publique, le bootable rescue, l'imagerie instable et les workflows de stockage avancé ;
- ce chantier donne une feuille de route unique, priorisée et suivable pour fermer les écarts structurants avant d'ajouter des raffinements secondaires ;
- il crée un cadre clair pour les prochaines tranches multi-modules.

**Périmètre** :
- couvert : benchmark public, mode bootable, imagerie support instable, RAID/NAS/VM, APFS/chiffrement, durcissement cross-platform, reporting premium, docs/QA/release readiness ;
- non couvert : promesses marketing non prouvées, cloud imposé, "magic recovery", refonte monolithique globale ;
- dépendances : `docs/benchmark-market.md`, audit comparatif du `2026-04-11`, modules Rust/Tauri/React concernés ;
- hypothèses : le différenciateur principal de Récupère restera la sûreté prouvée, l'image-first discipline et l'auditabilité, pas un discours IA plus agressif.

**Contraintes** :
- conserver la lecture seule stricte sur la source ;
- ne jamais masquer les limites encore ouvertes ;
- exiger des preuves de validation avant toute revendication de parité marché ;
- préserver l'équilibre novice / expert.

**Architecture concernée** :
- core low-level ;
- imaging ;
- analyzers ;
- raid / virtual disk / encryption ;
- preview / export / audit ;
- desktop app ;
- QA / CI / docs.

**Document de pilotage** :
- voir `docs/top-tier-roadmap.md`

**Critères de done** :
- la roadmap contient les chantiers prioritaires, leurs dépendances, critères de validation et garde-fous ;
- elle est suffisamment actionnable pour servir de backlog d'exécution ;
- elle devient le point de suivi principal pour les futures tranches de montée en gamme.

**Statut 2026-04-11** :
- chantier ouvert ;
- document de pilotage cree dans `docs/top-tier-roadmap.md` ;
- premiere tranche recommandee : `TT-01 Benchmark public reproductible`.

## Chantier 80 — TT-01 Benchmark public reproductible
**Objectif** : Construire la premiere infrastructure benchmark vraiment exploitable du repo, avec protocole, corpus versionne, validation automatique du manifeste et format standard pour enregistrer les resultats de Récupère et des concurrents.

**Pourquoi** :
- le plus gros manque face aux leaders reste l'absence de preuve publique et reproductible ;
- il faut un cadre concret avant de comparer Récupère a des outils vraiment accessibles aux utilisateurs, puis aux suites plus avancees en evidence bonus ;
- une structure versionnee evitera les benchmarks ad hoc impossibles a maintenir ou a verifier.

**Hypotheses** :
- la premiere tranche du benchmark sera hybride : scenarios `ready-in-repo` bases sur les generateurs synthétiques existants, plus scenarios `public-artifact-pending` deja references mais pas encore distribues comme fixtures publiques ;
- les runs concurrents resteront manuels au debut, mais devront utiliser les memes identifiants de scenarios et les memes attentes d'evidence ;
- la meilleure premiere avancee n'est pas d'automatiser tous les runs, mais de figer un protocole, un corpus et un format de resultat.

**Risques** :
- decrire un corpus trop abstrait sans chemin realiste vers des resultats concrets ;
- melanger preuves de tests internes et vraies preuves de benchmark public ;
- laisser des ids ou des regles evoluer apres l'apparition des premiers resultats, ce qui casserait la reproductibilite ;
- publier trop tot des conclusions alors que des scenarios critiques sont encore absents ou partiels.

**Modules impactes** :
- `docs/`
- nouveau dossier `benchmarks/`
- `scripts/`
- `package.json`
- `.github/workflows/ci.yml`
- potentiellement `README.md` et les commandes backend dans les tranches suivantes.

**Plan d'execution** :
1. definir le protocole benchmark v1 et la structure du workspace `benchmarks/` ;
2. creer un manifeste de corpus versionne avec scenarios, readiness, evidence et attentes minimales ;
3. ajouter un script de validation automatique du manifeste et un template standard de resultats ;
4. brancher une verification CI legere pour eviter la derive du corpus ;
5. preparer la tranche suivante : premiers resultats Récupère et promotion de fixtures vers de vrais artefacts benchmark.

**Critères de validation** :
- un document protocole v1 existe ;
- un manifeste versionne du corpus existe et est valide automatiquement ;
- un template de resultats standard peut etre genere a partir du manifeste ;
- la CI verifie le manifeste ;
- les scenarios de depart couvrent au moins delete, lost-volume, carving et unstable-media, meme si certains restent `public-artifact-pending`.

**Limites connues** :
- cette tranche ne livre pas encore de benchmark public final ;
- la plupart des fixtures initiales restent des recettes de generation in-repo et non des images benchmark redistribuables ;
- les runs concurrents ne sont pas automatises dans cette premiere passe.

**Statut 2026-04-11** :
- chantier ouvert ;
- structure `benchmarks/` initialisee ;
- protocole `v1` ajoute ;
- manifeste de corpus `v1` ajoute ;
- script de validation et generation de template ajoute ;
- check CI leger ajoute ;
- premier fichier de resultats `Récupère` baseline interne ajoute dans `benchmarks/results/2026-04-12-recupere-internal-baseline.json` ;
- validation locale confirmee via `npm run benchmark:check` ;
- point de vigilance actuel : un scenario `P0` du corpus (`apfs_deleted_orphan_catalog_v1`) reste encore `not-run` dans ce baseline faute de preuve benchmark dediee ;
- tranche suivante attendue : combler le trou APFS P0 et promouvoir les fixtures les plus importantes vers de vrais artefacts benchmark.

**Statut 2026-04-23** :
- protocole public realigne pour ne plus exiger d'outil payant comme prerequis de cloture initiale ;
- exporteur de fixtures `P0` `ready-in-repo` ajoute cote tests Rust pour produire des images comparables hors suite de tests ;
- premiere campagne comparative accessible executee de bout en bout avec `PhotoRec 7.2` et `TestDisk 7.2` sur les scenarios `ready-in-repo` deja promus ;
- fichiers de resultats ajoutes dans `benchmarks/results/2026-04-23-photorec-7.2-accessible-p0.json` et `benchmarks/results/2026-04-23-testdisk-7.2-accessible-p0.json` ;
- preuve bonus `DMDE 4.4.6` conservee sur `signature_carving_jpeg_v1`, tandis que `R-Studio` reste archive comme note operateur non revendiquee ;
- report HTML regenere avec `6` runs visibles ;
- benchmark `TT-01` reste `partial` car le trou `APFS` `P0` persiste et la preuve marche reste encore etroite.

## Chantier 81 — Professionnalisation post-audit P0
**Objectif** : Fermer le lot de blocages P0 issus de l'audit du 17 avril 2026 afin que le repo redevienne reproductible depuis un checkout propre, que la couverture qualite soit honnete, et que les flux desktop critiques ne reposent plus sur des hypotheses web ou Unix-only.

**Pourquoi** :
- l'audit a confirme que la base Rust/Tauri est deja serieuse, mais plusieurs ecarts empechent encore de parler d'un niveau "professionnel plus" ;
- le produit opere dans un domaine a fortes contraintes de surete, donc un repo non reproductible ou une couverture surestimee degrade directement la confiance ;
- il faut fermer ce lot avant d'ajouter de nouvelles capacites de recovery ou de communication produit.

**Perimetre** :
- couvert : resynchronisation `package.json` / `package-lock.json`, retour d'un `npm ci` propre, remise en etat de `biome`, alignement entre routes, e2e, README et CI, garde-fous browser-preview pour les IPC remote, remplacement des `window.prompt` / `window.confirm` et des chemins Unix codes en dur dans les flux remote critiques, i18n des chaines visibles sur ces parcours ;
- non couvert : nouveaux moteurs de recuperation, benchmark public TT-01, refonte large de `commands/mod.rs`, migration complete de tous les `lock().expect(...)`, suite native WebDriver complete via `tauri-driver` ;
- dependances : `package.json`, `package-lock.json`, `.github/workflows/ci.yml`, `playwright.config.ts`, `README.md`, `docs/testing.md`, `src/router.tsx`, `src/hooks/ipc/remote.ts`, `src/components/device/RemoteAgentsSection.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx`, composants et traductions associes ;
- hypotheses : la voie la plus rapide vers un niveau professionnel consiste a durcir les garde-fous et la verite produit avant toute nouvelle sophistication fonctionnelle.

**Contraintes** :
- ne jamais degrader la posture read-only ni introduire de flux ambigu sur le disque source ;
- ne jamais gonfler artificiellement la couverture qualite ou appeler "desktop e2e" un simple browser preview ;
- supprimer les hypotheses Unix-only sur les parcours desktop utilisateur ;
- garder une UX novice calme, localisee et explicite sur ce qui est local, distant, simule ou natif.

**Modules impactes** :
- supply chain / release : `package.json`, `package-lock.json`, scripts et workflows GitHub ;
- QA / docs : `README.md`, `docs/testing.md`, `playwright.config.ts`, specs e2e ciblees ;
- desktop app : `src/router.tsx`, pages `Devices`, `Results`, `Export`, composants remote et logs ;
- IPC frontend : `src/hooks/ipc/remote.ts` et dispatchers associes ;
- backend desktop : commandes remote si le contrat de destination locale ou les flux de telechargement doivent etre raffermis.

**Contrats et interfaces** :
- `npm ci` doit reussir depuis un checkout propre, sans install manuelle hors lockfile ;
- `npm run lint` et `npm run format:check` doivent etre executables localement et en CI avec les memes dependances ;
- la page `Devices` en browser preview ne doit jamais appeler un `invoke` Tauri brut sans garde `isTauri()` ou fallback explicite ;
- les tests Playwright doivent rester presentes comme couverture browser-preview, pas comme verite native Tauri ;
- les parcours rapport / CSV / pull distant doivent utiliser des interactions desktop maitrisees, pas des `prompt` navigateur et pas de chemins `/tmp` ou `/root` codes en dur ;
- le parcours de restauration distante doit passer par une UI explicite et validee, pas une invite bloquante implicite ;
- les routes et les tests de paywall/export doivent refleter la vraie decision produit, sans contradiction entre navigation et attentes e2e.

**Risques** :
- resynchroniser le lockfile et durcir la CI peut faire ressortir plusieurs regressions cachees d'un coup ;
- remplacer les invites navigateur par des flux desktop natifs peut toucher plusieurs parcours results/export et casser des tests existants ;
- trop durcir trop vite le smoke natif peut rendre la CI rouge avant que les faux positifs restants soient traites ;
- une mauvaise decision sur la semantics de `/export` peut recreer un ecart entre UX, tests et promesse produit.

**Questions ouvertes** :
- faut-il conserver `/export` strictement derriere un scan actif, ou autoriser un ecran upsell autonome ?
- recommandation : conserver le garde route actuel et rendre le test paywall coherent avec un scan seede et une selection exportable ;
- faut-il rendre le smoke natif macOS bloquant dans ce lot, ou au lot suivant une fois le passage vert constate ?
- recommandation : corriger le parcours et rendre le smoke bloquant des qu'un run CI stable est observe, sans laisser `continue-on-error` plus longtemps que necessaire.

**Plan d'execution** :
1. resynchroniser la toolchain Node et le lockfile (`biome`, `npm ci`, scripts de qualite) ;
2. realigner la verite de couverture entre routes, tests, README, docs testing et politique CI ;
3. ajouter les garde-fous browser-preview manquants pour le module remote afin que `/devices` et les ecrans associes restent stables hors Tauri ;
4. remplacer les chemins Unix codes en dur et les invites navigateur des flux remote par des parcours desktop maitrises, localises et testables ;
5. redefinir le gate de "niveau professionnel" de ce lot et documenter explicitement ce qui reste hors perimetre.

**Tests et validation** :
- `npm ci` reussit ;
- `npm run lint` reussit ;
- `npm run format:check` reussit ;
- `npm run test:ui` reussit ;
- `npm run test:e2e` reussit, y compris le spec paywall ;
- `npm run build` reussit ;
- `npm run release:preflight` reussit ;
- `cargo check --manifest-path src-tauri/Cargo.toml` reussit ;
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` reussit ;
- la route `/devices` ne remonte plus d'erreur `invoke` en browser preview ;
- les parcours remote critiques ne contiennent plus de `window.prompt`, `window.confirm`, `/tmp` ni `/root` codes en dur cote frontend.

**Limites connues** :
- meme apres ce lot, le repo n'aura pas encore une vraie suite native interactive Tauri de type `tauri-driver` ;
- ce lot ne transforme pas encore Récupère en produit benchmarke publiquement face au marche ;
- la dette structurelle restante autour des gros modules Rust et des `lock().expect(...)` reste a traiter dans des chantiers dedies.

**Statut 2026-04-17** :
- chantier ouvert ;
- plan cree a la suite de l'audit complet du repo ;
- ordre recommande : reproductibilite Node/CI, verite tests/docs, garde-fous browser-preview remote, flux desktop remote, puis durcissement final du gate CI.
- avancement courant :
- `npm ci`, `npm run lint`, `npm run format:check`, `npm run build`, `npm run test:ui`, `npm run test:e2e`, `cargo check` et `cargo test --lib` sont revenus au vert localement ;
- les gardes browser-preview du module remote, les flux remote desktop critiques et l'i18n associee ont ete corriges ;
- le dernier blocage P0 identifie sur le gate natif etait un oubli de branchement de `RECUPERE_LICENSE_PUBKEY_HEX` dans les workflows de release et de smoke, plus l'absence de fail-fast dans `release-preflight` ;
- correctif engage : preflight bundle strict sur la cle publique de licence, injection workflow harmonisee, puis rerun du smoke natif avec cle non-placeholder pour valider le passage en bloquant ;
- validation locale obtenue : bundle macOS produit avec une cle publique de test non-placeholder, puis `node scripts/native-smoke.mjs --timeout-ms 15000 --fail-on-crash` vert ;
- decision de lot : le smoke natif macOS repasse bloquant dans `.github/workflows/ci.yml`, le reliquat principal restant hors perimetre etant la future vraie suite interactive `tauri-driver`.

## Chantier 82 — Couche de memoire filesystem et snapshots temporels ✅ TERMINÉ (Sprint 4 + Sprint 5 B6, 2026-04-17/18 — scheduler passif + 2 cmds + UI SettingsPage + i18n FR/EN)
**Objectif** : Ajouter la couche d'architecture manquante de memoire filesystem, capable d'indexer des chemins ou volumes selectionnes dans le temps, de stocker des snapshots locaux, de detecter les changements et d'enrichir la recuperation avec le dernier contexte connu des fichiers.

**Pourquoi** :
- cette couche manque aujourd'hui entre le moteur de recuperation et l'assistance produit ;
- elle permet d'expliquer ce qui a disparu, quand, ou, et avec quel niveau de confiance ;
- elle ajoute de la valeur sans promettre de "magic recovery" ni reposer sur le cloud ;
- elle correspond au besoin formule dans `prompt_claude_code_recuperation-1.md`.

**Perimetre** :
- couvert : indexation locale, snapshots multiples, comparaison d'etats, classification disparition / deplacement / renommage, vue des fichiers disparus, score contextuel de recuperabilite, integration conservative avec les resultats de recovery existants, journalisation et planification passive ;
- non couvert : reecriture du moteur de recuperation, restauration automatique sur le disque source, inference opaque non tracable, interface graphique complexe dans la premiere tranche ;
- dependances : `core`, `analyzers`, `scoring`, `audit`, `history`, `desktop app`, types partages Rust/TypeScript ;
- hypotheses : la premiere valeur vient d'une indexation de chemins et volumes montes selectionnes explicitement par l'utilisateur, avant toute surveillance temps reel agressive.

**Contraintes** :
- ne jamais ecrire sur le disque source dans le cadre de cette couche ;
- toute persistance doit aller dans une base locale dediee, hors source ;
- ne jamais presenter un fichier "disparu" comme "supprime avec certitude" sans niveau de confiance ;
- l'ancien emplacement doit servir de contexte et de destination relative reconstruite sur un support sur, jamais de permission implicite pour restaurer sur la source ;
- fonctionnement offline, base locale legere, code modulaire, logs tracables.

**Architecture concernee** :
- nouvelle couche `filesystem memory / snapshot index` ;
- store local de metadonnees et snapshots ;
- moteur de diff temporel ;
- couche de scoring contextuel ;
- integration `results / history / export` ;
- desktop app avec vues guidees et expertes ;
- contrats partages Rust + TypeScript.

**Contrats et interfaces** :
- `IndexedFileRecord` : nom, chemin complet, taille, extension, dates, volume, identifiant stable si disponible, hash partiel ou complet, etat de capture ;
- `FilesystemSnapshot` : identifiant, cible, horodatage, resume, etat d'execution, erreurs, empreinte de volume ;
- `SnapshotDiff` : fichiers nouveaux, disparus, deplaces, renommes, ambigus, avec raisons et niveau de confiance ;
- `MissingFileInsight` : dernier chemin connu, derniere presence, fenetre de disparition estimee, type, taille, score de recuperabilite estime ;
- `RecoveryContextHint` : enrichissement optionnel des resultats existants avec emplacement connu, anciennete de disparition et priorite de tentative ;
- `MonitoringPolicy` : manuel, planifie, temps reel si supporte, toujours explicite et desactivable.

**UX / UI** :
- novice : vue "fichiers disparus" simple, avec dernier emplacement connu, date estimee et message clair sur ce qui est certain ou non ;
- expert : diff detaille entre snapshots, motifs de classement, identifiants, hashes, metadata de volume, timeline des changements ;
- export : permettre de reconstruire l'arborescence d'origine sur une destination sure, sans restaurer sur la source ;
- historique : rattacher snapshots, changements detectes et tentatives de recuperation au journal technique.

**Plan d'execution** :
1. definir les types partages, la base locale et le contrat de snapshot ;
2. implementer l'indexation de fichiers sur chemins ou volumes selectionnes ;
3. implementer les snapshots et le moteur de comparaison entre etats ;
4. construire la vue logique des fichiers disparus et la classification conservative des changements ;
5. enrichir le scoring et les resultats de recovery avec le contexte memorise ;
6. ajouter la planification passive et, plus tard, la surveillance temps reel si elle reste fiable et portable ;
7. brancher l'historique, l'audit et les vues desktop associees.

**Tests et validation** :
- tests unitaires sur les heuristiques de diff : disparu, deplace, renomme, ambigu ;
- tests d'integration sur snapshots synthetiques multi-etats ;
- tests sur erreurs : disque non monte, volume deconnecte, permissions insuffisantes, chemin temporairement inaccessible ;
- verification que la base locale n'ecrit jamais sur la source ;
- verification que les enrichissements de recovery restent estimes et traces ;
- build frontend et backend verts apres chaque tranche ;
- logs d'audit contenant le declenchement des snapshots, les diffs et les erreurs.

**Risques** :
- faux positifs si l'identite d'un fichier n'est pas assez stable entre deux etats ;
- cout CPU/disque si le hashing est trop agressif ou la frequence de scan trop elevee ;
- complexite cross-platform des identifiants stables de fichiers et de volumes ;
- inflation de la base locale si la retention des snapshots n'est pas bornee ;
- confusion UX entre "fichier disparu" et "fichier recuperable".

**Questions ouvertes** :
- quel niveau de hash retenir en premiere tranche : partiel par defaut, complet a la demande, ou adaptatif selon la taille ?
- faut-il commencer par des snapshots manuels et planifies avant toute surveillance temps reel ?
- faut-il limiter la premiere version aux chemins montes choisis par l'utilisateur, puis etendre ensuite aux images et volumes importes ?

**Limites connues** :
- cette couche n'augmente pas magiquement la recuperation physique ; elle ameliore le contexte, le triage et l'orientation ;
- la disparition detectee reste parfois une estimation jusqu'a preuve complementaire ;
- la premiere tranche ne doit pas partir sur une UI lourde ni un daemon de surveillance complexe.

**Statut 2026-04-17** :
- chantier ouvert ;
- ajoute comme couche manquante issue du cadrage `prompt_claude_code_recuperation-1.md` ;
- recommandation d'ordre : types + store local, snapshots, diff engine, vue fichiers disparus, scoring contextuel, puis surveillance passive.

**Statut 2026-04-17 (session sprint-4)** :
- **B1 landé** : `src-tauri/src/filesystem_memory/{mod,types,store}.rs` — `IndexedFileRecord`, `FilesystemSnapshot`, `SnapshotDiff`, `MissingFileInsight`, `RecoveryContextHint`, `MonitoringPolicy` (Manual par défaut, Scheduled avec `MIN_INTERVAL_MINUTES=15`), store JSON-lines sous `dirs::data_local_dir()/recupere/filesystem_memory/filesystem_memory.jsonl`, écriture atomique (tmp + rename), rotation 10 snapshots max par cible, garde `assert_store_not_inside_source`, override `RECUPERE_FILESYSTEM_MEMORY_DIR` pour tests.
- **B2 landé** : `filesystem_memory/indexer.rs` — walker read-only borné par `max_depth`, hash partiel SHA-256 premier + dernier 64 KiB, métadonnées stables (size, mtime, extension), exclusions par défaut (`.git`, `node_modules`, `target`, `.DS_Store`), statut `Completed`/`Partial` selon les erreurs rencontrées.
- **B3 landé** : `filesystem_memory/diff.rs` — moteur déterministe classifiant `new / missing / moved / renamed / modified / ambiguous` avec niveau `Confidence::{High,Medium,Low}`, projection `missing_file_insights`, projection `recovery_context_hints_for`.
- **B4 landé** : commandes Tauri `create_filesystem_memory_snapshot`, `list_filesystem_memory_snapshots`, `compute_filesystem_memory_diff`, `get_filesystem_memory_missing_files`, `get_recovery_context_hints` (enregistrées dans `generate_handler!`), page desktop `src/pages/MissingFilesPage.tsx` + route `/missing-files` + entrée sidebar, i18n FR+EN complète, novice voit liste simple + dernier emplacement, expert voit hint de recovery détaillé.
- **B5 landé** : enrichissement non-invasif via `get_recovery_context_hints` — aucune modification du scoring ou du struct `RecoveredFile`, l'UI reçoit séparément les hints à afficher, respecte "changements chirurgicaux" d'AGENTS.md.
- **B6 partiel** : tracing audit ajouté sur chaque commande (`tracing::info!` avec `scan_id`, `baseline_id`, `head_id`, `files_indexed`, `changes_count`). Scheduler passif et intégration `MonitoringPolicy::Scheduled` explicitement reportés à une tranche ultérieure — le plan mentionnait déjà que la surveillance temps réel reste hors périmètre.
- **Tests** : 20 tests unitaires Rust dans `filesystem_memory::{types,store,indexer,diff}` couvrant round-trip serde, persist/replace/rotation, indexation avec et sans hash, exclusions, heuristiques diff (new/missing/moved/renamed/modified/ambiguous), projection missing + hints, refus de diff sur targets différentes. Tous verts sur `cargo test --lib`.
- **Règles AGENTS.md tenues** : pas de réinvention de moteur existant, garde-fou dur "never write to source", traçabilité audit, wording conservateur ("estimated" / "uncertain" dans l'UI + i18n), novice vs expert distingués.
- **Reliquat** : scheduler passif (B6 complet), intégration surveillance active (hors scope Chantier 82 par conception), wrapping via un bouton depuis la page Résultats — à rouvrir dans une session ultérieure si l'utilisateur le demande.

**Statut 2026-04-18 (session sprint-5 suite)** :
- **B6 complet landé** : scheduler passif opt-in branché sur la policy persistée.
  - `src-tauri/src/filesystem_memory/scheduler.rs` (nouveau) : `std::thread::spawn` + `mpsc::Sender<StopSignal>`, `recv_timeout` avec poll de 30 s pour rester réactif à `stop()`, tick qui ré-exécute `capture_snapshot` + `persist_snapshot` + `rotate_snapshots` sur chaque `target_path` déjà présent en store (dédupliqué + exclut les snapshots `Running`), fail-safe « aucune target » = log info. `RealtimeDeferred` laissé inactif avec `tracing::warn!` explicite ("realtime monitoring is not implemented yet").
  - Persistance policy : nouveau fichier `monitoring_policy.json` dans le même répertoire que `filesystem_memory.jsonl`, écriture atomique (tmp + rename), `.normalize()` appelé avant toute sauvegarde pour garantir le floor `MIN_INTERVAL_MINUTES = 15`. Helpers `load_monitoring_policy[_from]` / `save_monitoring_policy[_in]` + `policy_file_path()` exposés depuis `store.rs`.
  - Commandes Tauri `get_filesystem_memory_policy` + `save_filesystem_memory_policy` ajoutées dans `commands/filesystem_memory_cmd.rs` et enregistrées dans `lib.rs::generate_handler!`. La commande save redémarre le scheduler (`scheduler::start_with_policy(normalized)`) et enregistre un `AuditEventKind::SettingsChanged` avec `{"setting": "filesystem_memory_policy", "policy": normalized}` — toute transition est donc tracée dans le journal signé.
  - Démarrage dans `lib.rs::setup` : appelle `filesystem_memory::load_monitoring_policy()` puis `scheduler::start_with_policy(policy)` (Manual par défaut → thread idle). Échec non-fatal, `tracing::warn!` si le fichier policy est illisible.
  - Front : type `MonitoringPolicy` + const `MIN_MONITORING_INTERVAL_MINUTES = 15` ajoutés à `src/types/filesystemMemory.ts`, bindings `fetchFilesystemMemoryPolicy` + `saveFilesystemMemoryPolicy` dans `src/hooks/ipc/filesystemMemory.ts`. `SettingsPage.tsx` affiche une nouvelle `SectionCard` (dans le bloc `<details open>` avancé) avec select manuel/planifié/temps réel, input `number` minimum 15 pour l'intervalle, wording honnête ("Ce n'est PAS une surveillance temps réel"), bouton save + bannière résultat. i18n FR + EN complète.
- **Règles AGENTS.md tenues** : pas de nouveau variant d'`AuditEventKind` (réutilisation de `SettingsChanged`), scheduler strictement lecture sur la source (il rejoue `capture_snapshot` qui respecte déjà l'invariant read-only), wording UI honnête ("deferred", "NOT realtime"), option `RealtimeDeferred` volontairement inerte.
- **Tests ajoutés** : 9 tests (+ 325 → **334 verts sur `cargo test --lib`**) :
  - `scheduler::collect_distinct_targets_dedupes_and_ignores_running_snapshots`
  - `scheduler::manual_policy_does_not_spawn_a_thread`
  - `scheduler::realtime_deferred_policy_does_not_spawn_a_thread`
  - `scheduler::stop_is_idempotent`
  - `scheduler::scheduled_policy_normalizes_interval_before_storing_it`
  - `scheduler::tick_loop_fires_the_injected_closure_until_stopped` (timing 50 ms + compteur atomique pour éviter les 15 min)
  - `store::load_policy_defaults_to_manual_on_a_fresh_store`
  - `store::policy_round_trip_preserves_scheduled_interval`
  - `store::save_policy_floors_short_intervals_to_minimum`
- **Validation** : `cargo fmt`, `cargo check --all-targets` = 0 warning, `cargo test --lib` = 334 verts, `npx tsc --noEmit` propre, `npm run test:ui` = 54 verts.
- **Limites assumées** : le scheduler ne fait pas de « discovery » de nouvelles cibles — il re-snapshot uniquement les `target_path` déjà présents en store, donc l'utilisateur doit toujours déclencher un premier snapshot manuel depuis `MissingFilesPage` avant que le mode planifié ait du travail à faire. Le front pourrait l'ajouter dans une prochaine session (CTA « Ajouter ce chemin à la surveillance passive »).

## Chantier 83 — Suite native E2E cross-platform (tauri-driver + Appium Mac2) ✅ TERMINÉ (Sprint 6 A3, 2026-04-18 — 7 specs WebdriverIO, 3 jobs CI, fixtures synth voie 2)
**Objectif** : Ajouter une deuxième suite E2E qui pilote la vraie fenêtre Tauri sur **les 3 plateformes cibles de Récupère** (Linux, Windows, macOS), côte à côte avec l'existante Playwright/browser-preview. La suite couvre les 7 flux produits critiques sur fixtures synthétiques uniquement.

**Pivot tranche 1 bis** : tauri-driver ne supporte pas macOS (Apple bloque l'attache externe sur WKWebView). Le projet utilise donc **2 stacks complémentaires**, une seule suite de specs partagée :
- Linux + Windows : `tauri-driver` (config `wdio.tauri-driver.conf.ts`).
- macOS : `Appium Mac2 driver` + WKWebView context switching (config `wdio.appium.conf.ts`).
Un script dispatcher `scripts/run-native-e2e.mjs` choisit la bonne config selon `process.platform`. Les 7 specs sont **identiques** sur les 3 plateformes — les helpers absorbent la différence de stack.

**Pourquoi** :
- la suite Playwright actuelle (`e2e/*.spec.ts`) tourne contre `vite preview` avec le flag `__ALLOW_BROWSER_PREVIEW__` et des IPC mockés (`seedBrowserPreviewState()`) — elle n'attrape aucune régression native (changement de shape IPC, runtime Tauri, droits privilégiés imaging, etc.) ;
- la réserve « native-smoke harness » apparaît déjà dans le commentaire tête de `playwright.config.ts` et dans le reliquat différé des sprints 3/4/5 sous le nom A3 ;
- livrer sans cette couche laisse un angle mort sur toutes les features backend qui ont atterri en Sprints 1→5 (imaging, scan fs natifs, lost volume, chantier 76, filesystem_memory) ;
- l'équipe a besoin de pouvoir casser la CI sur un vrai flux utilisateur natif avant de rapprocher une prochaine release.

**Périmètre** :
- couvert : 7 specs WebdriverIO isolées dans `e2e/native/`, pilotage par **`tauri-driver` 2.0.5 (Linux + Windows)** et **Appium Mac2 driver 3.x (macOS)**, fixtures synthétiques **brutes** générées par un Cargo `example` (`src-tauri/examples/gen_synth_fixture.rs`, voie 2 — pas de retouche du code metier), 3 jobs CI dédiés (`e2e-native-linux`, `e2e-native-windows`, `e2e-native-macos`), doc README section E2E, configuration de retries=0 et parallelism=1 ;
- non couvert : tests sur vrai device physique, remplacement de la suite Playwright existante, refactor des commandes Tauri, migration des tests inline `commands/mod.rs`, extraction support-bundle builder, fixtures FS « formatées » via les fns `synthetic_*` du code analyzers (couvertes par les tests Rust unitaires) ;
- dépendances : Tauri 2, binaire debug Récupère compilé (`cargo build --manifest-path src-tauri/Cargo.toml`), `tauri-driver` 2.0.5 (cargo install global, hors `devDependencies`, Linux + Windows uniquement), `appium` 3.x + `appium-mac2-driver` 3.x (devDependencies, macOS uniquement), `webdriverio` + `@wdio/{cli,local-runner,mocha-framework,spec-reporter,appium-service,types}` v9.x, `@types/mocha`, `tsx` ;
- hypothèses : `tauri-driver` 2.0.5 reste compatible Tauri 2 série courante (réinstall recommandée à chaque bump majeur Tauri) ; Appium Mac2 sait switcher dans le contexte WKWebView via le bridge Web Inspector exposé par Tauri 2 en debug builds (`inspectable = true` par défaut depuis la beta) ; le bundle ID Récupère est `com.recupere.desktop` (référencé en dur dans `wdio.appium.conf.ts`) ; aucune retouche du code metier n'est nécessaire (voie 2 actée tranche 2).

**Contraintes** :
- **aucune écriture sur disque source** : la suite teste explicitement que l'imaging / scan / export n'altère pas les fixtures (assertion mtime + sha256 post-hoc) ;
- fixtures strictement synthétiques, jamais de vrai device ;
- pas de secret en dur : la licence dev key reste gérée par l'env var `RECUPERE_DEV_LICENSE_KEY` (identique aux tests Playwright actuels) ;
- pas de collision avec Playwright : les deux suites cohabitent, `playwright.config.ts` reste inchangé, WebdriverIO scrute uniquement `e2e/native/` ;
- CI bloquant ≠ Day 1 : chacun des 3 jobs natifs en `continue-on-error: true` initial, retiré après **2 runs verts consécutifs** par plateforme — traqué dans le body de chaque job et dans ce chantier ;
- **changements chirurgicaux strictement respectés** : aucune retouche du code metier Rust ni du runtime app — l'example `gen_synth_fixture` est isolé dans `src-tauri/examples/` (zéro impact bundle release), aucun nouveau module `commands/`, aucune élévation de visibilité, aucune Cargo feature ajoutée. Acté tranche 2 (voie 2).

**Architecture concernée** :
- nouveau harness JS `e2e/native/` (helpers + 7 specs partagés cross-platform) ;
- 2 configurations WebdriverIO à la racine : `wdio.tauri-driver.conf.ts` (Linux + Windows) et `wdio.appium.conf.ts` (macOS) ;
- dispatcher `scripts/run-native-e2e.mjs` qui choisit la config selon `process.platform` ;
- `tsconfig.wdio.json` dédié (strict + noUnusedLocals, includes les 2 configs + `e2e/native/**/*.ts`) ;
- `e2e/native/wdio-env.d.ts` qui augmente le type `WebdriverIO.Capabilities` avec les capabilities tauri-driver et Appium Mac2 (zéro cast, zéro `any`) ;
- nouveau `src-tauri/examples/gen_synth_fixture.rs` (Cargo example, compilé par `cargo build --examples`, jamais bundlé avec l'app) ;
- 3 jobs CI `.github/workflows/ci.yml` : `e2e-native-linux` (ubuntu-latest), `e2e-native-windows` (windows-latest), `e2e-native-macos` (macos-latest) ;
- documentation : section README « Running E2E tests » + statut dans ce chantier.

**Contrats et interfaces** :
- helper TS `e2e/native/helpers/fixtures.ts` : wrappe `child_process.spawn('cargo', ['run', '--example', 'gen_synth_fixture', '--', kind, outPath])` + retourne le chemin absolu de la fixture écrite ;
- helper TS `e2e/native/helpers/driver.ts` : assemble le `browser` WebdriverIO, charge la dev key depuis l'env, prépare un tmpdir de session ;
- chaque spec consomme les fixtures + le driver sans exposer d'ABI publique — les specs sont des consommateurs de l'UI existante.

**UX / UI** :
- pas de changement UI — la suite est un outil de test.

**Étapes d'implémentation (tranches)** :
1. `PLANS.md` — entrée Chantier 83 (ce document).
2. Installation WebdriverIO + `wdio.tauri-driver.conf.ts` (initial) + script npm `test:e2e:native` + tsconfig patch.
2 bis. **Pivot cross-platform** : ajout `wdio.appium.conf.ts` (macOS), install `appium` + `appium-mac2-driver`, dispatcher `scripts/run-native-e2e.mjs`, scripts `test:e2e:native:{linux,windows,macos}`, augmentation `wdio-env.d.ts` avec capabilities Appium.
3. Helper fixtures synth (Cargo `example` + helper TS) — voie 2, fixtures brutes, zéro retouche metier.
4. `native-scan-flow.spec.ts` — fixture `carver-signatures` synth → lancement scan signature-carving → vérification progression + résultats.
5. `native-imaging-flow.spec.ts` — fixture synth → flux « créer image read-only » → assertion mtime source inchangée.
6. `native-export-flow.spec.ts` — sélection fichiers récupérés → export → sha256 des fichiers exportés.
7. `native-lost-volume.spec.ts` — fixture `mbr-gpt` synth, inspection volume perdu.
8. `native-history.spec.ts` — page Historique, affichage session passée + logs.
9. `native-expert.spec.ts` — toggles mode expert (hex preview, ADS, resource fork) sur fixture `expert-stub`.
10. `native-licensing.spec.ts` — paywall / activation licence via dev key env.
11. **3 jobs CI** (`e2e-native-linux`, `e2e-native-windows`, `e2e-native-macos`) avec `continue-on-error: true`, documentation du critère de promotion (2 runs verts consécutifs **par plateforme**).
12. README section « Running E2E tests » multi-plateformes + mémoire auto `project_sprint6_a3_done.md`.

**Tests et validation** :
- `cargo check --all-targets` = 0 warning après chaque tranche ;
- `cargo test --lib` ≥ 334 verts (baseline Sprint 5 B6) ;
- `npx tsc --noEmit` propre (inclut `wdio.conf.ts` et `e2e/native/**`) ;
- `npm run test:ui` = 54 verts (Vitest inchangé) ;
- suite Playwright existante `playwright.config.ts` passe toujours (spot check local) ;
- `npm run test:e2e:native` vert localement sur macOS pour chaque spec ajoutée.

**Risques** :
- **macOS / Appium Mac2** : la première attache à Récupère déclenche une demande d'autorisation **Accessibilité** (Préférences Système → Confidentialité). Action manuelle persistante par poste de dev ; CI macOS doit pré-accorder via `tccutil` ou équivalent (à valider tranche 10) ;
- **macOS / WKWebView context switching** : Appium met 2-3 s à voir la webview après le launch, mitigé par `waitUntil` côté helper. Plus fragile que tauri-driver, accepté comme coût d'avoir une vraie couverture macOS ;
- versions figées `tauri-driver` 2.0.5 + `appium` 3.x + `appium-mac2-driver` 3.x : bump à planifier si incompatibilité Tauri 2.x future ;
- flakiness webview : mitigé par parallelism=1, timeout ≥30 s par test, zéro retry (on refuse de masquer les vrais bugs) ;
- coût temps CI : 3 runners au lieu d'1, mais ubuntu et windows compensent le coût macos. Accepté pour couvrir le périmètre produit Récupère.

**Questions ouvertes** :
- ✅ tranchée tranche 1 bis : couverture cross-platform actée (Linux + Windows + macOS). 3 jobs CI, 2 stacks E2E.
- faut-il baker les fixtures synth en binaire testdata (reproductibilité parfaite) ou laisser l'example Cargo les régénérer à chaque run ? Choix actuel : régénération — traçable, pas de blob dans le repo.
- pré-autorisation Accessibilité macOS sur runner CI : à confirmer (`tccutil reset Accessibility` puis grant scripté, ou job `continue-on-error` permanent jusqu'à trouver une solution propre).

**Limites connues** :
- la suite ne teste pas les opérations privilégiées (imaging via `diskutil` / `wmic`) — elles restent couvertes par les tests Rust unitaires ;
- les fixtures voie 2 ne sont PAS des FS formatés — les specs valident le **flux UI/IPC/engine** end-to-end, pas la fidélité des parsers FS (déjà couverte par 334 tests Rust) ;
- la CI en `continue-on-error: true` pendant la phase d'observation (par plateforme) ne bloque pas les PR — à documenter auprès du reviewer humain ;
- le développement local cross-platform demande un poste par OS (ou des conteneurs Linux + une VM Windows) ; la CI reste l'autorité.

**Statut 2026-04-18 (session sprint-6 A3)** : ✅ **CHANTIER 83 LIVRÉ** — 13 tranches sur 13 (0, 1, 1 bis, 2 → 11) closes.
- **tranche 0** : ce plan, intégré au PLANS.md sous le Chantier 82.
- **tranche 1** : `wdio.tauri-driver.conf.ts` (renommée tranche 1 bis), `tsconfig.wdio.json`, `e2e/native/wdio-env.d.ts`, install `webdriverio` + suite WDIO v9 + `tsx`. Baseline verte.
- **tranche 2** : `src-tauri/examples/gen_synth_fixture.rs` (3 kinds : `carver-signatures` 8 MiB / `mbr-gpt` 16 MiB / `expert-stub` 1 MiB) + `e2e/native/helpers/fixtures.ts`. Voie 2 actée (zéro retouche metier). Smoke runtime OK.
- **tranche 1 bis** : pivot cross-platform — split en 2 configs (`wdio.tauri-driver.conf.ts` Linux+Windows, `wdio.appium.conf.ts` macOS), install `appium` 3.x + `appium-mac2-driver` 3.x, dispatcher `scripts/run-native-e2e.mjs`, scripts `test:e2e:native:{linux,windows,macos}`, capabilities Appium ajoutées à `wdio-env.d.ts`. `tauri-driver` 2.0.5 installé en cargo global (0.1.4 obsolète, ne compile pas).
- **tranche 3** : `e2e/native/native-scan-flow.spec.ts` + helpers `driver.ts` (attachToWebview platform-aware + invokeTauriCommand) + `license.ts` (mint dev license fresh via `cargo run --bin gen_license` + activate via IPC). Type-check OK. **Première exécution runtime sur poste Mac dev = bloquée** : Mac2 driver crash car Xcode complet absent (CLI Tools seuls). **Voie B actée** : runtime macOS validé exclusivement via la CI GitHub Actions `macos-latest` (Xcode pré-installé). README documente la limite.
- **tranches 4 → 9** : 6 specs WebdriverIO supplémentaires écrites en série, type-check WDIO clean entre chaque, baselines (cargo check / cargo test --lib 334 / vitest 54) inchangées :
  - 4 → `native-imaging-flow.spec.ts` : import + `start_imaging` + poll progress + assertion taille image dest > 0 + assertion sha256/size/mtime source inchangés (read-only invariant).
  - 5 → `native-export-flow.spec.ts` : scan → `get_results` → `start_export` (selectedFileIds, conflictStrategy=rename, verifyIntegrity=true) → poll → assertion `exported_files === selectedFileIds.length` + walk fs destination + read-only invariant source.
  - 6 → `native-lost-volume.spec.ts` : fixture `mbr-gpt` → `get_diagnostic` → assertion `potential_volumes_inspected === true` && `potential_volumes.length >= 1` + déclenchement `start_potential_volume_scan` (assert IPC accepte la requête, on ne poll pas la complétion sur fixture sans FS) + read-only invariant.
  - 7 → `native-history.spec.ts` : scan complet → `get_scan_history` → assertion entrée présente avec scanId/deviceId/status terminal + `get_scan_logs` retourne au moins 1 entry structurée.
  - 8 → `native-expert.spec.ts` : `get_file_hex_preview` retourne ≥ 1 byte sur le 1er fichier candidat + `get_file_auxiliary_preview` (kind=ads et resource-fork) accepte la requête sans crash (response empty acceptée car fixture brute n'a pas d'ADS/RF).
  - 9 → `native-licensing.spec.ts` : reset baseline → `activate_license({ key: malformé })` rejet propre (status malformed/invalid_signature) → mint dev key → activation → `get_license_status` confirme pro → deactivate → status retombe à free.
- **tranche 10** : 3 jobs CI dans **un workflow séparé** `.github/workflows/e2e-native.yml` (sortis de `ci.yml` pour préserver son temps de réponse + clarté du signal) — `e2e-native-linux` (ubuntu-latest, cargo install tauri-driver 2.0.5 + webkit2gtk-driver + xvfb), `e2e-native-windows` (windows-latest, cargo install tauri-driver 2.0.5, msedgedriver bundle), `e2e-native-macos` (macos-latest, npx appium driver install mac2, **pre-grant Accessibility déterministe via INSERT direct dans `~/Library/Application Support/com.apple.TCC/TCC.db`** sur la base de la doc Apple TCC publique). Triggers : cron quotidien 03:00 UTC + workflow_dispatch + push tags `v*` — **PAS sur `pull_request:`** tant que la suite n'est pas observée stable. **Aucun `continue-on-error`** : signal honnête, badge CI reflète la vérité. Promotion à `pull_request:` bloquant = 2 runs schedulés verts consécutifs par plateforme (PR explicite à ce moment-là, tracée). Upload logs WDIO + Appium en artifact (rétention 14 jours) en cas d'échec. Les 2 YAML validés via js-yaml.
- **tranche 11** : ce statut + mémoire auto `project_sprint6_a3_done.md` + entrée MEMORY.md.

**Validation finale** : `npx tsc --noEmit` (baseline src/) clean ; `npx tsc -p tsconfig.wdio.json --noEmit` (2 configs + 7 specs + helpers) clean ; `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` 0 warning ; `cargo test --manifest-path src-tauri/Cargo.toml --lib` **334 passed** ; `npm run test:ui` **54 passed**. Aucune régression sur le code existant — les 7 specs natives, les 2 configs WDIO, le helper Cargo example, les augmentations de type et les 3 jobs CI sont **strictement additifs**.

**Ce qui reste hors scope (backlog explicite)** : grand splittage `commands/mod.rs` (toujours 4 750 LoC), migration tests inline `commands/mod.rs` (~3 000 LoC) vers `tests/`, promotion `pull_request:` bloquante du workflow `e2e-native.yml` (PR explicite après 2 runs schedulés verts consécutifs par plateforme — checklist en 6 items intégrée en tête du fichier `e2e-native.yml`).

**Décisions de durcissement post-livraison** :
- **Xcode complet est désormais un prérequis dev officiel pour la suite native macOS** (et non une "limite") — documenté en bloc "Native macOS — dev requirements" dans le README. Les contributeurs sans Xcode bossent sur le reste du projet sans souci.
- **`continue-on-error: true` retiré** : la suite native vit dans son propre workflow `.github/workflows/e2e-native.yml`, indépendant du `ci.yml` principal. Triggers schedulés + manuels + tags release seulement. Si rouge → workflow rouge, pas de placebo.
- **Pre-grant Accessibility CI macOS** : passage de `tccutil reset` best-effort (placebo) à un INSERT déterministe dans le `TCC.db` SQLite de l'utilisateur runner — schéma documenté en commentaires du job, basé sur la doc Apple TCC publique.

## Chantier 84 - Professionnalisation produit du socle "memoire filesystem"
**Objectif** : Transformer la couche "filesystem memory" d'une feature prometteuse mais partielle en une capacite produit professionnelle, fermee de bout en bout, suffisamment fiable pour soutenir un discours commercial et un usage prudent en contexte sensible.

**Pourquoi** :
- le ressenti "application amateur" vient moins d'un manque de code que d'un manque de fermeture produit, de garanties et d'integration ;
- la meilleure trajectoire vers un niveau professionnel n'est pas d'ajouter encore des features, mais de finir une capacite critique jusqu'au niveau preuve + audit + UX + tests ;
- `Chantier 82` a deja pose une base utile ; la prochaine valeur vient du durcissement, pas de la multiplication des surfaces ;
- dans un domaine recovery, une promesse partielle ou mal semantisee degrade la confiance plus vite qu'une limitation explicite.

**Perimetre** :
- couvert : identite robuste de volume/cible, semantique temporelle exacte ("last observed" vs "mtime"), exposition UI complete des changements (`new / missing / moved / renamed / modified / ambiguous`), integration reelle avec `results / scoring / export / history / audit`, couverture E2E et criteres de release de cette capacite ;
- non couvert : nouveau filesystem analyzer majeur, vrai watcher temps reel natif multi-plateforme, cloud obligatoire, refactor large non necessaire hors des modules touches ;
- dependances : `filesystem_memory`, `audit`, `scoring`, `commands`, `results`, `history`, `settings`, types partages Rust/TypeScript, suites `test:ui` et `e2e-native` ;
- hypotheses : la meilleure option court terme est de fermer d'abord la promesse "le systeme se souvient du disque" sur chemins/volumes deja supportes, avant d'ouvrir de nouvelles promesses plus larges.

**Contraintes** :
- ne jamais ecrire sur le disque source ;
- ne jamais presenter un "fichier disparu" comme une suppression certaine sans niveau de confiance et sans borne temporelle explicite ;
- ne jamais comparer silencieusement deux snapshots de volumes differents comme s'il s'agissait du meme disque ;
- toute affirmation importante doit etre tracable soit dans l'audit signe, soit dans les logs techniques, soit dans les deux ;
- la version novice doit rester lisible et calme ; la version expert doit exposer les details sans cacher les limites ;
- toute extension de scope doit etre justifiee par un gain direct sur la fermeture produit.

**Architecture concernee** :
- backend Rust : `src-tauri/src/filesystem_memory/{types,store,indexer,diff,scheduler}.rs`, `commands/filesystem_memory_cmd.rs`, `audit`, `scoring`, `lib.rs` ;
- frontend desktop : `src/pages/MissingFilesPage.tsx`, `ResultsPage.tsx`, `HistoryPage.tsx`, `SettingsPage.tsx`, composants `results/*`, `SidebarNav`, hooks IPC ;
- contrats partages : `src/types/filesystemMemory.ts`, `src/types/results.ts`, et mappings IPC associes ;
- QA / release : tests Rust, Vitest UI, WDIO natif, workflows CI lies a cette capacite.

**Contrats et interfaces** :
- `FilesystemSnapshot` doit distinguer explicitement l'identite de la cible analysee (`target_path`) de l'identite du volume / support observe ; si l'identite du volume est absente ou instable, la confiance doit etre degradee ou la comparaison refusee ;
- `MissingFileInsight` ne doit plus reutiliser `modified_at_ms` comme preuve de "derniere presence" ; il faut separer :
  - date de derniere observation par snapshot ;
  - date de modification du fichier si connue ;
  - fenetre estimee de disparition ;
- `SnapshotDiff` doit pouvoir servir a la fois la vue novice et la vue expert, sans projection reductrice irreversible ;
- `RecoveryContextHint` doit etre effectivement consomme par les resultats et/ou le scoring, pas seulement expose via IPC ;
- l'audit doit couvrir au minimum : creation de snapshot, comparaison de snapshots, changement de policy, erreurs critiques de capture, enrichissement contextuel applique aux resultats si cette action influence l'experience utilisateur.

**UX / UI** :
- novice :
  - un parcours simple "capturer -> comparer -> comprendre ce qui a change -> voir comment cela aide la recuperation" ;
  - libelles honnetes : "dernier instantane ou ce fichier etait encore observe", "fenetre estimee de disparition", "niveau de confiance" ;
  - vue claire des changements, pas seulement des fichiers disparus ;
- expert :
  - diff complet avec raison de classement, identite de volume, hash, taille, timestamps techniques, et raisons de declassification/ambiguite ;
  - historique rattache aux snapshots et aux actions utilisateur ;
- results/export :
  - affichage du dernier chemin connu, de l'anciennete de disparition, et du niveau de confiance quand un contexte memorise existe ;
  - interdiction implicite de "restaurer a l'ancien emplacement" sur la source ; seule une destination sure reste autorisee.

**Plan d'execution** :
1. **P0 - Verite produit et contrat ferme**
   - geler la promesse utilisateur de la capacite ;
   - corriger les champs et mots qui pretendent aujourd'hui plus que ce que le systeme sait reellement ;
   - definir les nouveaux criteres "pro-ready" de cette couche avant tout dev supplementaire.
2. **P1 - Integrite des snapshots**
   - ajouter ou durcir l'identite de volume/support ;
   - refuser ou degrader explicitement les comparaisons entre cibles incompatibles ;
   - migrer proprement les snapshots existants si le contrat evolue.
3. **P2 - Semantique temporelle fiable**
   - separer "mtime" du fichier et "last observed in snapshot" ;
   - exposer une vraie fenetre de disparition ;
   - revoir les messages UI et les hints de recovery pour qu'ils restent honnetes.
4. **P3 - Diff complet visible dans le produit**
   - rendre visibles `new / moved / renamed / modified / ambiguous`, pas seulement `missing` ;
   - garder une projection novice simple sans perdre la granularite experte.
5. **P4 - Integration reelle avec la recuperation**
   - brancher `RecoveryContextHint` dans `ResultsPage`, les filtres, le scoring contextuel et l'export ;
   - faire en sorte que la memoire filesystem aide concretement le triage et la priorisation.
6. **P5 - Audit, historique et support**
   - ajouter les evenements manquants au journal signe ou definir explicitement leur trace technique equivalente ;
   - rattacher snapshots, comparaisons, erreurs et usages du contexte memoire a `HistoryPage` et aux bundles support.
7. **P6 - Validation produit**
   - ajouter des tests unitaires/integration/E2E sur le parcours complet ;
   - definir une gate de release : la feature ne peut plus etre consideree "pro" sans ces validations vertes.

**Tranches recommandees** :
- tranche A : contrat + identite volume + semantique temporelle ;
- tranche B : diff complet UI + integration results/scoring/export ;
- tranche C : audit/history/support + E2E natif + criteres release.

**Etat au 2026-04-18** :
- tranche A engagee et partiellement livree dans le code ;
- identite de volume ajoutee/calculee au moment du snapshot et verification stricte lors du diff ;
- semantique temporelle rendue plus honnete dans les contrats et l'UI (`last observed` / `first missing observed` / `file modified`) ;
- vue `MissingFiles` enrichie avec le diff complet et avertissements sur snapshots partiels ;
- audit et `HistoryPage` branches sur les evenements critiques de cette capacite ;
- tranche B engagee et partiellement livree :
  - `Results` et `Export` consomment maintenant le contexte memoire filesystem quand le diff choisi correspond reellement a la source analysee ;
  - le matching backend a ete durci pour eviter un rapprochement amateur "par nom seul" ; l'app privilegie un match par chemin exact, puis un fallback unique `(name, size)` avec confiance degrades ;
- sous-lot de fermeture decide pour le scoring contextuel :
  - introduire un score de triage derive et borne par `RecoveryContextHint`, sans ecraser silencieusement le score brut du moteur ;
  - limiter tout bonus au triage des candidats supprimes et non corrompus ;
  - rendre visible dans `Results` et `Export` quand la memoire filesystem a re-priorise un fichier, avec un message explicite "triage seulement" ;
- sous-lot scoring contextuel livre :
  - les fichiers enrichis par la memoire filesystem portent maintenant un delta de triage borne, visible dans `Results` et repris dans `Export` ;
  - le score brut reste conservable/explicable via la distinction `base score` / `triage-adjusted score` ;
  - les fichiers visibles ou corrompus ne recoivent aucun faux bonus de recuperabilite ;
- reste a fermer pour atteindre le niveau "pro-ready" de ce chantier : E2E natif dedie, gate de release explicite, et eventuelle integration backend plus profonde si l'on veut reutiliser ce triage hors UI.

**Tests et validation** :
- tests unitaires Rust :
  - refus ou declassification des diffs inter-volumes ;
  - projection correcte des dates `last_observed_at` / `file_modified_at` / `missing_window` ;
  - non-regression des heuristiques `moved / renamed / ambiguous` ;
- tests d'integration :
  - snapshots successifs d'une meme cible avec renommage, deplacement, disparition, retour du fichier, acces refuse partiel ;
  - comportement sur disque debranche / cible remontee / identite de volume modifiee ;
  - verification stricte que le store reste hors source ;
- tests UI / E2E :
  - parcours natif "capture -> compare -> voir les changements -> retrouver le contexte dans les resultats" ;
  - affichage honnete des etats `empty / loading / partial-success / warning / error` ;
  - non-regression novice vs expert ;
- criteres de done :
  - impossible de presenter comme "meme disque" deux snapshots de volumes differents sans signal explicite ;
  - l'app peut dire honnetement "ce fichier etait encore observe dans le snapshot du [date], puis absent dans celui du [date]" ;
  - les resultats de recovery affichent le contexte memoire quand il existe ;
  - l'historique et l'audit montrent les actions critiques de cette capacite ;
  - la doc produit peut decrire cette feature sans exageration.

**Risques** :
- derive de scope si on essaye de "tout rendre pro" au lieu de fermer une capacite precise ;
- dette de migration si les anciens snapshots ne portent pas assez d'identite de volume ;
- ambiguite persistante sur certains cas cross-platform ou l'identifiant stable manque ;
- surcharge UX si la vue expert contamine la vue novice ;
- tentation d'annoncer la surveillance temps reel avant d'avoir une implementation vraiment fiable.

**Questions ouvertes** :
- quelle source d'identite de volume retenir par plateforme dans la premiere tranche sans introduire un couplage fragile ?
- faut-il refuser totalement le diff quand `volumeFingerprint` manque, ou autoriser un mode degrade a confiance basse ?
- jusqu'ou faire descendre l'integration dans `scoring` sans refactorer lourdement le moteur existant ?
- faut-il faire apparaitre ces informations aussi dans `ExportPage`, ou seulement dans `ResultsPage` et `HistoryPage` pour la premiere fermeture produit ?

**Limites connues** :
- meme apres ce chantier, l'application ne "recree" pas des donnees physiquement detruites ;
- la surveillance temps reel reste hors de cette tranche tant qu'elle n'est pas portable et honnete ;
- la professionnalisation vise ici une capacite produit fermee, pas une finition totale de tout le produit.

**Definition de succes produit** :
- un utilisateur peut comprendre, verifier et exploiter l'historique d'un fichier sans extrapolation implicite ;
- un reviewer technique peut suivre la trace complete d'une decision importante ;
- un test natif peut prouver le parcours critique de bout en bout ;
- la communication produit peut promettre exactement ce que le systeme garantit.

**Statut 2026-04-18** :
- chantier ouvert ;
- priorite recommandee : haute ;
- recommandation de pilotage : suspendre les nouvelles features transverses tant que la tranche A n'est pas fermee ;
- objectif de session suivant : lancer la tranche A avec changements chirurgicaux sur contrats, audit et UI associee.

## Chantier 85 — Sprint 8 : clôture des vrais manques (mini-plan)

**Objectif** : Fermer les 7 écarts détectés par l'audit du 2026-04-18 en distinguant les fixes chirurgicaux (tranche A) des décisions produit à trancher avant implémentation (tranche B).

**Pourquoi** :
- l'audit a montré que le code est mûr (99/101 endpoints câblés UI, 10/10 pages fonctionnelles, 334 tests Rust verts) mais 7 poches résiduelles subsistent ;
- la majorité relève de décisions produit (exposer/supprimer/gated), pas de bugs à corriger en silence ;
- AGENTS.md interdit toute « magie » — VHDX pris comme disque brut viole cette règle et doit être fixé d'abord.

**Périmètre** :
- **couvert** : les 7 points identifiés dans la liste chirurgicale finale de l'audit ;
- **non couvert** : tout nouveau chantier produit non listé ci-dessous, refactor large, fuzzing analyzers, migration `Mutex`→`RwLock` ;
- **dépendances** : [src-tauri/src/virtual_disk/vhd.rs](src-tauri/src/virtual_disk/vhd.rs), [src-tauri/src/commands/device.rs](src-tauri/src/commands/device.rs), [src-tauri/src/raid/](src-tauri/src/raid/), [src-tauri/src/commands/audit.rs](src-tauri/src/commands/audit.rs), [docs/hard-case-matrix.md](docs/hard-case-matrix.md), [benchmarks/](benchmarks/).

**Tranche A — fixes chirurgicaux (action immédiate)**

| # | Point | Fix choisi | Effort |
|---|-------|-----------|--------|
| A1 | Stub VHDX ([vhd.rs:190-202](src-tauri/src/virtual_disk/vhd.rs#L190-L202)) retourne le header VHDX comme données utilisateur | Rejet explicite avec erreur `Err("VHDX format detected but not yet supported. Use VHD, E01, VMDK or raw .img/.dd.")` + test unitaire + message i18n côté UI si l'erreur remonte via `import_recovery_source` | ~30 LoC + 1 test |

**Tranche B — décisions produit à valider avant implémentation**

| # | Point | Option 1 | Option 2 | Risque si mauvais choix |
|---|-------|----------|----------|-------------------------|
| B1 | 2 fns encryption orphelines ([device.rs:197-221](src-tauri/src/commands/device.rs#L197-L221)) | **Supprimer** le code mort (plus safe, AGENTS.md § 4 « pas de features non demandées ») | **Exposer** derrière un mode `lab` gated + feature flag + i18n strict | Bruteforce = « faux magic recovery » si exposé sans garde-fou → violation AGENTS.md |
| B2 | RAID reconstruction absente ([raid/](src-tauri/src/raid/) 745 LoC) | **Conserver la détection** uniquement + message UI clair « reconstruction non livrée » | **Implémenter** reconstruction RAID 0/1/5/6 (chantier à chiffrer séparément, ~plusieurs jours) | Module fantôme qui ne sert à rien si gardé tel quel ; revendication marketing fausse si annoncé |
| B3 | JPEG multi-gap carving ([docs/hard-case-matrix.md](docs/hard-case-matrix.md) TODO) | **Accepter le gap** et le documenter en « limitation connue » dans l'UI | **Implémenter** l'assembly multi-gap (chantier dédié) | Faux positifs si annoncé comme résolu |
| B4 | MKV EBML partial ([docs/hard-case-matrix.md](docs/hard-case-matrix.md) TODO) | **Accepter le gap** et le documenter | **Implémenter** le parser EBML complet | Previews MKV trompeuses si livré partiel |
| B5 | APFS P0 benchmark `apfs_deleted_orphan_catalog_v1` (Chantier 80 trou) | **Créer l'artefact benchmark** (génération fixture synth + 1 run baseline) | **Exclure** explicitement du corpus v1 jusqu'à un chantier dédié | Benchmark public biaisé si conclusions publiées sans ce scénario |
| B6 | `get_recent_traces` / `clear_recent_traces` orphelins UI ([commands/audit.rs:28,35](src-tauri/src/commands/audit.rs#L28-L35)) | **Ajouter** un panneau debug dans `SettingsPage` ou `Expert` mode | **Retirer** du handler si aucun consommateur prévu | Commandes fantômes qui polluent la surface IPC |

**Contraintes** :
- ne jamais écrire sur le disque source ;
- ne jamais faire croire qu'une récupération est possible quand elle ne l'est pas (AGENTS.md § « faux magic recovery ») ;
- chaque action de tranche B doit être approuvée explicitement avant exécution.

**Étapes d'exécution** :
1. **A1** : fix VHDX + test, commit unique, vérifier `cargo test` ;
2. attendre validation produit pour **B1 à B6** ;
3. implémenter chaque point validé séparément, un commit par point ;
4. mettre à jour cette section avec ✅ à mesure des livraisons.

**Tests et validation** :
- A1 : `cargo test --manifest-path src-tauri/Cargo.toml --lib virtual_disk` vert ;
- B1-B6 : tests spécifiques selon l'option retenue ;
- globalement : `npm run test:ui`, `cargo check`, `cargo test --lib` verts en fin de sprint.

**Risques** :
- choisir l'option « implémenter » sans chiffrer le chantier (surtout B2 RAID reconstruction = plusieurs jours) ;
- laisser des modules fantômes dans le code si on hésite entre supprimer et exposer (B1, B6) ;
- annoncer publiquement des features B3/B4/B5 avant livraison complète.

**Statut 2026-04-18** :
- **A1** : prêt à implémenter, décision chirurgicale évidente (rejet explicite) ;
- **B1 à B6** : en attente de décision produit, aucun travail entamé ;
- **priorité recommandée** : A1 tout de suite, puis vider B1 et B6 (hygiène surface IPC), puis B3-B4-B5 documentation, puis B2 seulement si décision « implémenter » prise avec ressources.

## Chantier 86 - Fermeture des surfaces orphelines et diagnostic produit

**Objectif** : augmenter la credibilite produit sans supprimer des capacites potentiellement utiles, en exposant les endpoints deja codes mais invisibles, et en donnant enfin une surface verifiable pour l'audit local et le diagnostic support.

**Constat de depart** :
- plusieurs endpoints Tauri sont reellement implementes mais sans consumer UI clair : `verify_audit_trail`, `get_recent_traces`, `clear_recent_traces`, `search_file_by_name`, `suggest_file_reconstruction`, `probe_gemma_registry`, `generate_lab_bundle`, `deactivate_license` ;
- l'absence de surface produit donne une impression de fonctionnalites inachevees ou marketing ;
- la credibilite professionnelle souffre davantage d'un manque de fermeture UX que d'un manque brut de code.

**Hypotheses** :
- on privilegie l'exposition de capacites deja existantes avant d'ouvrir un nouveau chantier backend large ;
- on garde un langage honnete : triage, diagnostic, verification locale, jamais de "magic recovery" ;
- la premiere tranche doit rester chirurgicale et testable sans remodeler l'architecture.

**Risques** :
- si l'on expose une capacite brute sans garde-fous UX, on deplace juste le probleme ;
- trop charger `Settings` ou `Results` peut nuire a la lisibilite novice ;
- les diagnostics/audit doivent rester strictement locaux et ne jamais inclure de contenu recupere ni de donnees source.

**Modules impactes** :
- frontend : `src/pages/SettingsPage.tsx`, `src/pages/ResultsPage.tsx`, `src/components/results/AiAnalysisPanel.tsx`, `src/hooks/ipc/audit.ts`, `src/types/audit.ts`, i18n ;
- backend/IPC : pas de nouvelle commande requise pour la tranche initiale, uniquement exposition/consommation propre des commandes existantes.

**Plan d'execution** :
1. **P0 - Surface diagnostic/audit fermee**
   - exposer `verify_audit_trail`, `get_recent_traces`, `clear_recent_traces` cote TypeScript ;
   - ajouter dans `Settings` une section diagnostic locale avec etat de la chaine d'audit, traces recentes, action de purge, et langage support explicite ;
   - verification : `Settings` permet de verifier localement la chaine et de lire/vider les traces recentes sans browser console.
2. **P1 - Capacites IA orphelines rendues utiles**
   - exposer `search_file_by_name` dans `Results` comme outil d'investigation rapide ;
   - exposer `suggest_file_reconstruction` pour le fichier actuellement cible/selectionne ;
   - verification : depuis `Results`, l'utilisateur peut trouver un fichier par nom/chemin puis obtenir une strategie locale de triage/reparation pour un candidat concret.
3. **P2 - Recadrage produit minimal**
   - aligner les labels et notices pour dire ce qui est supporte, ce qui est investigatif, et ce qui reste une estimation ;
   - verification : aucune nouvelle surface n'insinue une recuperation certaine ou une ecriture sur la source.
4. **P3 - Validation**
   - `npx tsc --noEmit`
   - tests UI/Vitest sur les nouvelles surfaces critiques ;
   - si possible, revalidation ciblee du parcours `Results -> AI tools` et `Settings -> diagnostics`.

**Criteres de succes** :
- plus aucun endpoint expose dans cette tranche ne reste sans consumer UI explicite ;
- l'utilisateur peut verifier localement l'integrite de l'audit et collecter des traces sans passer par un terminal ;
- l'outil IA dans `Results` apporte de la valeur concrete de triage, pas seulement des demos generiques ;
- la communication produit devient plus credible parce qu'elle montre les preuves locales plutot que des promesses.

**Limites connues** :
- cette tranche ne ferme pas encore RAID reconstruction, chiffrement operationnel complet, ni tous les endpoints orphelins secondaires ;
- `runtime_capabilities` restera a recadrer dans un chantier dedie de verite produit transverse ;
- `generate_lab_bundle`, `probe_gemma_registry` et `deactivate_license` pourront suivre dans une tranche de fermeture ulterieure si la surface support/licensing le justifie.

## Chantier 87 - Bundle laboratoire pro et integration Expert

**Objectif** : transformer le `lab bundle` en artefact de support professionnel reel, puis l'integrer directement au workflow Expert pour qu'il serve pendant l'investigation et l'escalade, pas seulement depuis `Settings`.

**Constat de depart** :
- `generate_lab_bundle` existe mais produit un simple fichier texte ;
- le contenu actuel est utile mais trop pauvre pour un vrai relais support / labo ;
- l'atelier Expert n'expose pas encore ce workflow alors qu'il concentre deja l'analyse technique.

**Hypotheses** :
- un bundle pro doit combiner lisibilite humaine et structure machine ;
- on privilegie un dossier structure local (`summary.txt`, `manifest.json`, contexte technique) plutot qu'un format opaque ;
- le bundle reste strictement read-only, sans contenu recupere ni octets source.

**Modules impactes** :
- backend : `src-tauri/src/commands/export.rs` ;
- frontend : `src/pages/ExpertPage.tsx`, eventuellement `SettingsPage.tsx` si le message de succes doit rester coherent ;
- i18n expert/support.

**Plan d'execution** :
1. enrichir `generate_lab_bundle` avec :
   - identite build/runtime ;
   - metadonnees device plus completes ;
   - diagnostic detaille ;
   - contexte RAID/chiffrement/SMART si disponible ;
   - etat de la chaine d'audit ;
   - traces techniques recentes bornees ;
   - sortie structuree sous forme de dossier contenant au minimum un resume texte et un manifest JSON ;
2. integrer la generation du bundle dans `ExpertPage` pour qu'un technicien puisse l'exporter depuis le contexte d'analyse ;
3. valider TypeScript/UI et verifier que le bundle reste local, lisible et utile sans promettre d'actions destructives.

**Criteres de succes** :
- le bundle est exploitable par un humain et par un outil de support ;
- l'atelier Expert permet de le produire sans quitter le contexte d'investigation ;
- aucune donnee sensible interdite n'est ajoutee ;
- le workflow reste simple : choisir un dossier, generer, obtenir un chemin de sortie clair.

## Chantier 88 - Chiffrement operationnel expert puis readiness RAID

**Objectif** : reduire l'ecart entre detection et usage reel sur deux parcours techniques visibles dans le produit, en commencant par un deblocage chiffrement operable en mode expert, puis en preparant un panneau RAID plus actionnable sans sur-promettre de reconstruction non livree.

**Constat de depart** :
- le produit sait detecter un volume chiffre et expose deja un backend d'ouverture, mais aucun workflow expert clair ne permet de s'en servir depuis l'app ;
- cette absence donne l'impression d'un produit qui "voit" le chiffrement sans aider a avancer ;
- cote RAID, la detection existe, mais l'usage reel reste encore trop proche d'un simple diagnostic passif.

**Hypotheses** :
- le premier lot chiffrement doit rester strictement borne a une ouverture par mot de passe fourni par l'utilisateur ;
- aucun bruteforce, aucune promesse de recuperation magique, aucune ecriture implicite sur la source ;
- le chemin doit rester reserve au mode expert avec avertissement explicite et journalisation de l'action.

**Modules impactes** :
- backend : `src-tauri/src/commands/device.rs`, `src-tauri/src/lib.rs` ;
- frontend : `src/hooks/ipc/device.ts`, `src/pages/ExpertPage.tsx`, i18n ;
- suite RAID ciblee ensuite dans `ExpertPage` et, si necessaire, dans les contrats de metadonnees RAID deja exposes.

**Plan d'execution** :
1. **P0 - Ouverture chiffrement utile en mode expert**
   - exposer proprement la commande Tauri d'ouverture chiffrement ;
   - ajouter dans `ExpertPage` un formulaire reserve aux volumes decryptables avec mot de passe utilisateur, et messaging explicite sur le caractere sensible de l'action ;
   - recharger l'etat device/chiffrement apres tentative pour que la suite du workflow soit operable ;
   - verification : depuis `Expert`, un technicien peut tenter l'ouverture d'un volume chiffre avec son mot de passe puis relancer un scan/diagnostic sur l'etat actualise.
2. **P1 - Readiness RAID plus operationnelle**
   - enrichir la section readiness Expert avec lecture plus exploitable des metadonnees RAID et prochaines etapes recommandees ;
   - clarifier ce qui est detecte, ce qui peut etre prepare, et ce qui reste hors scope faute de reconstruction livree ;
   - verification : un utilisateur expert comprend immediatement si un support RAID est pret pour investigation locale, preparation importee ou escalade labo.
3. **P2 - Validation**
   - `cargo check --manifest-path src-tauri/Cargo.toml` ;
   - `npx tsc --noEmit` ;
   - `npm run test:ui` ;
   - controle Biome cible sur les fichiers modifies.

**Criteres de succes** :
- le chiffrement n'est plus seulement detecte : une action expert reelle existe dans l'app ;
- toute tentative est explicite, journalisee et reservee a un cadre prudent ;
- le volet RAID devient plus utile pour l'orientation technique meme sans reconstruction complete ;
- le produit gagne en credibilite parce qu'il aide davantage a agir qu'a seulement constater.

**Limites connues** :
- ce chantier ne livre pas de bruteforce ni de contournement chiffrement ;
- ce chantier ne livre pas encore de reconstruction RAID complete ;
- les cas avances `APFS`/`BitLocker`/`LUKS`/ensembles RAID complexes garderont des limites backend tant qu'un chantier dedie n'est pas ferme.

## Chantier 89 - Workflow RAID operable de bout en bout

**Objectif** : faire passer le RAID d'un simple diagnostic a un vrai parcours operable dans l'application, en s'appuyant sur le moteur `RaidSource` existant pour produire une source d'analyse RAID locale compatible avec le pipeline courant.

**Hypotheses** :
- le meilleur premier flux produit est de materialiser une image d'analyse RAID locale a partir des membres detectes, puis de l'injecter comme source importee ;
- on privilegie l'automatisation prudente : pre-remplir la configuration candidate detectee, tout en laissant un ajustement expert minimal sur les membres manquants et les parametres critiques ;
- on ne promet pas encore une reconstruction RAID "niveau suite pro" pour tous les cas ; on ferme d'abord le parcours principal `RAID detecte -> image d'analyse -> diagnostic/scan`.

**Risques** :
- la generation d'une image RAID locale peut etre longue et couteuse en espace disque ;
- le clustering automatique peut etre juste sur la signature mais faux sur l'ordre logique des membres dans certains cas ;
- `RAID 6` degrade a double panne n'est pas completement reconstruit dans le backend actuel ;
- il faut garder un langage honnete pour eviter de sur-vendre la couverture.

**Modules impactes** :
- backend RAID / import : `src-tauri/src/raid/mod.rs`, `src-tauri/src/imported_sources/mod.rs`, `src-tauri/src/commands/device.rs`, `src-tauri/src/lib.rs` ;
- frontend expert / devices / scan : `src/pages/ExpertPage.tsx`, hooks IPC device/import, eventuellement `DevicesPage` et `ScanPage` pour la bascule UX ;
- contrats partages : types TypeScript de devices / sources importees.

**Plan d'execution** :
1. **P0 - Contrat et artefact local**
   - ajouter une commande backend pour construire une image d'analyse RAID locale depuis une configuration explicite ;
   - enregistrer cette image comme source importee lisible par l'app ;
   - verification : une reconstruction RAID cree un nouveau device image utilisable par le moteur existant.
2. **P1 - Flux Expert pilote**
   - exposer dans `ExpertPage` un workflow "Build RAID analysis image" base sur la candidate detectee ;
   - permettre au minimum de confirmer les membres retenus et l'usage d'une configuration auto-detectee ;
   - verification : depuis l'ecran Expert, l'utilisateur peut generer puis selectionner la source RAID preparee.
3. **P2 - Bascule vers scan/diagnostic**
   - recharger les devices, selectionner la nouvelle source image RAID, et orienter vers `Diagnostic` ou `Scan` ;
   - verification : le pipeline existant peut scanner la source RAID reconstruite sans branche speciale.
4. **P3 - Validation**
   - tests unitaires backend sur la creation d'image RAID ;
   - `cargo check`, `npx tsc --noEmit`, `npm run test:ui`.

**Criteres de succes** :
- le produit sait faire plus que detecter un RAID : il peut construire une source d'analyse exploitable ;
- l'utilisateur expert peut enchainer vers un scan sans sortir de l'app ;
- toute sortie reste locale, tracee et clairement separee du support source ;
- les limites restantes (ordre, niveaux avances, RAID 6 dual-failure) sont explicites.

**Extension 2026-04-18 - mode degrade et validation d'hypotheses** :
- ajouter la possibilite de declarer explicitement des slots manquants dans la configuration RAID expert, avec limites par niveau (`RAID1`, `RAID5`, `RAID6`) ;
- afficher un verdict de validation avant build : supporte / degrade prudent / non supporte, avec raisons et niveau de confiance ;
- autoriser le build degrade seulement quand le backend sait reellement le lire sans promesse abusive ;
- verification : un utilisateur expert peut configurer un membre manquant, lire le verdict, puis lancer un build uniquement si la configuration reste honnetement supportee.

**Extension 2026-04-18 - source RAID reconstruite comme citoyen de premiere classe** :
- detecter explicitement dans l'UI qu'un `device` image selectionne provient d'une reconstruction RAID locale ;
- propager ce statut jusqu'a `Diagnostic` et `Scan`, avec notices dediees et langage centre sur la source reconstruite plutot que sur les membres bruts ;
- verification : apres build RAID, l'utilisateur ne retombe pas dans un parcours generique de simple image importee ; l'app lui indique clairement qu'il travaille maintenant sur une vue RAID reconstruite.
- etendre ensuite ce meme contexte jusqu'a `Results` et `Export`, afin que la provenance RAID reconstruite reste visible dans le triage et dans le resume de sortie ;
- verification : l'origine RAID reconstruite n'est plus perdue en fin de parcours, y compris au moment de valider un export.
- prolonger enfin cette provenance jusque dans les artefacts generés par l'application (`recovery report`, `CSV`) pour que la documentation exportee conserve elle aussi le bon contexte technique ;
- verification : un rapport ou CSV issu d'une source RAID reconstruite indique explicitement le format/source d'analyse et marque la provenance RAID reconstruite.

## Chantier 90 - Plan d'execution top-tier en 5 chantiers maximum

**Objectif** : transformer l'audit complet en feuille de route resserree vers un niveau comparable aux meilleures applications desktop de recuperation, avec un ordre d'execution qui privilegie la fermeture des parcours critiques plutot que l'accumulation de features visibles.

**Date de cadrage** : `2026-04-18`

**Constat de depart** :
- l'application est deja solide en architecture, read-only, UX guidee, export et traçabilite locale ;
- les ecarts top-tier restants sont concentres sur l'imagerie support instable, la profondeur des cas difficiles, les workflows labo avances, la preuve publique, et la fermeture de l'audit/support ;
- le produit souffre moins d'un manque de code brut que d'un manque de fermeture sur quelques capacites decisives.

**Hypotheses** :
- viser "les meilleures apps" impose de fermer d'abord ce qui fait gagner la confiance terrain : supports instables, workflows complexes, preuve publique, observabilite ;
- on garde la discipline AGENTS: lecture seule stricte, aucune promesse de magic recovery, toute action sensible traçable ;
- les chantiers existants `RAID`, `filesystem memory`, `support bundle` et `Expert` servent de base et doivent etre absorbés dans une trajectoire plus haute, pas re-ouverts sans fin.

**Risques** :
- continuer a empiler des tranches visibles sans fermer `TT-03`, `TT-04`, `TT-05` laisserait le produit "impressionnant mais pas top-tier" ;
- attaquer la preuve benchmark trop tard laisse le discours produit plus fort que la preuve ;
- attaquer des cas difficiles sans observabilite/support robustes rendrait le produit plus opaque au lieu de le rendre plus pro.

**Definition du done pour ce plan** :
- chaque chantier ci-dessous doit produire un gain mesurable de niveau produit ;
- aucun chantier n'est considere ferme sans verifications explicites et sans reduction visible d'au moins une limitation actuellement exposee a l'utilisateur ;
- l'ordre d'execution est volontairement prescriptif.

### Chantier A - Imagerie support instable niveau pro (`TT-03`, priorite absolue)

**Ce qui manque aujourd'hui**
- l'imagerie existe, mais reste marquee `limited` et n'a pas encore le niveau d'observabilite, de reprise, de rescue-map et de pilotage qu'on attend d'un outil de terrain sur support instable ;
- le produit manque encore d'une vraie confiance operateur sur les cas "disque qui lit mal / freeze / repart / degrade".

**Modules impactes**
- backend : `src-tauri/src/imaging`, `src-tauri/src/commands/imaging_cmd/*`, `src-tauri/src/commands/scan.rs`, `src-tauri/src/commands/export.rs` ;
- frontend : `src/pages/ScanPage.tsx`, `src/pages/HistoryPage.tsx`, composants timeline/logs/imaging ;
- docs/tests : `docs/top-tier-roadmap.md`, `docs/hard-case-matrix.md`, `e2e`, tests Rust d'imagerie.

**Travail attendu**
- durcir la boucle d'imagerie prudente, la reprise, les rescue maps et l'observabilite live ;
- rendre l'imagerie d'incident lisible de bout en bout dans l'UI ;
- produire un artefact d'incident/export exploitable par un technicien sans lire les logs bruts.

**Criteres de validation**
- scenario de support instable simulé avec gaps illisibles, reprise et rescue-map ;
- timeline et history montrent clairement ce qui a ete copie, saute, rescousse ou zero-fill ;
- plus aucun texte produit n'a besoin de minimiser le workflow d'imagerie sur son coeur de promesse.

**Tranche A1 - Telemetrie operateur en temps reel**
- etendre le contrat `ScanProgress` pour remonter aussi les passes ciblees de secours et les octets rescousses apres retry ;
- afficher dans `ScanPage` un vrai panneau d'incident d'imagerie regroupant profil, reprise, rescue-map, segments illisibles et secours cible ;
- verification : pendant ou apres une session d'imagerie, l'utilisateur comprend immediatement si l'imagerie est prudente, degradee, reprise ou partiellement secourue, sans devoir attendre `History`.

**Tranche A2 - Guidage d'incident et highlights live**
- ajouter dans `ScanPage` un guidage de prochaine etape sure base sur les signaux live d'imagerie (`zero-fill`, reprise, rescue-map, helper eleve, passes ciblees) ;
- remonter aussi les derniers evenements d'imagerie utiles depuis les logs techniques, sans obliger l'operateur a lire tout le journal ;
- verification : en cours d'imagerie, un operateur comprend non seulement l'etat courant, mais aussi le bon comportement a adopter et les evenements techniques marquants.

**Tranche A3 - Handoff immediat apres imagerie**
- permettre depuis `ScanPage`, une fois l'imagerie terminee, d'exporter directement le rapport d'incident et la rescue map sans passer obligatoirement par `History` ;
- garder `History` comme centre complet, mais ne plus casser le flux operateur au moment le plus sensible de la session ;
- verification : apres une imagerie terminee ou degradee, l'utilisateur peut immediatement sortir les artefacts critiques de session puis poursuivre vers l'historique si besoin.

**Tranche A4 - History comme centre d'incident d'imagerie**
- ajouter dans `HistoryPage` un resume operateur interprete pour les sessions d'imagerie, pas seulement des compteurs bruts ;
- deriver un statut lisible `stable / resumed / rescued / degraded` a partir des metriques deja persistees ;
- afficher une prochaine etape sure et des tuiles de lecture rapide directement dans le detail de session ;
- verification : en ouvrant une session d'imagerie dans `History`, un operateur comprend l'etat reel, l'impact incident, et la bonne suite sans lire toute la timeline.

**Tranche A5 - Handoff support exportable**
- enrichir le rapport d'imagerie deja exportable avec un resume operateur et une prochaine etape sure ;
- faire sortir dans le fichier transmis le meme verdict `stable / resumed / rescued / degraded` que dans `History` ;
- conserver un seul artefact d'export pour eviter les divergences entre l'UI et le support bundle ;
- verification : un rapport d'imagerie exporte depuis `History` est compréhensible par un technicien sans ouvrir l'application.

**Tranche A6 - Plages illisibles exploitables en live et en handoff**
- remonter les `unreadable_ranges` precis jusque dans `ScanProgress`, au lieu de ne montrer que des compteurs agregés pendant l'imagerie ;
- afficher dans `ScanPage` un echantillon des plus grandes plages illisibles avec offsets et tailles, pour qu'un operateur comprenne ou se trouvent les gaps les plus lourds ;
- enrichir le rapport d'incident d'imagerie avec un `UNREADABLE RANGE SAMPLE` exploitant les plages precises persistees ;
- verification : pendant une imagerie degradee, puis dans le rapport exporte, un technicien peut citer les plus gros gaps concrets sans regenerer localement la rescue map ni lire les logs bruts.

**Tranche A7 - Gaps illisibles relisibles hors session live**
- faire remonter dans `HistoryPage` un echantillon des plus grandes plages illisibles persistees, pas seulement le compteur global des gaps ;
- enrichir `imaging-handoff-summary.txt` dans le support bundle avec le nombre de plages precises et le plus gros gap, pour que le handoff support reste exploitable sans ouvrir le rapport complet ;
- verification : un technicien qui ouvre `History` ou seulement le support bundle comprend encore ou se situent les trous majeurs de lecture, meme apres la fin de la session live.

### Chantier B - Stockages avances et workflows labo (`TT-04`, apres A)

**Ce qui manque aujourd'hui**
- RAID a fortement progresse, mais NAS / VM / volumes complexes / workflows labo restent tres en dessous des leaders ;
- la detection est meilleure que la reconstruction et l'exploitation reellement productisees.

**Modules impactes**
- backend : `src-tauri/src/raid`, `src-tauri/src/virtual_disk`, `src-tauri/src/imported_sources`, `src-tauri/src/commands/device.rs`, `src-tauri/src/commands/export.rs` ;
- frontend : `src/pages/ExpertPage.tsx`, `src/pages/DevicesPage.tsx`, `src/pages/ScanPage.tsx`, `src/pages/ResultsPage.tsx`, `src/pages/ExportPage.tsx` ;
- docs/tests : `docs/hard-case-matrix.md`, `docs/top-tier-roadmap.md`, tests RAID / VHD / VMDK / imports.

**Travail attendu**
- consolider le RAID reconstruit deja livre pour les cas reellement supportes ;
- ouvrir un vrai axe `VM / images / sources labo` au meme niveau de credibilite que les sources locales ;
- fermer la boucle de preparation/import/scan/resultats/export pour ces sources.

**Criteres de validation**
- une source RAID reconstruite ou image labo complexe peut etre analysee sans retomber dans un parcours "special caché" ;
- les limites restantes sont precises et bornees par type de stockage, pas floues ;
- le produit gagne un vrai profil technician/lab, pas seulement une page Expert plus riche.

**Tranche B1 - Fiche source labo credibile**
- enrichir `ImportedRecoverySourceStatus` avec le nom enregistre et la taille logique de la source ;
- classer explicitement les sources importees `raw / forensic / virtual disk / reconstructed RAID` dans l'UI ;
- afficher dans le panneau de readiness pourquoi certaines sources exigent une preparation locale read-only avant analyse ;
- verification : `Devices`, `Diagnostic` et `Scan` distinguent clairement une image brute, un conteneur forensic, un disque virtuel et une image RAID reconstruite.

**Tranche B2 - Import guide et non ambigu**
- faire retourner le statut reel de la source des l'import backend, au lieu d'un simple succes generique ;
- afficher dans `Devices` une notice d'import qui annonce le type de source, sa taille logique, et si une preparation locale sera necessaire ;
- verification : apres import d'un `RAW`, `E01`, `VMDK` ou `VHD`, l'utilisateur sait immediatement ce qu'il a ajoute et la prochaine etape attendue.

**Tranche B3 - Tracabilite cache/analyse**
- rendre le panneau de readiness capable de montrer explicitement la chaine `source importee -> cache local derive -> chemin effectif du moteur` ;
- distinguer clairement les cas `direct`, `cache pret` et `analyse bloquee en attente de preparation` ;
- verification : un technicien peut expliquer, depuis `Devices`, `Diagnostic` ou `Scan`, quel fichier reste la preuve d'origine et quel chemin est reellement consomme par le moteur.

**Tranche B4 - Tracabilite hors UI**
- enrichir `Recovery Report`, `lab bundle summary.txt` et `manifest.json` avec une vraie section de provenance/trace des sources importees ;
- faire sortir hors de l'UI les informations `nom enregistre`, `classe de source`, `taille logique`, `source d'origine`, `cache derive` et `mode d'analyse` ;
- verification : un support ou un labo peut comprendre la chaine de preparation d'une source importee complexe sans devoir rouvrir l'application.

**Tranche B5 - CSV support-ready**
- enrichir `export_results_csv` avec les colonnes de provenance importee utiles a un support externe ;
- inclure dans le CSV `nom enregistre`, `classe de source`, `taille logique`, `source d'origine`, `etat de preparation`, `cache derive` et `mode d'analyse` ;
- verification : un CSV de resultats suffit a recoller la provenance d'une source importee complexe sans ouvrir le rapport HTML ni l'application.

**Tranche B6 - Support reel des formats labo importes**
- enrichir `ImportedRecoverySourceStatus` avec un vrai statut `supported / limited / unsupported`, une note de support et une prochaine etape sure ;
- distinguer explicitement un `RAW`/`E01` d'un `VMDK`/`VHD` et surtout d'un `VHDX` non encore supporte, au lieu de tout reducer a `prepared / not prepared` ;
- verification : le panneau de source importee annonce franchement le niveau de support moteur du format et la bonne suite, sans laisser croire qu'un format non pris en charge est seulement "a preparer".

### Chantier C - APFS, chiffrement et cas macOS difficiles (`TT-05`, en parallele ou juste apres B)

**Ce qui manque aujourd'hui**
- APFS snapshot/clone, APFS chiffre, cas macOS compliques et workflows pre-unlock restent encore des trous top-tier explicites ;
- le chiffrement est mieux pilote qu'avant, mais pas encore au niveau "cas difficiles macOS / volumes modernes".

**Modules impactes**
- backend : `src-tauri/src/analyzers`, `src-tauri/src/encryption`, `src-tauri/src/commands/device.rs`, `src-tauri/src/commands/scan.rs` ;
- frontend : `src/pages/DevicesPage.tsx`, `src/pages/DiagnosticPage.tsx`, `src/pages/ExpertPage.tsx`, `src/pages/ResultsPage.tsx` ;
- docs/tests : `docs/hard-case-matrix.md`, fixtures APFS/chiffrement, bench corpus.

**Travail attendu**
- fermer les cas macOS a plus fort impact produit ;
- transformer la detection chiffrement/APFS en parcours plus operables ;
- durcir la verite produit pour ce qui est supporte, pre-unlock, post-unlock, snapshot, clone, orphelin, etc.

**Criteres de validation**
- reduction nette des `Gap` / `Partial` APFS/chiffrement dans la matrice des cas durs ;
- parcours utilisateur plus clair entre "ce qui est bloqué par la cle", "ce qui est scannable", "ce qui est reconstituable" ;
- plus grande parite avec les outils de reference sur macOS.

**Tranche C1 - Chiffrement pre-unlock operable**
- enrichir `EncryptionInfo` avec un vrai etat operateur et une prochaine etape sure, au lieu d'un simple `detected / canUnlock` ;
- faire apparaitre dans l'UI avancee si la source est encore bloquee avant deverrouillage et quelle suite est recommandee ;
- verification : un support comprend immediatement qu'un volume chiffre detecte n'est pas encore une surface fiable de recuperation tant qu'il reste verrouille.

**Tranche C2 - Diagnostic verrouille par chiffrement**
- faire evoluer `build_diagnostic` pour que les recommandations normales cessent d'etre marquees comme conseillees quand la vue reste verrouillee par chiffrement ;
- afficher dans `DiagnosticPage` un rappel explicite que la vue courante est encore bloquee avant deverrouillage ;
- verification : sur une source chiffree verrouillee, le hero action et les notices du diagnostic n'encouragent plus un scan "normal" comme si le systeme de fichiers etait deja lisible.

**Tranche C3 - Scan verrouille avant lancement**
- charger l'etat chiffrement directement dans `ScanPage` pour que la page sache si la vue courante reste bloquee avant deverrouillage ;
- desactiver les workflows incompatibles, afficher une banniere explicite "image read-only seulement", et refuser aussi `beginScan` / l'auto-start si un mode non autorise tente de partir ;
- verification : sur une source chiffree verrouillee, `Scan` ne peut plus lancer par erreur un quick/deep/deleted/signature/reconstruction comme si le systeme de fichiers etait deja lisible, tandis que l'imagerie read-only du chiffre brut reste disponible.

**Tranche C4 - Resultats APFS plus operables**
- ajouter dans `Results` un resume operateur explicite pour les cas APFS supprimés / derives de snapshot / derives du journal ;
- fournir des actions rapides de tri par provenance (`catalogue courant`, `snapshot`, `journal`) pour eviter que l'utilisateur doive reconstruire seul la fragilite des resultats APFS ;
- verification : sur un jeu de resultats APFS complexe, un technicien peut isoler immediatement les fichiers les plus prudents a exporter et ceux qui demandent plus de verification.

**Tranche C5 - Export APFS plus explicite**
- propager le resume de provenance APFS jusque dans le wizard d'export pour la selection courante ;
- distinguer clairement ce qui vient du catalogue courant, d'un snapshot ou du journal au moment de confirmer la sortie ;
- verification : avant export, un technicien voit immediatement combien de fichiers APFS demandent une verification supplementaire et lesquels restent les plus prudents a sortir.

**Tranche C6 - Diagnostic APFS plus tranchant**
- enrichir le diagnostic avec des limites APFS plus precises pour `snapshots non livres` et `pre-unlock chiffre` au lieu de rester sur un bloc APFS trop generique ;
- ajouter dans `DiagnosticPage` une vue operateur APFS qui distingue clairement `catalogue courant`, `orphelins supprimes`, `snapshot`, `pre-unlock` ;
- verification : sur un device ou candidat APFS, le diagnostic explique sans ambiguite ce qui est scannable maintenant, ce qui reste un workflow conservateur, et ce qui n'est pas encore livre.

**Tranche C7 - Detection snapshots APFS montes**
- detecter en best-effort si un volume APFS monte expose des snapshots locaux sur macOS ;
- remonter ce signal dans le diagnostic pour distinguer `snapshots detectes mais non livres` de `etat snapshot inconnu` ;
- verification : sur un volume APFS monte, le diagnostic peut indiquer qu'un historique snapshot existe reellement meme si son exploitation n'est pas encore livree.

**Tranche C8 - Provenance moteur des orphelins APFS**
- faire remonter les resultats APFS supprimes comme derives de `catalogue courant` plutot que sous une provenance generique `recovery-image` ;
- appliquer la meme verite de provenance dans le scan direct APFS et le scan APFS depuis volume retrouve ;
- verification : un resultat APFS supprime apparait avec une provenance coherente jusqu'a `Results` et `Export`, et un test APFS cible verrouille ce contrat.

**Tranche C9 - Qualification expert des orphelins APFS**
- enrichir les resultats APFS supprimes avec une qualification backend honnete (`validator_status`, `recovery_complexity`, segments, gaps) au lieu de laisser ces champs vides ;
- appliquer cette qualification aux deux chemins APFS deja livres (`scan direct` et `volume retrouve`) ;
- rendre la complexite visible dans `Results` pour que le gain backend soit lisible sans ouvrir d'outil expert ;
- verification : un orphelin APFS expose une provenance `live-catalog`, un statut de validation `unsupported`, une complexite derivee du scoring existant, et un test cible verrouille ce contrat.

**Tranche C10 - Triage IA des orphelins APFS qualifies**
- faire compter les orphelins APFS `live-catalog + unsupported` dans le chemin `complex review / preview first` du brief IA au lieu de les laisser glisser vers `export now` sur le seul score ;
- ajouter une caution, une etape suivante, un blocage et un resume de complexite explicites pour ces cas ;
- aligner le browser preview IA sur cette meme prudence descriptive ;
- verification : un candidat APFS supprime qualifie ne peut plus etre presente comme `export now` par le brief IA, et un test dedie verrouille ce triage.

**Tranche C11 - Lot d'export par defaut plus prudent**
- quand aucun fichier n'a ete selectionne explicitement dans `Results`, retirer automatiquement du lot implicite les candidats APFS `preview-first` s'il existe deja des fichiers plus simples a exporter ;
- garder le lot complet si tous les fichiers sont dans ce cas, pour ne pas bloquer artificiellement l'utilisateur ;
- rendre cette retenue visible dans l'assistant d'export pour que le comportement reste explicite et reversible via une selection explicite dans `Results` ;
- verification : un export lance sans selection explicite ne pousse plus par defaut les orphelins APFS non valides dans le lot de masse, et des tests TS verrouillent cette selection conservative.

**Tranche C12 - Traçabilité export/report des candidats retenus hors lot**
- faire remonter la retenue du lot implicite jusque dans le rapport HTML, le CSV et les artefacts du lab bundle ;
- documenter combien de fichiers sont exclus du lot implicite, pourquoi, et lesquels sont concernes quand la vue live du scan est disponible ;
- verification : un support peut comprendre hors UI pourquoi certains candidats APFS n'ont pas ete inclus dans le lot implicite, et un test Rust verrouille la logique de retenue cote backend.

**Tranche C13 - Posture chiffrement lisible a la source**
- completer la lecture `detected / workflow_state` par une vraie posture operateur visible des la fiche support (`surface normale`, `deverrouiller d'abord`, `chemin labo seulement`) ;
- verification : depuis `Devices`, un technicien comprend immediatement si la vue actuelle est une vraie surface de scan ou seulement une surface a imager / deverrouiller, sans devoir interpreter lui-meme `pre_unlock_blocked`.

### Chantier D - Preuve publique et scorecard marche (`TT-01`, a ne plus repousser)

**Ce qui manque aujourd'hui**
- sans benchmark public reproductible, impossible de soutenir un niveau top-tier avec credibilite ;
- l'app peut progresser techniquement sans que le marche le voie ni qu'on sache objectivement ou elle se situe.

**Modules impactes**
- `benchmarks/`, `docs/benchmark-market.md`, `docs/top-tier-roadmap.md`, `README.md`, workflows CI lies au benchmark.

**Travail attendu**
- finir le corpus prioritaire ;
- produire une campagne comparative exploitable ;
- maintenir une scorecard vivante reliee aux chantiers A/B/C.

**Criteres de validation**
- publication interne exploitable puis publication externe quand les donnees sont defendables ;
- comparaison explicite contre au moins deux comparateurs accessibles sans achat supplementaire sur un protocole stable, par défaut `PhotoRec` et `TestDisk`, les suites payantes restant optionnelles ;
- le benchmark cesse d'etre une ambition et devient un instrument de pilotage.

**Tranche D1 - Scorecard versionnee dans le repo**
- ajouter une scorecard top-tier datee et versionnee dans `benchmarks/`, reliee aux chantiers `TT-03`, `TT-04`, `TT-05` et `TT-01` ;
- separer clairement cette scorecard des result files de benchmark pour ne pas casser les validateurs existants ;
- verification : le repo expose un artefact stable de pilotage benchmark/top-tier, et `npm run benchmark:check` reste vert.

**Tranche D2 - Campagne comparative publique v1**
- figer un runbook public et executable pour la premiere campagne comparative au lieu de laisser `TT-01` au stade d'intention ;
- rendre le template de resultats pre-remplissable depuis la ligne de commande pour un run concurrent reel accessible sans achat supplementaire (`PhotoRec`, `TestDisk`, puis `DMDE` free si disponible), les outils payants devenant optionnels ;
- faire du report HTML un vrai artefact de comparaison, avec couverture `P0`, statuts `unsupported / blocked / not-run`, et seuil de publication visible ;
- verification : un operateur peut preparer un result file concurrent sans editer le schema a la main, `npm run benchmark:report` produit une comparaison exploitable, et la roadmap ne sur-vend plus la fermeture de `TT-01`.

### Chantier E - Audit, history, reporting et support de niveau pro (`TT-07` + support)

**Ce qui manque aujourd'hui**
- l'audit et les traces existent, mais pas encore comme centre de preuve/support pleinement ferme ;
- `History`, `lab bundle`, reports et logs restent encore trop fragmentes pour un vrai niveau operations/support.

**Modules impactes**
- backend : `src-tauri/src/audit`, `src-tauri/src/commands/audit.rs`, `src-tauri/src/commands/export.rs`, `src-tauri/src/commands/scan.rs` ;
- frontend : `src/pages/HistoryPage.tsx`, `src/pages/SettingsPage.tsx`, `src/pages/ExpertPage.tsx`, eventuellement `ResultsPage.tsx` ;
- docs/tests : flows support, audit chain, support bundle/lab bundle.

**Travail attendu**
- faire de `History` un vrai centre d'investigation et pas seulement un listing ;
- aligner rapports, bundles, timeline et audit chain ;
- rendre toute action sensible ou toute limitation importante facile a reconstituer apres coup.

**Criteres de validation**
- un technicien peut comprendre un cas, son contexte et ses limites sans repasser par le terminal ;
- les bundles support/lab et l'historique racontent la meme histoire ;
- l'app gagne en credibilite entreprise/support, pas seulement en UX individuelle.

## Ordre d'execution recommande

1. **Chantier A** — imagerie support instable niveau pro
2. **Chantier B** — stockages avances et workflows labo
3. **Chantier C** — APFS, chiffrement et cas macOS difficiles
4. **Chantier E** — audit, history, reporting, support
5. **Chantier D** — preuve publique et scorecard benchmark

## Pourquoi cet ordre

- `A` est le socle de credibilite terrain le plus important ;
- `B` et `C` ferment ensuite les cas complexes qui distinguent vraiment les leaders ;
- `E` transforme les progres techniques en produit supportable et defendable ;
- `D` doit avancer en continu, mais sa publication vaut surtout une fois `A/B/C` suffisamment murs.

## Regle de pilotage

- ne pas ouvrir un `Chantier 91+` de surface tant que `A` n'a pas un plan valide et une tranche implementable ;
- toute nouvelle feature visible doit prouver qu'elle ferme un ecart de cette feuille de route ;
- si un lot n'eleve pas clairement la position de l'app face aux meilleures suites, il est secondaire.

### Tranche C13 - Historique export et support bundle conscients du lot prudent

**Objectif**
- conserver la posture d'export prudent APFS au-dela du wizard `Export`, jusque dans `History` et les artefacts support.

**Backend**
- persister dans `ExportSessionSummary` si l'export vient d'une selection explicite ou du lot implicite prudent ;
- persister le nombre de candidats `preview-first` APFS retenus hors lot au lancement ;
- enrichir le support bundle avec un resume lisible de cette posture d'export.

**Frontend**
- remonter ces champs dans `History` ;
- afficher clairement le mode de selection et le holdout APFS eventuel dans les details d'export.

**Validation**
- tests Rust sur la persistance/résumé ;
- `cargo check`, `npx tsc --noEmit`, `npm run test:ui`.

### Tranche C14 - Triage IA exportable hors UI

**Objectif**
- faire vivre le signal `APFS preview-first` au-dela de `Results`, jusque dans le brief IA local et les artefacts d'escalade.

**Backend**
- exposer un compteur dédié `apfs_catalog_preview_first` dans `AiRecoveryCounts` ;
- injecter un résumé du brief IA local dans le rapport HTML et dans le `lab bundle` quand un scan live est attaché ;
- écrire un artefact `ai-recovery-brief.json` dans le bundle labo.

**Frontend**
- afficher ce compteur dédié dans le panneau `AiRecoveryBriefPanel`.

**Validation**
- test Rust sur le brief IA APFS ;
- `cargo check`, `npx tsc --noEmit`, `npm run test:ui`.

### Tranche C15 - Réduction honnête des APFS `unsupported`

**Objectif**
- faire sortir une partie des orphelins APFS du statut trop large `unsupported` quand les métadonnées sont déjà cohérentes, sans jamais prétendre à une validation structurelle complète.

**Backend**
- qualifier en `reassembled` les cas APFS supprimés qui sont:
  - complets en taille,
  - contigus en un seul byte-run,
  - et dont l'extension a été inférée depuis la signature des octets de prévisualisation ;
- conserver `partial-unvalidated` pour les cas incomplets, et `unsupported` pour le reste.

**Effet produit**
- réduire une partie des cas `preview-first` artificiellement trop prudents ;
- garder une sémantique honnête: `reassembled` ne veut pas dire `validated`.
- aligner le triage: `reassembled` sort du lot prudent par defaut, tandis que `partial-unvalidated` y reste avec `unsupported`.

**Validation**
- tests unitaires sur la qualification APFS ;
- mise à jour du test APFS supprimé existant ;
- `cargo check`, `npx tsc --noEmit`, `npm run test:ui`.

### Tranche C16 - APFS Preuve 3 sur les multi-run complets

**Objectif**
- sortir une autre partie des `unsupported` quand l'orphelin APFS supprimé reste complet mais distribué sur plusieurs byte-runs sans trou.

**Backend**
- qualifier en `reassembled` les cas:
  - complets en taille,
  - multi-run mais sans gap,
  - avec extension inférée depuis la signature des octets de prévisualisation ;
- conserver une complexité `medium` via `assembly_segment_count > 1`.

**Effet produit**
- réduire encore les faux cas `preview-first` trop prudents ;
- garder une sémantique honnête: multi-run complet != validation structurelle.

**Validation**
- test unitaire dédié sur un cas APFS multi-run complet ;
- `cargo check`, `cargo test` ciblé.

### Tranche C17 - APFS Preuve 4 avec timestamps catalogue

**Objectif**
- ne plus faire dépendre la promotion `reassembled` de la seule signature de preview.

**Backend**
- exiger, en plus du payload complet et d'une extension inférée, la présence de timestamps catalogue `created_at` et `modified_at` pour promouvoir un orphelin APFS supprimé en `reassembled`.

**Effet produit**
- rendre `reassembled` plus crédible et plus défendable techniquement ;
- garder `unsupported` pour les payloads signature-backed mais trop pauvres en preuve catalogue.

**Validation**
- tests unitaires ciblés sur:
  - cas single-run avec timestamps,
  - cas multi-run complet avec timestamps,
  - cas signature-backed sans timestamps qui doit rester `unsupported`.

### Tranche C18 - APFS Preuve 5 visible dans le produit

**Objectif**
- rendre la différence `preview-first` / `reassembled` compréhensible directement dans l'application, pas seulement dans les statuts techniques.

**Frontend**
- ajouter un helper partagé pour reconnaître les candidats APFS `reassembled` du catalogue courant ;
- afficher dans `Results` un signal opérateur dédié quand des fichiers APFS supprimés du catalogue courant sont maintenant qualifiés `reassembled` ;
- afficher dans `Export` cette même lecture avec:
  - un compteur dédié,
  - une notice claire disant qu'ils sont plus exploitables que les candidats retenus hors lot,
  - mais qu'ils ne sont toujours pas structurellement validés.

**Effet produit**
- arrêter de mélanger visuellement les cas APFS `reassembled` avec les cas `preview-first` ;
- garder une posture honnête: `reassembled` améliore la preuve, sans transformer ces fichiers en cas “garantis”.

**Validation**
- test unitaire sur le helper `isApfsCatalogReassembledCandidate` ;
- `npx vitest run src/utils/resultFilters.test.ts` ;
- `npx tsc --noEmit` ;
- `npx biome check` sur les fichiers frontend touchés.

### Tranche C19 - APFS Preuve 6 exportable, traçable et cohérente hors UI

**Objectif**
- propager enfin la distinction `preview-first` / `reassembled` jusque dans les artefacts backend et support, pour que les rapports et bundles racontent la même vérité que l'UI.

**Contrats**
- étendre `AiRecoveryCounts` avec un compteur `apfs_catalog_reassembled` ;
- aligner les mappings Rust/TypeScript/browser preview sur ce nouveau contrat ;
- corriger le backend export pour retenir hors lot implicite les cas `partial-unvalidated` comme l'UI le fait déjà.

**Backend**
- `build_scan_recovery_brief` doit compter séparément:
  - les APFS `preview-first` (`unsupported` + `partial-unvalidated`),
  - les APFS `reassembled` ;
- enrichir le brief IA avec une preuve, une caution et un résumé de complexité dédiés aux cas `reassembled` ;
- enrichir `generate_recovery_report`, `export_results_csv` et `generate_lab_bundle` avec:
  - le compteur APFS `reassembled`,
  - la vue default-batch corrigée,
  - la traçabilité des fichiers `reassembled` ;
- enrichir `build_support_bundle_archive_bytes` avec un résumé des briefs IA des scans live pour le support.

**Effet produit**
- plus de contradiction entre `Results`, `Export`, `Recovery Report`, `lab bundle` et `support bundle` ;
- les cas APFS `reassembled` sortent du non-dit et deviennent traçables ;
- les cas `partial-unvalidated` ne sont plus sous-comptés dans les artefacts backend.

**Validation**
- `cargo check --manifest-path src-tauri/Cargo.toml` ;
- tests Rust ciblés sur:
  - le brief IA APFS `reassembled`,
  - le support bundle,
  - le holdout APFS `partial-unvalidated` ;
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check` sur les fichiers touchés.

### Tranche E1 - History et support bundle conscients de la provenance opérateur

**Objectif**
- faire de `History` un vrai centre opérateur en affichant la provenance réelle des scans et exports, pas seulement leur statut technique.

**Contrats**
- enrichir `ScanSessionSummary` et `ExportSessionSummary` avec:
  - source enregistrée,
  - type de source importée,
  - format,
  - chemin d'analyse,
  - état de préparation,
  - signal `reconstructed_raid_source` ;
- conserver des defaults `serde` pour rester compatibles avec les archives locales déjà présentes.

**Backend**
- dériver la provenance des scans depuis les sources importées enregistrées ;
- propager cette provenance aux exports via le scan associé ;
- enrichir le support bundle avec:
  - `scan-provenance-summary.txt`,
  - `scan-history.json` / `export-history.json` enrichis,
  - `export-posture-summary.txt` capable de rappeler le type de source.

**Frontend**
- afficher dans `History`:
  - badges de provenance utiles dans les listes,
  - détails de source dans la vue session/export,
  - signal clair quand une session repose sur une source RAID reconstruite,
  - signal clair quand une source importée n'était pas encore préparée.

**Effet produit**
- un technicien peut relire l'historique d'un cas et comprendre sur quelle source réelle le travail a été mené ;
- la traçabilité du contexte `RAW / forensic / virtual disk / reconstructed RAID` ne s'arrête plus à l'écran courant ;
- les bundles support deviennent plus exploitables humainement sans parser tout le JSON.

**Validation**
- `cargo check --manifest-path src-tauri/Cargo.toml` ;
- `cargo test --manifest-path src-tauri/Cargo.toml build_support_bundle_archive_bytes_contains_manifest_histories_and_logs -- --nocapture` ;
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check` sur les fichiers touches.

### Tranche E2 - Artefacts d'imagerie alignes avec la provenance opérateur

**Objectif**
- faire en sorte qu'un rapport d'incident d'imagerie ou une rescue map transporte le meme contexte source que `History`, au lieu de redevenir un artefact anonyme de bas niveau.

**Contrats**
- reutiliser uniquement les champs deja persistés dans `ScanSessionSummary` :
  - source enregistrée,
  - type de source,
  - format,
  - chemin d'analyse,
  - disponibilité,
  - etat de preparation,
  - signal `reconstructed_raid_source` ;
- ne pas introduire de second circuit de vérité spécifique aux rapports d'imagerie.

**Backend**
- enrichir `build_imaging_session_report` avec une section `SOURCE PROVENANCE` quand le contexte existe ;
- enrichir `build_imaging_rescue_map` avec des commentaires d'en-tete rappelant la provenance et l'etat de preparation ;
- couvrir les deux artefacts par des tests Rust relies a des `ScanSessionSummary` persistés.

**Effet produit**
- un technicien qui reçoit uniquement le rapport d'imagerie ou la rescue map comprend aussi s'il travaille depuis une source brute, un conteneur forensic préparé, ou une image RAID reconstruite ;
- la notion de cache/preparation locale ne disparaît plus au moment de l'escalade ou du handoff ;
- les artefacts d'imagerie deviennent cohérents avec `History`, `Support Bundle` et les autres sorties produit.

**Validation**
- `cargo check --manifest-path src-tauri/Cargo.toml` ;
- `cargo test --manifest-path src-tauri/Cargo.toml generate_imaging_session_report_summarizes_resume_and_unreadable_context -- --nocapture` ;
- `cargo test --manifest-path src-tauri/Cargo.toml generate_imaging_rescue_map_builds_ddrescue_style_blocks -- --nocapture` ;

### Tranche E3 - Support bundle embarque les vrais artefacts d'imagerie

**Objectif**
- faire d'un support bundle un vrai paquet de handoff pour les cas d'imagerie, sans obliger le technicien distant a regenerer localement rapport d'incident et rescue map.

**Backend**
- detecter les sessions de scan liees a l'imagerie dans l'historique/support ;
- embarquer dans le support bundle :
  - `imaging-handoff-summary.txt`,
  - `imaging-reports/<scan>.txt`,
  - `imaging-rescue-maps/<scan>.map` quand disponible ;
- rester prudent :
  - aucun octet source ni contenu recupere n'est ajoute,
  - on reutilise les generateurs d'artefacts deja existants au lieu de dupliquer la logique.

**Effet produit**
- un support ou un labo recoit enfin le bundle complet du cas imaging ;
- l'etat operateur (`stable / resumed / rescued / degraded`) et la rescue map sont transportes avec le reste du dossier ;
- moins de friction pour diagnostiquer un cas distant ou reprendre un incident.

**Validation**
- `cargo check --manifest-path src-tauri/Cargo.toml` ;
- `cargo test --manifest-path src-tauri/Cargo.toml build_support_bundle_archive_bytes_contains_manifest_histories_and_logs -- --nocapture` ;

### Tranche E4 - History redevient actionnable pour les cas d'imagerie

**Objectif**
- permettre a un technicien de repartir depuis l'historique vers le bon ecran de travail, au lieu de n'y trouver que des artefacts passifs.

**Frontend**
- depuis une session imaging dans `History` :
  - proposer `Review in Devices` si la source n'est plus detectee ou si une preparation locale reste necessaire ;
  - proposer `Open in Diagnostic` et `Open in Scan` quand le support est encore disponible ;
  - re-selectionner le bon device dans le store avant navigation pour eviter toute reconstitution manuelle du contexte.

**Effet produit**
- moins de friction operateur entre lecture d'un incident imaging et reprise de travail ;
- `History` cesse d'etre une archive morte et devient un veritable point de relance ;
- le comportement reste honnete : on n'ouvre `Diagnostic` / `Scan` directement que quand la source est encore presente.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche E5 - History redevient actionnable aussi pour les exports

**Objectif**
- eviter qu'un export ancien soit seulement une ligne d'historique morte alors que le contexte source existe encore localement.

**Frontend**
- depuis les details d'un export dans `History` :
  - proposer `Review Source in Devices` si la source n'est plus detectee ou qu'une preparation locale reste necessaire ;
  - proposer `Reopen in Diagnostic` et `Reopen in Scan` quand le support source du scan associe est encore disponible ;
  - reutiliser le `scan_id` associe pour retrouver le bon device dans l'historique de scan, puis re-selectionner ce device dans le store avant navigation.

**Effet produit**
- `History` devient un vrai point de reprise aussi pour les cas exportes ;
- un support peut repartir d'un ancien export sans retrouver manuellement le bon support ;
- l'app reste honnete: pas de faux lien vers `Results` si les resultats ne sont plus garantis vivants dans le runtime.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche E6 - History redevient actionnable pour tous les scans historiques

**Objectif**
- terminer la transformation de `History` en vrai point de reprise, y compris pour les scans standards non lies a l'imagerie.

**Frontend**
- dans les details d'une session de scan historique :
  - proposer `Review Scan Source in Devices` si la source n'est plus detectee ou demande encore une preparation ;
  - proposer `Reopen Scan in Diagnostic` et `Reopen Scan in Scan` quand le support est encore disponible ;
  - conserver la logique specialisee imaging deja livree pour les rapports et rescue maps.

**Effet produit**
- l'historique devient un centre de relance global ;
- moins de friction pour reprendre un cas ancien, qu'il s'agisse d'un scan quick/deep/deleted ou d'une session imaging ;
- pas de promesse exageree: on relance le workflow vivant, pas un faux instantane complet de l'ancien contexte.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche E7 - Escalade support directement depuis History

**Objectif**
- permettre a un technicien d'exporter un support bundle directement depuis le contexte historique d'une session ou d'un export, sans repasser par `Settings`.

**Frontend**
- ajouter `Export Support Bundle` dans les details de session et d'export ;
- nommer le bundle avec l'identifiant du contexte historique courant ;
- reutiliser le `devicePath` quand disponible pour garder la validation de destination sure ;
- rester utilisable meme si le support n'est plus present, tant que l'export du bundle lui-meme reste possible.

**Effet produit**
- `History` devient aussi un point d'escalade support ;
- moins d'aller-retours entre ecrans pour preparer un dossier support ;
- le geste operateur est plus naturel: consulter un cas ancien puis sortir immediatement le bundle associe.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche E8 - Export de timeline technique cible depuis History

**Objectif**
- permettre d'exporter depuis `History` un handoff technique plus fin qu'un bundle global, en se concentrant sur le cas selectionne.

**Frontend**
- ajouter `Export Case Timeline` dans les details de session et d'export ;
- pour une session de scan : exporter la timeline technique de ce scan ;
- pour un export : exporter une timeline fusionnee `scan + export` via la brique existante `technicalTimeline` ;
- reutiliser `save_technical_timeline_report` et la validation de destination sure quand le `devicePath` est connu.

**Effet produit**
- un support peut partager rapidement la chronologie technique d'un cas sans sortir tout le bundle ;
- `History` devient un vrai point de handoff technique cible ;
- l'app reutilise la logique de timeline deja existante au lieu de dupliquer les formats.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche E9 - Resume de cas exportable depuis History

**Objectif**
- fournir un handoff humain court et lisible depuis `History`, en plus de la timeline brute et du bundle global.

**Frontend**
- ajouter `Export Case Summary` dans les details de session et d'export ;
- pour une session de scan :
  - resumer identite, source, format, preparation, volume RAID reconstruit eventuel,
  - inclure le resume operateur d'imagerie quand le cas concerne l'imagerie ;
- pour un export :
  - resumer destination, selection mode, volume source, et erreurs enregistrees ;
- reutiliser `save_technical_timeline_report` comme ecrivain read-only de rapport texte cible.

**Effet produit**
- un technicien peut envoyer rapidement un resume de cas comprehensible a un tiers sans sortir tout le bundle support ;
- `History` devient un vrai point de handoff humain, pas seulement de logs ;
- la sortie reste sobre et honnete, sans inventer de preuve supplementaire.

**Validation**
- `npx tsc --noEmit` ;
- `npm run test:ui` ;
- `npx biome check src/pages/HistoryPage.tsx src/i18n/locales/en.json src/i18n/locales/fr.json` ;

### Tranche F1 - Session top-tier benchmark / rescue / coherence produit

**Objectif**
- reduire franchement le verrou `TT-01` autour de `apfs_deleted_orphan_catalog_v1` sans pretendre le fermer si la fixture reste instable ;
- transformer la couche benchmark publique en instrument de pilotage plus net, avec distinction explicite entre `completed`, `unsupported`, `blocked` et `not-run` ;
- livrer une premiere tranche credible pour `TT-02` bootable rescue, coherente avec la posture read-only ;
- resserrer la coherence repo/docs/scorecard/release autour des chantiers `TT-03`, `TT-04`, `TT-05`, `TT-07` et `TT-08`.

**Hypotheses**
- la fermeture complete de `APFS P0` depend peut-etre encore d'un comportement APFS/macOS 15.7.4 hors de controle du repo, donc la meilleure sortie de session peut etre une reduction documentee du blocage plutot qu'un faux `done` ;
- la marche la plus utile pour `TT-02` dans cette session est un workflow de secours bootable borne, scriptable et verifiable, pas un environnement de secours universel multi-materiel ;
- la meilleure passe `TT-07` a court terme est un durcissement tamper-evident et de la tracabilite des artefacts, plutot qu'une promesse prematuree de signature PKI complete.

**Risques**
- un report benchmark qui melange baseline interne, campagne publique, spot-checks et runs bloques reste ambigu meme si les fichiers existent ;
- annoncer un workflow bootable trop large ferait retomber le repo dans une posture "serieuse en apparence, pas encore prouvee" ;
- toute tentative APFS trop agressive risque d'etre fragile ou trop dependante du host, ce qui irait contre la surete et l'honnetete produit.

**Modules impactes**
- `PLANS.md`
- `benchmarks/corpus/v1/manifest.json`
- `benchmarks/results/*.json`
- `benchmarks/README.md`
- `benchmarks/scorecard-v1.md`
- `benchmarks/scorecard-v1.json`
- `benchmarks/public-comparative-campaign-v1.md`
- `docs/top-tier-roadmap.md`
- `docs/benchmark-market.md`
- `docs/hard-case-matrix.md`
- `README.md`
- `scripts/benchmark-manifest.mjs`
- `scripts/benchmark-results.mjs`
- `scripts/benchmark-report.mjs`
- `scripts/release-preflight.mjs`
- `scripts/generate-release-manifest.mjs`
- `src-tauri/src/analyzers/apfs.rs`
- `src-tauri/src/audit/mod.rs`
- `src-tauri/src/commands/export.rs`

**Plan d'execution**
1. qualifier l'etat reel `APFS P0` et choisir entre stabilisation technique ou blocage reduit avec preuve exploitable ;
   verification : test cible APFS ou evidence technique benchmarkable, plus statut manifeste/resultats/report coherents.
2. durcir la couche benchmark publique ;
   verification : templates/resultats/report distinguent clairement portee de run, couverture de campagne et statuts faibles sans ambiguite.
3. livrer une premiere tranche `TT-02` rescue bootable ;
   verification : artefact ou workflow documente dans le repo, borne, read-only, avec limites explicites et controles de preflight.
4. renforcer la tracabilite premium et la coherence produit ;
   verification : scorecard/roadmap/docs/release et artefacts tamper-evident racontent le meme etat reel.

**Criteres de validation**
- `npm run benchmark:check` ;
- `npm run benchmark:report` ;
- au moins un test cible pour le chemin APFS touche ou une preuve technique equivalentement exploitable ;
- les docs `README`, roadmap, scorecard et benchmark ne se contredisent plus sur `TT-01` a `TT-08` ;
- si un slice `TT-02` est livre, son perimetre bootable reel et ses limites materielles restent ecrits noir sur blanc ;
- toute sortie `TT-07` ou `TT-08` ajoute une preuve repo-verifiable, pas seulement du discours.

**Limites connues**
- cette session ne suffira probablement pas a faire passer tout `TT-01` a `closed` ;
- `TT-02` ne sera pas "leader-grade" apres une seule tranche ; l'objectif est un MVP de secours credible ;
- `TT-03`, `TT-04`, `TT-05`, `TT-07` et `TT-08` peuvent seulement avancer par fermetures partielles, pas par declaration globale de parite.
