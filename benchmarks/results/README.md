# Benchmark results

Store benchmark result files here.

Recommended naming:

- `YYYY-MM-DD-recupere-<build>.json`
- `YYYY-MM-DD-dmde-<version>.json`
- `YYYY-MM-DD-photorec-<version>.json`
- `YYYY-MM-DD-testdisk-<version>.json`
- `YYYY-MM-DD-r-studio-<version>.json`
- `YYYY-MM-DD-stellar-<version>.json`
- `YYYY-MM-DD-disk-drill-<version>.json`

Each result file should:

- reference the corpus manifest id;
- use the exact scenario ids from the manifest;
- record the host OS and operator;
- declare a `runScope` when the run is an internal baseline, a public campaign,
  a spot-check, or a targeted regression proof;
- use `campaignId` for scoped public slices such as
  `public-comparative-campaign-v1`;
- state whether the run was completed, blocked, unsupported, or invalid;
- keep unsupported and weaker outcomes visible.

Generate a fresh template with:

```bash
npm run benchmark:template
```

Generate an operator-ready template for a real comparative run with:

```bash
npm run benchmark:template -- \
  --out benchmarks/results/2026-04-23-photorec-7.2.json \
  --tool-name "PhotoRec" \
  --tool-version "7.2" \
  --build-ref "public-campaign-v1" \
  --host-os "macOS 15.7.4" \
  --host-arch "arm64" \
  --operator "Initials" \
  --notes "Manual comparative run following benchmarks/public-comparative-campaign-v1.md"
```
