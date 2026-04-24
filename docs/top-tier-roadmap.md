# Récupère — Roadmap haut de panier

Statut: `actif`  
Date de départ: `2026-04-11`  
Référence d'audit: `audit comparatif marché du 2026-04-11`  
Document complémentaire: [`docs/benchmark-market.md`](./benchmark-market.md)

## 1. But

Faire de Récupère une alternative crédible au plus haut niveau du marché desktop data recovery, sans trahir les principes fondateurs du projet:

- sûreté d'abord;
- exactitude avant marketing;
- lecture seule stricte sur la source;
- IA locale et honnête;
- traçabilité forte;
- UX rassurante pour novice et exploitable pour expert.

Ce document devient la source de vérité pour suivre ce qu'il reste à faire jusqu'au niveau "top tier".

## 2. Verdict de départ

Au `2026-04-11`, Récupère est:

- déjà fort en sûreté, audit, architecture, UX guidée et discipline produit;
- en dessous des leaders globaux sur la preuve publique, les workflows de secours bootables, l'imagerie de supports instables, le stockage avancé, certains cas APFS/chiffrement et la maturité terrain.

Concurrents de référence à viser:

- `R-Studio`
- `UFS Explorer Professional / Technician`
- `DMDE`
- `Stellar Toolkit`
- `Disk Drill`

## 3. Définition du "done"

Récupère pourra être considéré "au plus haut panier" quand les conditions suivantes seront réunies:

- benchmark public reproductible publié, avec protocole, corpus, métriques et résultats vérifiables;
- parité crédible ou avantage clair sur plusieurs classes de cas réels importantes;
- workflow bootable de secours prêt pour les machines non bootables;
- workflow d'imagerie de supports instables jugé sérieux et observable de bout en bout;
- couverture avancée RAID / NAS / VM / volumes complexes exposée proprement dans l'UI;
- durcissement cross-platform suffisant pour éviter les angles morts d'accès raw aux supports;
- UX novice/expert irréprochable sur les cas à risque;
- validation CI et validation locale solides sur macOS, Windows et Linux;
- documentation produit honnête, cohérente et à jour.

## 4. Hypothèses globales

- le différenciateur principal n'est pas "plus d'IA", mais "plus de sûreté, plus de preuve, plus de workflows sérieux";
- le plus gros écart face aux leaders n'est pas le manque de code brut, mais le manque de preuve publique et de couverture des cas difficiles;
- la base actuelle du repo est suffisamment solide pour construire par tranches sans refonte totale;
- les chantiers doivent rester traçables et testables, sans promesse magique sur les données physiquement détruites.

## 5. Risques globaux

- vouloir élargir trop vite le périmètre au détriment de la fiabilité;
- livrer des workflows avancés sans assez de corpus réels ni d'observabilité;
- dégrader la simplicité novice en ajoutant des options expertes mal cadrées;
- créer une dette documentaire entre ce que l'app fait réellement et ce que les docs disent;
- laisser l'IA envahir des zones où seules des garanties techniques et des preuves comptent.

## 6. Modules impactés

- `src-tauri/src/core`
- `src-tauri/src/imaging`
- `src-tauri/src/analyzers`
- `src-tauri/src/raid`
- `src-tauri/src/virtual_disk`
- `src-tauri/src/encryption`
- `src-tauri/src/commands`
- `src-tauri/src/audit`
- `src`
- `e2e`
- `.github/workflows`
- `docs`
- `README.md`
- `SECURITY.md`
- `PLANS.md`

## 7. Règles de suivi

- `[ ]` non démarré
- `[~]` en cours
- `[x]` terminé
- chaque tranche terminée doit mettre à jour ce document, `PLANS.md` si nécessaire, et ses critères de validation;
- aucun item ne passe à `[x]` sans preuve de validation associée;
- les limitations restantes doivent rester écrites noir sur blanc.

## 8. Ordre d'exécution recommandé

1. `TT-01` Benchmark public et preuve marché
2. `TT-02` Workflow bootable de secours
3. `TT-03` Imagerie de supports instables niveau pro
4. `TT-04` Stockages avancés et workflows labo
5. `TT-05` APFS, chiffrement et cas macOS difficiles
6. `TT-06` Durcissement cross-platform low-level
7. `TT-07` Différenciation UX, audit et reporting
8. `TT-08` Polissage produit, docs, release readiness

