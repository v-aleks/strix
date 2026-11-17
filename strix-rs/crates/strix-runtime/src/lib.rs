//! Docker sandbox lifecycle, workspace materialization and Caido control.
//!
//! Phase 0 ships the crate skeleton; phase 4 ports the sandbox behaviour from
//! `strix.runtime`.

/// Placeholder marker so the crate is not empty.
pub const DEFAULT_SANDBOX_IMAGE: &str = "ghcr.io/usestrix/strix-sandbox:1.4.0";
