//! Agent loop, budget hooks, session persistence and context compaction.
//!
//! Phase 0 ships the crate skeleton; see `CONTRACT.md` for the behaviour the
//! Rust engine must reproduce from the Python `strix.core` and `strix.llm`
//! modules.

/// Placeholder marker so the crate is not empty.
pub const MAX_TURNS_DEFAULT: u32 = 500;
