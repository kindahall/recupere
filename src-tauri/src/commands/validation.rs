// ============================================================================
// Récupère — command-layer validation helpers
// ============================================================================
//
// Extracted from `commands/mod.rs` (plan chantier I5, first safe slice).
// These are pure, allocation-free string → &'static str normalisers that
// guard the IPC boundary: every scan / export / conflict-strategy string
// coming from the UI passes through one of these before it's trusted.
//
// Rules of the extract:
//   - no behaviour change (identity error strings + identity `Ok`
//     branches) — the refactor is purely mechanical movement.
//   - `pub(crate)` so the same call sites (`super::normalize_*`) keep
//     compiling — see the `pub use` re-exports in `commands/mod.rs`.
// ============================================================================

pub(crate) fn normalize_scan_type(scan_type: &str) -> Result<&'static str, String> {
    match scan_type {
        "quick" => Ok("quick"),
        "deep" => Ok("deep"),
        "image" => Ok("image"),
        "carving" => Ok("carving"),
        "signature-carving" => Ok("signature-carving"),
        "reconstruction" => Ok("reconstruction"),
        other => Err(format!(
            "Unsupported scan type `{other}`. Only `quick`, `deep`, `image`, `carving`, `signature-carving`, and `reconstruction` are available."
        )),
    }
}

pub(crate) fn normalize_conflict_strategy(conflict_strategy: &str) -> Result<&'static str, String> {
    match conflict_strategy {
        "rename" => Ok("rename"),
        "skip" => Ok("skip"),
        "overwrite" => Ok("overwrite"),
        other => Err(format!(
            "Unsupported conflict strategy `{other}`. Expected `rename`, `skip`, or `overwrite`."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_scan_type_accepts_every_documented_variant() {
        for kind in [
            "quick",
            "deep",
            "image",
            "carving",
            "signature-carving",
            "reconstruction",
        ] {
            assert_eq!(normalize_scan_type(kind).unwrap(), kind);
        }
    }

    #[test]
    fn normalize_scan_type_rejects_unknown_values() {
        let err = normalize_scan_type("magic-restore").expect_err("unknown value must be rejected");
        assert!(
            err.contains("magic-restore"),
            "error should mention the bad value: {err}"
        );
    }

    #[test]
    fn normalize_conflict_strategy_accepts_documented_variants() {
        assert_eq!(normalize_conflict_strategy("rename").unwrap(), "rename");
        assert_eq!(normalize_conflict_strategy("skip").unwrap(), "skip");
        assert_eq!(
            normalize_conflict_strategy("overwrite").unwrap(),
            "overwrite"
        );
    }

    #[test]
    fn normalize_conflict_strategy_rejects_unknown_values() {
        assert_eq!(normalize_conflict_strategy("rename").unwrap(), "rename");
        assert!(normalize_conflict_strategy("keep-both").is_err());
    }
}
