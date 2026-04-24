// ============================================================================
// Récupère — Remote agent client
// ============================================================================
// Lets the desktop app pilot a `recupere-agent` instance running on a remote
// server over HTTP. The agent runs the heavy scan/restore work locally on the
// server; only metadata, progress and (optionally) restore commands transit
// the network.
//
// Submodules:
//   - `client`   : reqwest wrapper around the agent HTTP API
//   - `registry` : in-memory store of registered agents (host + token)
//   - `commands` : Tauri command handlers exposed to the frontend
// ============================================================================

pub mod client;
pub mod commands;
pub mod registry;