## 9. Tableau de suivi

| ID | Priorité | Chantier | Statut | Impact | Dépendances |
|---|---|---|---|---|---|
| `TT-01` | `P0` | Benchmark public reproductible | `[~]` | critique | aucune |
| `TT-02` | `P0` | Workflow bootable de secours | `[~]` | critique | `TT-01` recommandé |
| `TT-03` | `P0` | Imagerie support instable niveau pro | `[~]` | critique | aucune |
| `TT-04` | `P1` | RAID / NAS / VM / volumes complexes | `[~]` | très fort | `TT-03` utile |
| `TT-05` | `P1` | APFS, chiffrement, cas macOS avancés | `[~]` | très fort | `TT-03` utile |
| `TT-06` | `P1` | Durcissement low-level multiplateforme | `[ ]` | fort | aucune |
| `TT-07` | `P2` | UX, audit signé, reporting premium | `[~]` | fort | `TT-01` utile |
| `TT-08` | `P2` | Docs, packaging, QA, readiness commerciale | `[~]` | fort | `TT-01` à `TT-07` |

**Synchronisation 2026-04-22**

- `TT-03`, `TT-04`, `TT-05`, `TT-07` et `TT-08` passent de `[ ]` à `[~]` pour refléter les artefacts réellement présents dans le repo;
- ce passage à `[~]` ne veut pas dire "top-tier fermé", seulement "capacité réelle livrée mais encore incomplète";
- la scorecard `benchmarks/scorecard-v1.md` reste la vue courte la plus honnête pour suivre ces chantiers partiels.

**Synchronisation 2026-04-23**

- `TT-02` passe de `[ ]` à `[~]` pour refléter une premiere tranche livree: workflow MVP documente autour d'un live USB Linux + `AppImage`, avec posture read-only et limites explicites;
- `TT-01` reste `[~]` mais le repo porte maintenant une preuve fraiche du blocage `APFS P0` au lieu d'un simple commentaire historique;
- cette mise a jour ne ferme pas les ecarts top-tier: elle rend leur etat plus benchmarkable et plus honnete.

## 10. Détail des chantiers

### `TT-01` Benchmark public reproductible

**Objectif**

Publier un benchmark crédible, reproductible et versionné comparant Récupère à des outils pro sur un corpus contrôlé.

**Pourquoi**

- sans preuve publique, impossible de soutenir une affirmation de niveau top-tier;
- cela force l'honnêteté produit et aide à prioriser les écarts réels;
- c'est le point qui manque le plus face aux leaders.

**Livrables**

- corpus versionné d'images et scénarios;
- protocole de benchmark documenté;
- runner ou scripts de benchmark;
- métriques normalisées;
- rapports comparatifs publiables;
- documentation des cas non supportés.

**Modules impactés**

- `docs`
- scripts de benchmark si ajoutés
- `README.md`
- `.github/workflows`
- potentiellement `src-tauri/src/commands` pour exporter plus d'artefacts comparables

**Critères de validation**

- au moins un corpus initial couvrant delete, format rapide, partition perdue, carving, corruption partielle, support instable;
- résultats reproductibles sur une version taggée du produit;
- sorties de benchmark vérifiables par hash / rapport / log;
- comparaison explicite contre au moins deux comparateurs accessibles sans achat supplementaire, par défaut `PhotoRec` et `TestDisk`; `DMDE` free mode et les suites payantes restent des preuves bonus, pas des prérequis de publication.

**Limites connues**

- ne pas publier de benchmark vague ni marketing;
- ne pas masquer les cas où Récupère est plus faible.

**Sous-tâches**

- [x] définir le protocole exact
- [~] construire le corpus minimal public
- [x] automatiser la validation du manifeste et le template standard de résultats
- [x] enregistrer un premier baseline interne Récupère
- [x] figer la campagne comparative publique `v1`
- [x] exécuter la première campagne comparative accessible
- [x] publier les résultats avec limites

**Statut 2026-04-11**

