# AGENTS.md

## Mission du repo
Construire une application desktop de récupération de données assistée par IA, multiplateforme, avec un moteur low-level en lecture seule (Rust), une UI React/TypeScript orientée desktop, et une architecture modulaire industrielle.

## Règles absolues
- Ne jamais écrire sur le disque source.
- Ne jamais restaurer de fichiers sur le disque source.
- Toujours traiter la récupération de données comme un domaine à fortes contraintes de sécurité et de fiabilité.
- Ne jamais présenter l'IA comme capable de recréer des données physiquement détruites.
- Toute action risquée doit être explicitement signalée à l'utilisateur.
- Toute décision importante doit être traçable dans les logs.

## Priorités d'implémentation
1. sûreté,
2. exactitude,
3. architecture modulaire,
4. clarté de l'interface,
5. performance,
6. sophistication IA.

## Comportement de l'agent
Ces règles biaisent vers la prudence plutôt que la vitesse. Pour les tâches triviales, utiliser le jugement.

### 1. Réfléchir avant de coder
- Expliciter les hypothèses. En cas d'incertitude, demander.
- Si plusieurs interprétations sont possibles, les présenter — ne pas choisir en silence.
- Si une approche plus simple existe, le dire. Pousser un contre-argument quand c'est justifié.
- Si quelque chose n'est pas clair, s'arrêter, nommer ce qui pose problème, demander.

### 2. Simplicité d'abord
- Code minimum qui résout le problème. Rien de spéculatif.
- Pas de features au-delà de ce qui a été demandé.
- Pas d'abstractions pour du code à usage unique.
- Pas de "flexibilité" ou "configurabilité" non demandée.
- Pas de gestion d'erreur pour des scénarios impossibles.
- Si 200 lignes pourraient en faire 50, réécrire.
- Test : un·e ingénieur·e senior dirait-il que c'est sur-compliqué ? Si oui, simplifier.

### 3. Changements chirurgicaux
- Ne toucher qu'à ce qui est nécessaire. Ne nettoyer que son propre désordre.
- Ne pas "améliorer" code, commentaires ou formatage adjacents.
- Ne pas refactorer ce qui n'est pas cassé.
- Respecter le style existant, même si on l'écrirait autrement.
- Si du code mort non lié est repéré, le signaler — ne pas le supprimer.
- Supprimer uniquement les imports, variables et fonctions rendus inutiles par les changements apportés.
- Ne pas supprimer le code mort préexistant sans demande explicite.
- Test : chaque ligne modifiée doit se relier directement à la demande utilisateur.

### 4. Exécution orientée objectif
- Transformer les tâches en objectifs vérifiables avant d'agir.
  - "Ajouter de la validation" → "Écrire des tests pour les entrées invalides, puis les faire passer".
  - "Corriger le bug" → "Écrire un test qui le reproduit, puis le faire passer".
  - "Refactorer X" → "Tests verts avant et après".
- Pour une tâche multi-étapes, énoncer un plan bref :
  1. [Étape] → vérification : [check]
  2. [Étape] → vérification : [check]
- Des critères de succès forts permettent de boucler seul. Des critères faibles ("fais en sorte que ça marche") imposent des allers-retours.

## Planification
- Pour toute fonctionnalité complexe, rédiger ou mettre à jour un plan dans `PLANS.md` avant implémentation.
- Ne pas lancer de gros refactor sans plan validé.
- Si une tâche touche plusieurs modules, documenter les contrats entre modules avant codage.

## Architecture attendue
Le projet doit rester séparé en couches :
- core low-level (hw-detector, io-reader)
- imaging (disk-imager)
- filesystem analyzers (fat32, exfat, ntfs)
- carving (engine, signatures)
- scoring
- ai services
- preview/export
- desktop app (React/TypeScript)
- shared types/contracts (Rust + TypeScript)

## Frontend
- Priorité au desktop.
- Utiliser des composants réutilisables, testables et cohérents.
- Ne jamais improviser une UI générique.
- Chaque écran doit définir ses états : empty, loading, scanning, partial-success, success, warning, error.
- Les composants UI doivent s'appuyer sur des tokens de design explicites.
- Toujours distinguer novice mode et expert mode.
- Light mode par défaut, dark mode comme variante.
- i18n obligatoire : anglais par défaut, français supporté.

## UX
- Réduire le stress utilisateur.
- Expliquer clairement ce qui est possible, incertain ou impossible.
- Éviter le jargon en mode guidé.
- Utiliser le jargon complet en mode expert.
- Les pourcentages de récupérabilité doivent être présentés comme des estimations.

## Code quality
- TypeScript strict côté UI.
- Contrats de types partagés.
- Tests unitaires sur la logique critique.
- Tests d'intégration sur les pipelines.
- Validation automatique avant merge si disponible.
- Petites PR logiques plutôt que gros blocs opaques.

## Quand utiliser PLANS.md
Utiliser `PLANS.md` pour :
- nouvelles fonctionnalités majeures,
- changements de stack,
- architecture multi-modules,
- workflows recovery/scan,
- design système UI,
- sécurité,
- intégration IA.

## Sortie attendue d'un agent
Avant une grosse implémentation, fournir :
- hypothèses,
- risques,
- modules impactés,
- plan d'exécution,
- critères de validation,
- limites connues.

## Ce qu'il faut éviter
- faux "magic recovery"
- architecture monolithique
- UI dashboard générique
- couplage fort entre UI et core
- logique critique cachée dans des composants frontend
- cloud imposé pour les opérations sensibles
