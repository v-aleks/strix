//! Contract tests over the golden fixtures exported from the Python engine.
//!
//! These tests do not yet compare Rust output against the fixtures — the Rust
//! report writer lands in phase 6. What they do now is pin the fixture set
//! itself: the shapes the Rust port has to reproduce. When the fixtures change,
//! these tests fail loudly, so a contract drift cannot slip into a later phase
//! unnoticed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("cannot read {}: {err}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|err| {
        panic!("{} is not valid JSON: {err}", path.display());
    })
}

fn manifest() -> Value {
    read_json(&fixture_root().join("manifest.json"))
}

fn scenario_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixture_root())
        .unwrap_or_else(|err| panic!("cannot list fixtures: {err}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn manifest_lists_every_scenario_on_disk() {
    let manifest = manifest();
    let scenarios = manifest["scenarios"].as_array().expect("scenarios array");
    let named: Vec<&str> = scenarios
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert!(
        named.contains(&"tool-schemas"),
        "the tool-schema fixture must be listed in the manifest"
    );

    let on_disk = scenario_dirs()
        .iter()
        .filter_map(|path| path.file_name())
        .filter_map(|name| name.to_str())
        .map(|name| name.trim_start_matches("scenario-").to_owned())
        .collect::<Vec<String>>();
    let expected = named
        .iter()
        .filter(|name| **name != "tool-schemas")
        .map(|name| (*name).to_owned())
        .collect::<Vec<String>>();
    assert_eq!(
        on_disk.len(),
        expected.len(),
        "every scenario in the manifest must exist on disk"
    );
    for name in expected {
        assert!(
            on_disk.contains(&name),
            "scenario {name} is listed in the manifest but missing on disk"
        );
    }
}

#[test]
fn at_least_twenty_scenarios_are_pinned() {
    let manifest = manifest();
    let scenarios = manifest["scenarios"].as_array().expect("scenarios array");
    assert!(
        scenarios.len() >= 20,
        "phase 0 requires at least 20 golden scenarios, found {}",
        scenarios.len()
    );
}

#[test]
fn every_scenario_writes_the_core_artifacts() {
    for dir in scenario_dirs() {
        // SARIF and coverage are always written, even for a clean scan: a
        // stale findings.sarif must not survive a run that found nothing.
        for name in ["run.json", "findings.sarif", "coverage.json"] {
            let path = dir.join(name);
            assert!(path.is_file(), "{} must exist in {}", name, dir.display());
            read_json(&path);
        }

        // The finding index exists only when the run filed or deleted a
        // finding — the Python engine writes it lazily.
        let findings_path = dir.join("vulnerabilities.json");
        if findings_path.is_file() {
            read_json(&findings_path);
        }
    }
}

#[test]
fn run_record_carries_the_expected_keys() {
    for dir in scenario_dirs() {
        let run = read_json(&dir.join("run.json"));
        for key in [
            "run_id",
            "run_name",
            "start_time",
            "end_time",
            "status",
            "auth_mode",
            "targets_info",
            "llm_usage",
        ] {
            assert!(
                run.get(key).is_some(),
                "run.json of {} must contain {key}",
                dir.display()
            );
        }
        assert_eq!(run["status"], "completed");
    }
}

#[test]
fn sarif_documents_use_version_2_1_0() {
    for dir in scenario_dirs() {
        let sarif = read_json(&dir.join("findings.sarif"));
        assert_eq!(sarif["version"], "2.1.0");
        assert_eq!(
            sarif["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        let runs = sarif["runs"].as_array().expect("sarif runs array");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["tool"]["driver"]["name"], "Strix");
    }
}

#[test]
fn finding_ids_are_sequential_and_never_reused() {
    for dir in scenario_dirs() {
        let findings_path = dir.join("vulnerabilities.json");
        if !findings_path.is_file() {
            continue;
        }
        let findings = read_json(&findings_path);
        let list = findings.as_array().unwrap_or_else(|| {
            panic!("vulnerabilities.json of {} must be an array", dir.display())
        });
        for (index, finding) in list.iter().enumerate() {
            let id = finding["id"].as_str().expect("finding id");
            assert_eq!(
                id,
                format!("vuln-{:04}", index + 1),
                "finding ids must be sequential in {}",
                dir.display()
            );
        }
    }
}

#[test]
fn severity_values_are_from_the_known_set() {
    let known = ["critical", "high", "medium", "low", "info"];
    for dir in scenario_dirs() {
        let findings_path = dir.join("vulnerabilities.json");
        if !findings_path.is_file() {
            continue;
        }
        let findings = read_json(&findings_path);
        for finding in findings.as_array().expect("findings array") {
            let severity = finding["severity"].as_str().expect("severity");
            assert!(
                known.contains(&severity),
                "unknown severity {severity} in {}",
                dir.display()
            );
        }
    }
}

#[test]
fn coverage_document_is_schema_version_one() {
    for dir in scenario_dirs() {
        let coverage = read_json(&dir.join("coverage.json"));
        assert_eq!(coverage["schema_version"], 1);
        assert!(coverage["entries"].is_array());
        assert!(coverage["gaps"].is_array());
        assert!(coverage["summary"].is_object());
    }
}

#[test]
fn csv_index_escapes_formula_prefixes() {
    let path = fixture_root().join("scenario-title-with-formula-prefix/vulnerabilities.csv");
    let text = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("cannot read {}: {err}", path.display());
    });
    assert!(
        !text.lines().skip(1).any(|line| line.starts_with('=')),
        "a title starting with = must be prefixed with a quote in the CSV index"
    );
    assert!(
        text.contains("'=cmd"),
        "the formula-looking title must be escaped, not stripped"
    );
}

#[test]
fn tool_schema_fixture_pins_the_whole_tool_surface() {
    let payload = read_json(&fixture_root().join("tool-schemas.json"));
    assert_eq!(payload["count"], 43, "the base tool set has 43 tools");
    let tools = payload["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names.len(), 43);
    for expected in [
        "think",
        "load_skill",
        "create_todo",
        "record_coverage",
        "create_vulnerability_report",
        "create_agent",
        "send_message_to_agent",
    ] {
        assert!(names.contains(&expected), "tool {expected} must be pinned");
    }
    for tool in tools {
        let parameters = tool["parameters"]
            .as_object()
            .unwrap_or_else(|| panic!("tool {} must have parameters", tool["name"]));
        assert!(
            parameters.contains_key("properties"),
            "tool {} must expose a properties schema",
            tool["name"]
        );
    }
}

#[test]
fn timestamps_are_normalized_in_the_fixtures() {
    for dir in scenario_dirs() {
        let run = read_json(&dir.join("run.json"));
        assert_eq!(run["start_time"], "<TIMESTAMP>");
        assert_eq!(run["end_time"], "<END_TIME>");
        assert_eq!(run["run_id"], "<RUN_ID>");
    }
}