- bootstrap benchmark lancé;
- workspace `benchmarks/` créé;
- protocole `v1` rédigé;
- manifeste de corpus `v1` validé automatiquement;
- template standard de résultats générable;
- premier résultat `Récupère` baseline interne enregistré le `2026-04-12`;
- check benchmark complet (`manifest + results`) validé localement;
- un scénario `P0` reste encore `not-run` dans ce baseline: `apfs_deleted_orphan_catalog_v1`;
- tentative de validation réelle relancée le `2026-04-22` sur macOS `15.7.4`: le chemin orphan-catalog APFS ne remonte toujours pas de candidat supprimé, donc le scénario reste bloqué tant que le fixture / moteur n'est pas stabilisé;
- premier spot-check concurrent réel ajouté le `2026-04-22`: `PhotoRec 7.2` exécuté en mode scripté officiel sur `signature_carving_jpeg_v1`, run enregistré mais résultat plus faible (`0` fichier récupéré);
- second spot-check concurrent réel ajouté le `2026-04-22`: `DMDE 4.4.6` console exécuté sur le même fixture JPEG minimal, analyse complète terminée mais aucun fichier retrouvé (`Raw:0, FAT:0(0)`);
- première campagne comparative accessible `P0` exécutée le `2026-04-23` avec les mêmes ids de scénarios que le corpus: `PhotoRec 7.2` dispose maintenant d'un fichier de résultats complet couvrant les scénarios `ready-in-repo` déjà promus (`4` scénarios joués, tous terminés avec `0` export), et `TestDisk 7.2` dispose d'un fichier de résultats complet montrant explicitement `unsupported` sur les mêmes scénarios dans cet environnement;
- le report HTML publie désormais des scopes de runs explicites (`internal-baseline`, `public-campaign`, `spot-check`, `targeted-regression`) pour distinguer baseline interne, campagne publique et preuves ciblées de blocage;
- une nouvelle passe APFS du `2026-04-23` confirme encore le blocage via les tests réels ignorés `recover_deleted_files_reads_a_real_apfs_deleted_fixture` et `run_deleted_apfs_scan_marks_results_as_live_catalog_provenance`; le nouveau helper `debug_deleted_catalog_candidates_reports_real_deleted_fixture_summary` réduit le probleme: la fixture expose `3` inodes fichier et `3` ids actifs, mais `0` candidat supprimé, ce qui pointe aujourd'hui vers la generation/capture du catalogue plutot que vers la reconstruction d'extents; note et résultat dédiés archivés dans `benchmarks/results/2026-04-23-recupere-apfs-p0-blocker-note.md` et `benchmarks/results/2026-04-23-recupere-apfs-regression.json`;
- package officiel `R-Studio for Mac 7.5.191751` acquis et inspecté le `2026-04-22`, mais aucun run n'est encore revendiqué: la passe du `2026-04-23` a réduit le blocage à l'étape d'autorisation admin elle-même (`CheckForAdministrativePrivileges(bool)` puis `AuthorizationCreate`), et annuler les prompts `SecurityAgent` ne laisse toujours pas de parcours benchmarkable sur image seule; notes opérateur archivées dans `benchmarks/results/2026-04-22-r-studio-7.5.191751-operator-note.md` et `benchmarks/results/2026-04-23-r-studio-7.5.191751-operator-note.md`;
- runbook de campagne comparative publique `v1` ajouté dans `benchmarks/public-comparative-campaign-v1.md`;
- générateur de template enrichi pour préparer un run concurrent réel sans éditer le JSON à la main;
- report HTML durci pour montrer les statuts `not-run`, `unsupported`, `blocked`, les scenarios `P0` encore hors scope publiable, et le vrai gate de campagne publique (`Récupère` + `PhotoRec` + `TestDisk`);
- prochaine sortie attendue: produire un vrai run `public-campaign` Récupère sur la tranche `P0` prête, stabiliser `APFS P0`, puis élargir la preuve comparative au-delà du premier baseline accessible.

### `TT-02` Workflow bootable de secours

**Objectif**

Permettre l'usage de Récupère quand l'OS source ne démarre pas, avec un environnement de secours bootable cohérent avec la posture read-only.

**Pourquoi**

- c'est un standard fort chez les outils pro;
- c'est critique sur les cas machine non bootable ou disque système fragile;
- cela augmente fortement la crédibilité terrain.

**Livrables**

