//! Backend side of the TUI protocol (version 3) spoken by the existing Go
//! sidecar.
//!
//! Phase 0 ships the crate skeleton; phase 10 replaces the Python backend of
//! `strix.interface.tui.backend` while leaving the Go sidecar untouched.

/// Protocol version understood by this backend.
pub const PROTOCOL_VERSION: u32 = 3;
