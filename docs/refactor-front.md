# Découpage progressif du front

## Pourquoi
Trois fichiers TypeScript dépassent largement le seuil de maintenabilité :

| Fichier | LoC | Problème |
|---|---|---|
| [src/hooks/useIpc.ts](../src/hooks/useIpc.ts) | ~1 150 | Mélange tous les wrappers IPC, par domaine. |
| [src/pages/ResultsPage.tsx](../src/pages/ResultsPage.tsx) | ~1 220 | Coordinateur géant : filtres, recherche, sélection, chargement résultats, IA, export. |
| [src/pages/ExpertPage.tsx](../src/pages/ExpertPage.tsx) | ~1 087 | Hex viewer, timeline, preview, contrôles avancés mélangés. |

L'audit P1 demande leur fragmentation. Comme pour `commands/mod.rs`, l'approche est **progressive** : chaque sous-passe doit laisser `npx tsc --noEmit` vert et l'app fonctionnelle. Aucun changement de logique métier — uniquement extraction et imports.

## A. Fragmentation de `useIpc.ts`

### Cible
```
src/hooks/ipc/
├── client.ts        # invoke<T>() + helpers de mapping snake_case → camelCase
├── runtime.ts       # fetchAppBuildInfo, fetchRuntimeCapabilities
├── device.ts        # fetchDevices, fetchSmartReport
├── diagnostic.ts    # fetchDiagnostic, fetchAiAdvisory
├── scan.ts          # startScan, startPotentialVolumeScan, pause/resume/cancel,
│                    # fetchScanProgress, fetchScanHistory, fetchScanLogs, fetchResults
├── imaging.ts       # startImaging
├── preview.ts       # fetchFilePreview, fetchFileHexPreview, *Auxiliary*
├── export.ts        # validateExportDestination, startExport, fetchExportProgress,
│                    # fetchExportLogs, fetchExportHistory, clearLocalHistory,
│                    # saveTechnicalTimelineReport, saveSupportBundle,
│                    # generateRecoveryReport, exportResultsCsv, generateLabBundle
├── gemma.ts         # fetchGemmaSettings, saveGemmaSettings, fetchGemmaStatus,
│                    # startGemmaPull, fetchGemmaPullProgress
├── ai.ts            # fetchScanAiBrief, classifyScanFiles, predictScanRecovery,
│                    # generateNarrativeReport, suggestFileReconstruction,
│                    # smartSelectByCategory, buildCloudAiPrompt, runGemmaAnalysis,
│                    # aiAutopilotScan, chatWithAi, searchFileByName
├── license.ts       # activateLicense, fetchLicenseStatus, deactivateLicense, etc.
├── audit.ts         # fetchAuditTrail
└── index.ts         # re-exports tout
```

`src/hooks/useIpc.ts` devient un simple `export * from './ipc';` pour préserver les imports existants. Aucun composant front à modifier.

### Process
1. Créer `src/hooks/ipc/client.ts` avec uniquement les helpers `invoke<T>()` et types partagés (`DiagnosticData`, `ScanProgressData`, `ExportValidationData`, etc.).
2. Créer `src/hooks/ipc/index.ts` qui re-exporte chaque module.
3. Pour chaque domaine ci-dessus :
   - Couper les fonctions du fichier monolithique
   - Les coller dans le fichier dédié avec leurs imports nécessaires
   - `npx tsc --noEmit` après chaque coupe
4. Une fois tous les domaines extraits, transformer `src/hooks/useIpc.ts` en :
   ```ts
   // Backwards-compatible re-export. Existing imports
   // `from '../hooks/useIpc'` continue to work.
   export * from './ipc';
   ```

### Anti-règles
- ❌ Ne pas changer les noms exportés.
- ❌ Ne pas typer plus strictement les `any` pendant cette passe (faire ça dans une PR séparée pour faciliter la review).

---

## B. Fragmentation de `ResultsPage.tsx`

### Approche
Extraire **les sections JSX** dans `src/components/results/`, **pas** les hooks d'état (qui restent dans la page) — cela évite la prop-drilling massive et garde un seul point de coordination.

### Cible
```
src/pages/ResultsPage.tsx                # ~250 LoC : data fetching + state + composition
src/components/results/
├── ResultsToolbar.tsx                   # filtres, recherche, sélection en masse
├── ResultsLayout.tsx                    # 3-pane layout (FileTree | Preview | AI)
├── ResultsExportBar.tsx                 # boutons d'export, status
├── ResultsAiSection.tsx                 # tab AI Analysis ↔ AI Chat (déjà séparés)
├── FileTreePanel.tsx                    # déjà existant
├── FilePreviewPanel.tsx                 # déjà existant
├── AiAnalysisPanel.tsx                  # déjà existant
└── AiChatPanel.tsx                      # déjà existant
```

### Process
1. Identifier dans `ResultsPage.tsx` les blocs JSX `>= 50 lignes` et leur extraire un composant.
2. Pour chaque composant extrait :
   - Lister les variables d'état utilisées
   - Les passer en props (read) ou en callbacks (write)
   - Pas de `useState` à l'intérieur sauf si l'état est purement local au composant
3. `npx tsc --noEmit` après chaque extraction.

### Vérification visuelle
**Obligatoire** après chaque extraction : ouvrir l'app sur un scan terminé, vérifier que :
- L'arborescence de fichiers s'affiche
- Le preview se charge sur clic
- Les boutons d'export sont actifs
- Le panneau AI fonctionne (si Gemma est ready)

---

## C. Fragmentation de `ExpertPage.tsx`

Même approche que B, cible :
```
src/pages/ExpertPage.tsx                 # ~250 LoC
src/components/expert/
├── ExpertHexView.tsx                    # hex viewer (déjà séparé en partie)
├── ExpertTimelineView.tsx               # technical timeline panel
├── ExpertPreviewView.tsx                # auxiliary preview
├── ExpertControlBar.tsx                 # boutons d'export du timeline, support bundle
└── ExpertSidebar.tsx                    # liste des fichiers + filtres expert
```

### Vérification
Smoke test sur un scan terminé en mode Expert.

---

## État actuel

- [ ] Sous-passe A1 — créer `src/hooks/ipc/{client.ts,index.ts}`
- [ ] Sous-passe A2 — extraire `runtime.ts`, `device.ts`, `license.ts`, `audit.ts` (faciles)
- [ ] Sous-passe A3 — extraire `diagnostic.ts`, `gemma.ts` (moyens)
- [ ] Sous-passe A4 — extraire `scan.ts`, `imaging.ts`, `preview.ts`, `export.ts`, `ai.ts` (gros)
- [ ] Sous-passe A5 — convertir `useIpc.ts` en re-export
- [ ] Sous-passe B1 — extraire `ResultsToolbar.tsx`
- [ ] Sous-passe B2 — extraire `ResultsLayout.tsx`
- [ ] Sous-passe B3 — extraire `ResultsExportBar.tsx`
- [ ] Sous-passe C1 — extraire `ExpertSidebar.tsx`
- [ ] Sous-passe C2 — extraire `ExpertControlBar.tsx`