- image bootable ou guide de création supporté officiellement;
- flow de démarrage minimal et sûr;
- détection de supports, imagerie, export et support bundle disponibles en mode secours;
- documentation utilisateur novice et expert.

**Modules impactés**

- packaging / release
- `src-tauri`
- docs d'installation et d'usage
- CI de build d'artefacts

**Critères de validation**

- démarrage sur machine de test dans un environnement non bootable simulé;
- aucune écriture sur la source;
- export uniquement vers destination sûre;
- journaux et rapports toujours traçables.

**Limites connues**

- ne pas prétendre couvrir tous les drivers ou tous les matériels dès la première tranche;
- commencer par un périmètre simple et fiable.

**Sous-tâches**

- [x] décider la stratégie bootable cible
- [x] définir le périmètre MVP du mode secours
- [~] construire les artefacts
- [ ] tester sur plusieurs cas système non bootable
- [x] documenter clairement les limites matérielles

**Statut 2026-04-23**

- premiere tranche livree sous forme de workflow officiel `live USB Linux + AppImage`, documente dans `docs/bootable-rescue-workflow.md`;
- le manifest de release peut maintenant expliciter la rescue readiness de cette tranche et ses limites;
- le preflight de release verifie la presence de cette documentation pour eviter qu'un packaging futur oublie la posture rescue;
- ce n'est pas encore un media de secours maison ni une validation materielle large: la valeur livree est un cadre rescue credible, borne et compatible avec la mission read-only.

### `TT-03` Imagerie support instable niveau pro

**Objectif**

Faire passer le moteur d'imagerie de "solide" à "référence sérieuse" sur supports dégradés.

**Pourquoi**

- c'est une zone décisive face à `R-Studio`, `UFS Explorer` et `DMDE`;
- une bonne imagerie protège la source et conditionne tout le reste;
- le repo est déjà bien avancé ici, donc le retour sur effort est élevé.

**Livrables**

- profils d'imagerie mieux exposés dans l'UI;
- cartes d'erreurs / unreadable ranges plus exploitables;
- reprise, checkpoints et rescue maps plus visibles;
- reporting plus riche sur les passes et les zones dégradées;
- corpus de tests synthétiques et réels élargi.

**Modules impactés**

- `src-tauri/src/imaging`
- `src-tauri/src/commands`
- `src`
- `e2e`
- documentation technique

**Critères de validation**

- meilleur taux de récupération lisible sur corpus instable;
- logs clairs des stratégies utilisées;
- reprise et import de map fiables;
- UX novice guidée vers "image first" quand le risque est élevé.

**Limites connues**

- ne jamais présenter une lecture partielle comme une récupération garantie;
- ne pas confondre amélioration d'ordonnancement et miracle de récupération.

**Sous-tâches**

- [ ] définir les métriques imaging à battre
- [ ] exposer clairement les profils et artefacts dans l'UI
- [ ] enrichir la validation sur supports dégradés
- [ ] améliorer l'audit et le rapport d'imagerie
- [ ] comparer les résultats avec les outils de référence

**Statut 2026-04-22**

- état réel: `[~]`
- preuves déjà présentes: incidents d'imagerie visibles, export/import de rescue map, échantillons de zones illisibles, support bundle et historique mieux reliés;
- ce qui manque encore: plus de corpus dégradé et une vraie preuve comparative moteur-à-moteur.

### `TT-04` RAID / NAS / VM / volumes complexes

**Objectif**

Passer d'un backend partiellement capable à un produit réellement exploitable sur les cas multi-disques et volumes complexes.

**Pourquoi**

- c'est un gros facteur de crédibilité pro;
- les leaders couvrent beaucoup plus de cas de stockage avancé;
- c'est nécessaire pour sortir du segment "outil sérieux mais encore limité".

**Livrables**

- exposition UI propre des workflows RAID / NAS / VM;
- amélioration de la reconstruction et du diagnostic;
- meilleure couverture Storage Spaces / LVM / NAS / images virtuelles prioritaires;
- bornes claires entre cas supportés, partiels et labo.

**Modules impactés**

- `src-tauri/src/raid`
- `src-tauri/src/virtual_disk`
- `src-tauri/src/core`
- `src-tauri/src/commands`
- `src`

**Critères de validation**

