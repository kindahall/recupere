# Prompt Claude Code — Application de récupération de fichiers (extension intelligente)

## Contexte

Une application de récupération de fichiers existe déjà.
Elle permet de scanner un disque et de tenter de récupérer des fichiers supprimés.

Objectif : ajouter un système intelligent de surveillance, d'historique et d'assistance à la récupération basé sur la mémoire des emplacements des fichiers.

---

## Objectif global

Créer un système qui :

- Observe l'état du système de fichiers dans le temps
- Mémorise les emplacements des fichiers et dossiers
- Détecte les changements (suppression, déplacement, renommage)
- Permet à l'utilisateur de comprendre ce qui a disparu
- Aide à la récupération avec contexte (emplacement, date, probabilité)

---

## Fonctionnalités à implémenter

### 1. Indexation des fichiers

Créer un module qui :

- Scanne les dossiers sélectionnés ou les disques
- Enregistre pour chaque fichier :
  - nom
  - chemin complet
  - taille
  - extension
  - date de création
  - date de modification
  - identifiant unique (si possible)
  - hash partiel ou complet
  - disque ou volume

- Stocke ces données dans une base locale

---

### 2. Historique des états

Créer un système de snapshots :

- Sauvegarder un état du disque à un instant T
- Conserver plusieurs états (ex : aujourd’hui, hier, 7 jours)
- Permettre la comparaison entre états

---

### 3. Détection des changements

Créer un moteur de comparaison entre deux snapshots :

- Identifier :
  - fichiers disparus
  - fichiers nouveaux
  - fichiers déplacés
  - fichiers renommés

Logique attendue :

- Même hash + chemin différent → déplacé
- Même contenu + nom différent → renommé
- Absent → supprimé (à confirmer)

---

### 4. Surveillance automatique

Créer un mode passif :

- Scan automatique quotidien (configurable)
- Option de surveillance en temps réel (si possible)
- Détection des changements sans intervention utilisateur

---

### 5. Gestion des fichiers supprimés

Créer une vue dédiée :

- Liste des fichiers disparus
- Afficher :
  - dernier chemin connu
  - date de dernière présence
  - date de disparition estimée
  - type de fichier
  - taille

---

### 6. Score de récupérabilité

Créer un système de score basé sur :

- ancienneté de suppression
- activité disque estimée
- type de stockage

Retourner un score :
- élevé
- moyen
- faible

---

### 7. Intégration avec moteur de récupération existant

- Utiliser l’emplacement connu pour orienter la récupération
- Permettre :
  - restauration à l’emplacement d’origine
  - restauration vers un autre emplacement

---

### 8. Gestion des erreurs

Le système doit gérer :

- disque non monté
- disque externe déconnecté
- permissions insuffisantes
- dossiers temporairement inaccessibles

Éviter les faux positifs de suppression

---

### 9. Optimisation

- éviter les scans complets inutiles
- utiliser un système incrémental
- limiter la consommation CPU et disque
- prioriser certains dossiers

---

### 10. Journalisation

Créer un système de logs :

- scans effectués
- changements détectés
- erreurs rencontrées
- tentatives de récupération

---

## Contraintes

- Ne pas réécrire le moteur de récupération existant
- S’appuyer dessus
- Code modulaire
- Base de données locale légère
- Fonctionnement offline

---

## Résultat attendu

Un système capable de :

- dire où se trouvait un fichier supprimé
- indiquer quand il a disparu
- distinguer suppression / déplacement / renommage
- assister efficacement la récupération

---

## Priorités

1. Indexation
2. Détection des changements
3. Historique
4. Interface des fichiers supprimés
5. Score de récupération
6. Optimisation

---

## Important

Ne pas coder directement une interface graphique complexe.
Se concentrer sur la logique, les modules et les structures de données.
