//! Golden-fixture contract tests.
//!
//! The fixtures in `tests/fixtures/golden/` are generated from the Python
//! implementation by `scripts/export_golden_fixtures.py`. Every phase of the
//! Rust port must reproduce them; this crate holds the comparison helpers.

use serde_json::Value;

/// Path of the golden fixture directory relative to the workspace root.
pub const FIXTURE_DIR: &str = "crates/strix-contracts/tests/fixtures/golden";

/// Recursively sort object keys so two JSON documents can be compared
/// independently of key order.
#[must_use]
pub fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                if let Some(child) = map.get(key) {
                    sorted.insert(key.clone(), canonicalize(child));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}