- flux bout-en-bout sur plusieurs fixtures RAID / VM / NAS;
- messages d'erreur intelligibles quand les paramètres manquent;
- mode expert suffisamment riche sans casser le mode novice.

**Limites connues**

- ne pas ouvrir un trop grand nombre de formats à moitié supportés;
- mieux vaut moins de workflows, mais fiables et bien bornés.

**Sous-tâches**

- [ ] prioriser les technologies cibles
- [ ] combler les trous RAID critiques
- [ ] exposer les cas VM / NAS dans l'UI
- [ ] ajouter des fixtures de validation
- [ ] écrire une matrice supporté / partiel / non supporté

**Statut 2026-04-22**

- état réel: `[~]`
- preuves déjà présentes: workflow d'image d'analyse RAID, provenance des sources importées, posture opérateur plus explicite;
- ce qui manque encore: matrice VM / NAS / virtual disk plus profonde et validations benchmark-grade plus larges.

### `TT-05` APFS, chiffrement, cas macOS avancés

**Objectif**

Réduire les écarts les plus visibles sur l'écosystème Apple et les volumes chiffrés.

**Pourquoi**

- `APFS` reste un juge de paix sur les outils modernes;
- les cas FileVault, snapshots et métadonnées Apple sont difficiles mais structurants;
- sans cela, impossible d'être crédible sur certains cas premium.

**Livrables**

- amélioration de la récupération supprimée APFS;
- meilleure gestion des volumes chiffrés et de leurs limites;
- meilleure documentation des cas supportés et non supportés;
- fixtures de régression dédiées.

**Modules impactés**

- `src-tauri/src/analyzers/apfs`
- `src-tauri/src/encryption`
- `src-tauri/src/commands`
- `src`
- docs

**Critères de validation**

- plus de cas APFS récupérés proprement sur corpus dédié;
- messages exacts sur les cas nécessitant déverrouillage ou impossibles;
- aucun faux sentiment de support complet si ce n'est pas le cas.

**Limites connues**

- les cas snapshots, chiffrement et structures Apple rares doivent être livrés par tranches;
- ne pas annoncer de support "APFS complet" trop tôt.

**Sous-tâches**

- [ ] dresser la matrice des cas APFS prioritaires
- [ ] renforcer les fixtures et les tests de régression
- [ ] améliorer la lisibilité des limitations dans l'UI
- [ ] étendre le support chiffrement réellement exploitable
- [ ] re-benchmarker sur corpus macOS

**Statut 2026-04-22**

- état réel: `[~]`
- preuves déjà présentes: surfacing APFS plus honnête, triage conservateur des candidats APFS supprimés, garde-fous chiffrement côté diagnostic et scan;
- ce qui manque encore: snapshots / clones APFS, cas pré-déverrouillage réellement profonds, et parité macOS difficile.

### `TT-06` Durcissement low-level multiplateforme

**Objectif**

Fermer les angles morts d'accès brut aux supports et homogénéiser la fiabilité low-level sur macOS, Windows et Linux.

**Pourquoi**

- les outils top-tier sont jugés aussi sur les cas "ça marche vraiment sur la machine du client";
- les écarts low-level créent des bugs dangereux ou des workflows cassés;
- certaines zones sont encore explicitement incomplètes.

**Livrables**

- durcissement Windows raw-device et mapping fiable volume -> disque;
- revue des chemins d'accès source / destination;
- meilleure détection des permissions et prérequis système;
- tests cross-platform ciblés.

**Modules impactés**

- `src-tauri/src/core`
- `src-tauri/src/commands`
- CI
- docs d'installation / dépannage

**Critères de validation**

- comportements cohérents sur macOS, Windows et Linux;
- messages d'échec actionnables;
- aucun contournement possible des garde-fous source/destination.

**Limites connues**

- certaines APIs OS imposent des branches spécifiques;
- la parité totale ne viendra pas en une tranche.

**Sous-tâches**

- [ ] fermer les TODOs low-level prioritaires
- [ ] ajouter des tests de résolution de devices
- [ ] renforcer la télémétrie locale des erreurs système
- [ ] aligner la doc d'installation et de permissions

### `TT-07` UX, audit signé, reporting premium

**Objectif**

Transformer les forces actuelles de Récupère en différenciation visible et difficile à copier.

**Pourquoi**

- la sûreté et l'honnêteté UX sont déjà des points forts;
- c'est l'endroit où Récupère peut devenir meilleur que certains concurrents;
- cela sert autant le particulier stressé que l'expert prudent.

**Livrables**

- rapports plus premium et potentiellement signés;
- mode novice extrêmement clair sur ce qui est possible / incertain / impossible;
- mode expert plus riche en détails techniques, sans danger UX;
- meilleure visualisation des risques, de l'imagerie et des limites.

**Modules impactés**

- `src`
- `src-tauri/src/audit`
- `src-tauri/src/commands`
- preview / export
- i18n

**Critères de validation**

- chaque écran critique a ses états `empty`, `loading`, `scanning`, `partial-success`, `success`, `warning`, `error`;
- la différence novice / expert est utile, pas cosmétique;
- les rapports peuvent être utilisés comme artefacts sérieux de support ou de preuve.

**Limites connues**

- ne pas transformer l'UI en dashboard générique;
- ne pas cacher l'incertitude derrière des scores trop lisses.

**Sous-tâches**

- [ ] définir le format de rapport premium cible
- [ ] revoir les écrans critiques sous stress utilisateur
- [ ] enrichir le mode expert
- [ ] harmoniser les messages de limitation et de risque
- [ ] sécuriser la cohérence i18n EN / FR

**Statut 2026-04-22**

- état réel: `[~]`
- preuves déjà présentes: `History`, handoff support, support bundles et séparation novice / expert sont déjà nettement plus sérieux qu'au départ;
- ce qui manque encore: artefacts d'audit premium / signés et fermeture plus stricte des parcours sous stress.

### `TT-08` Docs, packaging, QA, readiness commerciale

**Objectif**

Passer d'un produit techniquement impressionnant à un produit réellement prêt à être jugé durement par le marché.

**Pourquoi**

- les leaders sont aussi forts parce qu'ils sont cohérents, testés, documentés, packagés;
- les petits défauts périphériques dégradent fortement la confiance.

**Livrables**

- docs alignées avec la réalité du code;
- suppression des incohérences doc / code / tests;
- packaging plus propre;
- matrice QA plus explicite;
- correction des régressions UI et E2E restantes.

**Modules impactés**

- tout le repo selon les écarts
- docs
- `.github/workflows`
- packaging Tauri
- tests `e2e`

**Critères de validation**

- docs produit et architecture à jour;
- parcours critiques verts en CI;
- plus de décalage entre fonctionnalités annoncées et réellement présentes;
- installation et release plus prévisibles.

**Limites connues**

- ce chantier n'est pas cosmétique: il conditionne la crédibilité finale;
- il ne doit pas être repoussé à la toute fin.

**Sous-tâches**

- [ ] corriger les régressions E2E restantes
- [ ] nettoyer les incohérences documentaires
- [ ] revoir packaging, CSP, assets offline et détails release
- [ ] formaliser une matrice QA par plateforme
- [ ] préparer une checklist de release top-tier

**Statut 2026-04-22**

- état réel: `[~]`
- preuves déjà présentes: `cargo check`, `npm run test:ui` et `npm run benchmark:check` sont des validations repo utiles, et la posture documentaire sur les sources importées est plus honnête;
- ce qui manque encore: rescue bootable, davantage de preuve release cross-platform, et readiness commerciale cohérente.

## 11. Garde-fous à préserver pendant toute l'exécution

- ne jamais écrire sur le disque source;
- ne jamais restaurer sur le disque source;
- ne jamais présenter l'IA comme capable de recréer des données physiquement détruites;
- toujours journaliser les décisions importantes;
- toujours distinguer clairement estimation, récupération partielle et récupération vérifiée;
- toujours documenter les limites restantes.

## 12. Première tranche recommandée

La première tranche à ouvrir est `TT-01 Benchmark public reproductible`.

**Pourquoi commencer ici**

- c'est le meilleur révélateur des vrais écarts;
- cela évite de développer à l'aveugle;
- cela donnera un scorecard objectif pour prioriser `TT-03`, `TT-04` et `TT-05`.

**Sortie attendue de la première tranche**

- protocole écrit;
- corpus minimal initial;
- scripts de collecte;
- première campagne de benchmark interne;
- mise à jour de ce document avec résultats et nouveaux écarts.
