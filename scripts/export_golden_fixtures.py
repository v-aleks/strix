#!/usr/bin/env python3
"""Export golden fixtures for the Rust port from the Python implementation.

The Rust rewrite must reproduce the artifacts the Python engine writes, byte
for byte, once volatile fields are normalized. This script drives the real
Python ``ReportState`` over a set of synthetic finding scenarios and stores
the result as the contract the Rust crates are tested against.

Usage (from the repository root)::

    uv run python scripts/export_golden_fixtures.py

Output::

    strix-rs/crates/strix-contracts/tests/fixtures/golden/
        scenario-<name>/
            run.json
            vulnerabilities.json
            vulnerabilities.csv
            findings.sarif
            coverage.json
            vulnerabilities/<id>.md
        tool-schemas.json
        manifest.json

Normalization rules (applied to every JSON document):

* every timestamp-like field is replaced by a fixed placeholder, so two
  exports of the same scenario compare equal;
* the run id is replaced by a stable placeholder;
* the SARIF ``automationDetails.id`` and ``versionControlProvenance`` blocks
  are kept as-is, because they are derived from the scenario, not from the
  clock.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT))

FIXTURE_ROOT = REPO_ROOT / "strix-rs" / "crates" / "strix-contracts" / "tests" / "fixtures" / "golden"

#: Placeholders written in place of volatile values.
TIMESTAMP_PLACEHOLDER = "<TIMESTAMP>"
START_TIME_PLACEHOLDER = "<START_TIME>"
END_TIME_PLACEHOLDER = "<END_TIME>"
RUN_ID_PLACEHOLDER = "<RUN_ID>"

#: Keys whose values are timestamps and must be normalized.
TIMESTAMP_KEYS = frozenset(
    {
        "timestamp",
        "created_at",
        "updated_at",
        "deleted_at",
        "filed_at",
        "generated_at",
        "recorded_at",
        "start_time",
        "process_start_time",
    }
)

#: Keys whose values are run identifiers and must be normalized.
RUN_ID_KEYS = frozenset({"run_id"})

#: Keys that hold the SARIF invocation start time.
END_TIME_KEYS = frozenset({"end_time"})


def _normalize(value: Any, key: str | None = None) -> Any:
    """Recursively replace volatile values with placeholders."""
    if isinstance(value, dict):
        return {name: _normalize(child, name) for name, child in value.items()}
    if isinstance(value, list):
        return [_normalize(child, key) for child in value]
    if key in TIMESTAMP_KEYS and isinstance(value, str):
        return TIMESTAMP_PLACEHOLDER
    if key in RUN_ID_KEYS and isinstance(value, str):
        return RUN_ID_PLACEHOLDER
    if key in END_TIME_KEYS and isinstance(value, str):
        return END_TIME_PLACEHOLDER
    return value


def _normalize_markdown(text: str) -> str:
    """Normalize the timestamps embedded in the rendered markdown reports."""
    lines = []
    for line in text.splitlines(keepends=True):
        if line.startswith("**Found:**"):
            lines.append(f"**Found:** {TIMESTAMP_PLACEHOLDER}\n")
        elif line.startswith("**Generated:**"):
            lines.append(f"**Generated:** {TIMESTAMP_PLACEHOLDER}\n")
        else:
            lines.append(line)
    return "".join(lines)


#: Matches the timestamp format the Python engine writes.
_TIMESTAMP_PATTERN = re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC")


def _normalize_csv(text: str) -> str:
    """Normalize the timestamps in the CSV index.

    The CSV keeps its CRLF line endings and its quoting; only the timestamp
    column is replaced, so two exports of the same scenario compare equal.
    """
    return _TIMESTAMP_PATTERN.sub(TIMESTAMP_PLACEHOLDER, text)


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------

#: A representative CVSS 3.1 vector per severity used by the scenarios.
CVSS_VECTORS = {
    "critical": {
        "AV": "N",
        "AC": "L",
        "PR": "N",
        "UI": "N",
        "S": "C",
        "C": "H",
        "I": "H",
        "A": "H",
    },
    "high": {
        "AV": "N",
        "AC": "L",
        "PR": "N",
        "UI": "N",
        "S": "U",
        "C": "H",
        "I": "H",
        "A": "N",
    },
    "medium": {
        "AV": "N",
        "AC": "L",
        "PR": "L",
        "UI": "N",
        "S": "U",
        "C": "H",
        "I": "N",
        "A": "N",
    },
    "low": {
        "AV": "L",
        "AC": "H",
        "PR": "L",
        "UI": "N",
        "S": "U",
        "C": "L",
        "I": "N",
        "A": "N",
    },
    "info": {
        "AV": "L",
        "AC": "H",
        "PR": "H",
        "UI": "R",
        "S": "U",
        "C": "N",
        "I": "N",
        "A": "N",
    },
}


def _finding(
    title: str,
    severity: str,
    *,
    cwe: str | None = None,
    cve: str | None = None,
    endpoint: str | None = None,
    method: str | None = None,
    code_locations: list[dict[str, Any]] | None = None,
    finding_class: str | None = None,
    dependency_metadata: dict[str, str] | None = None,
) -> dict[str, Any]:
    """Build one synthetic finding for a scenario."""
    payload: dict[str, Any] = {
        "title": title,
        "severity": severity,
        "description": f"Description of {title}.",
        "impact": f"Impact of {title}.",
        "target": "https://example.test/app",
        "technical_analysis": f"Technical analysis of {title}.",
        "poc_description": f"Proof of concept for {title}.",
        "poc_script_code": "```bash\ncurl https://example.test/app\n```",
        "remediation_steps": f"Remediate {title} by validating input.",
        "evidence": f"Observed evidence for {title}.",
        "assumptions": "The target is in scope.",
        "counterevidence": "No counterevidence was found.",
        "confidence": "high",
        "confidence_rationale": "Reproduced twice.",
        "severity_change_conditions": "Severity drops once the fix ships.",
        "fix_effort": "medium",
        "cvss_breakdown": dict(CVSS_VECTORS[severity]),
    }
    if cwe:
        payload["cwe"] = cwe
    if cve:
        payload["cve"] = cve
    if endpoint:
        payload["endpoint"] = endpoint
    if method:
        payload["method"] = method
    if code_locations:
        payload["code_locations"] = code_locations
    if finding_class:
        payload["finding_class"] = finding_class
    if dependency_metadata:
        payload["dependency_metadata"] = dependency_metadata
    return payload


#: Every scenario the fixture set contains.
SCENARIOS: list[dict[str, Any]] = [
    {
        "name": "empty-scan",
        "findings": [],
        "coverage": [],
        "description": "A clean scan still writes every artifact, including an empty SARIF.",
    },
    {
        "name": "single-critical",
        "findings": [_finding("Unauthenticated RCE in upload handler", "critical", cwe="CWE-94")],
        "coverage": [],
        "description": "One critical finding with a CWE.",
    },
    {
        "name": "single-high-with-code-location",
        "findings": [
            _finding(
                "SQL injection in login",
                "high",
                cwe="CWE-89",
                code_locations=[
                    {
                        "file": "app/login.py",
                        "start_line": 42,
                        "end_line": 44,
                        "snippet": "cursor.execute(query % user_input)",
                        "label": "sink",
                    }
                ],
            )
        ],
        "coverage": [],
        "description": "A finding anchored to a source location.",
    },
    {
        "name": "two-findings-same-cwe",
        "findings": [
            _finding("SQL injection in login", "high", cwe="CWE-89"),
            _finding("SQL injection in search", "medium", cwe="CWE-89"),
        ],
        "coverage": [],
        "description": "Two findings of the same class must not collapse into one.",
    },
    {
        "name": "endpoint-findings",
        "findings": [
            _finding(
                "Broken object level authorization on /api/users/{id}",
                "high",
                cwe="CWE-639",
                endpoint="/api/users/{id}",
                method="GET",
            ),
            _finding(
                "Missing rate limit on /api/login",
                "low",
                cwe="CWE-770",
                endpoint="/api/login",
                method="POST",
            ),
        ],
        "coverage": [],
        "description": "Endpoint-anchored findings carry method and route.",
    },
    {
        "name": "all-severities",
        "findings": [
            _finding(f"Finding with severity {severity}", severity, cwe=f"CWE-{100 + index}")
            for index, severity in enumerate(("critical", "high", "medium", "low", "info"))
        ],
        "coverage": [],
        "description": "Every severity level appears at least once.",
    },
    {
        "name": "dependency-cve",
        "findings": [
            _finding(
                "Vulnerable dependency: requests 2.19.0",
                "high",
                cwe="CWE-1395",
                cve="CVE-2018-18074",
                finding_class="dependency_cve",
                dependency_metadata={
                    "package_name": "requests",
                    "package_ecosystem": "PyPI",
                    "installed_version": "2.19.0",
                    "fixed_version": "2.20.0",
                    "introduced_by": "app/api.py",
                    "dependency_path": "app > requirements.txt > requests",
                },
            )
        ],
        "coverage": [],
        "description": "A dependency finding with contextual metadata.",
    },
    {
        "name": "title-with-formula-prefix",
        "findings": [_finding("=cmd|'/C calc'!A1 in report title", "medium", cwe="CWE-79")],
        "coverage": [],
        "description": "A CSV-formula-looking title must be escaped in the CSV index.",
    },
    {
        "name": "title-with-backticks",
        "findings": [_finding("Reflected XSS via ``code`` marker", "medium", cwe="CWE-79")],
        "coverage": [],
        "description": "Backticks in a title must not break the markdown fence.",
    },
    {
        "name": "coverage-reported",
        "findings": [_finding("Stored XSS in comment body", "high", cwe="CWE-79")],
        "coverage": [
            {
                "surface": "comment_body",
                "risk_area": "xss",
                "outcome": "reported",
                "evidence": "Payload stored and rendered unescaped.",
            }
        ],
        "description": "A scan that recorded one covered surface.",
    },
    {
        "name": "coverage-ruled-out",
        "findings": [],
        "coverage": [
            {
                "surface": "admin_panel",
                "risk_area": "access_control",
                "outcome": "ruled_out",
                "evidence": "All endpoints require a session with the admin role.",
            }
        ],
        "description": "A surface that was reviewed and ruled out.",
    },
    {
        "name": "coverage-not-applicable",
        "findings": [],
        "coverage": [
            {
                "surface": "graphql",
                "risk_area": "injection",
                "outcome": "not_applicable",
                "evidence": "The target exposes no GraphQL endpoint.",
            }
        ],
        "description": "A surface that does not apply to this target.",
    },
    {
        "name": "coverage-needs-follow-up",
        "findings": [],
        "coverage": [
            {
                "surface": "websocket",
                "risk_area": "authn",
                "outcome": "needs_follow_up",
                "evidence": "Token expiry behaviour could not be observed.",
            }
        ],
        "description": "A surface that needs more work.",
    },
    {
        "name": "coverage-no-issue-found",
        "findings": [],
        "coverage": [
            {
                "surface": "static_assets",
                "risk_area": "info_disclosure",
                "outcome": "no_issue_found",
                "evidence": "No secrets found in served assets.",
            }
        ],
        "description": "A surface with nothing to report.",
    },
    {
        "name": "coverage-mixed",
        "findings": [_finding("Open redirect on /logout", "low", cwe="CWE-601")],
        "coverage": [
            {
                "surface": "logout",
                "risk_area": "open_redirect",
                "outcome": "reported",
                "evidence": "The next parameter accepts an absolute URL.",
            },
            {
                "surface": "password_reset",
                "risk_area": "authn",
                "outcome": "ruled_out",
                "evidence": "Reset tokens are single use and expire in 15 minutes.",
            },
            {
                "surface": "file_upload",
                "risk_area": "path_traversal",
                "outcome": "needs_follow_up",
                "evidence": "Upload path sanitization needs a second look.",
            },
        ],
        "description": "Several coverage entries with different outcomes.",
    },
    {
        "name": "code-locations-with-fix",
        "findings": [
            _finding(
                "Command injection in ping utility",
                "critical",
                cwe="CWE-78",
                code_locations=[
                    {
                        "file": "app/net/ping.py",
                        "start_line": 10,
                        "end_line": 12,
                        "snippet": "subprocess.run(f'ping {host}', shell=True)",
                        "label": "sink",
                        "fix_before": "subprocess.run(f'ping {host}', shell=True)",
                        "fix_after": "subprocess.run(['ping', host], shell=False)",
                    }
                ],
            )
        ],
        "coverage": [],
        "description": "A code location that carries a proposed fix.",
    },
    {
        "name": "multiple-code-locations",
        "findings": [
            _finding(
                "Path traversal in download handler",
                "high",
                cwe="CWE-22",
                code_locations=[
                    {
                        "file": "app/files/download.py",
                        "start_line": 21,
                        "end_line": 23,
                        "snippet": "open(os.path.join(root, name))",
                        "label": "sink",
                    },
                    {
                        "file": "app/files/preview.py",
                        "start_line": 55,
                        "end_line": 57,
                        "snippet": "open(os.path.join(root, name))",
                        "label": "sink",
                    },
                ],
            )
        ],
        "coverage": [],
        "description": "One finding spanning two source files.",
    },
    {
        "name": "no-cwe-no-location",
        "findings": [_finding("Informational observation about headers", "info")],
        "coverage": [],
        "description": "A finding with neither CWE nor source location.",
    },
    {
        "name": "mixed-classes",
        "findings": [
            _finding("SQL injection in orders", "high", cwe="CWE-89"),
            _finding(
                "Vulnerable dependency: lodash 4.17.15",
                "medium",
                cwe="CWE-1395",
                cve="CVE-2020-8203",
                finding_class="dependency_cve",
                dependency_metadata={
                    "package_name": "lodash",
                    "package_ecosystem": "npm",
                    "installed_version": "4.17.15",
                    "fixed_version": "4.17.19",
                    "introduced_by": "package.json",
                    "dependency_path": "app > package.json > lodash",
                },
            ),
            _finding("Missing security headers", "low", cwe="CWE-693"),
        ],
        "coverage": [],
        "description": "Dynamic and dependency findings in one run.",
    },
    {
        "name": "long-title",
        "findings": [
            _finding(
                "A very long finding title " + ("describing the impact in detail " * 8),
                "medium",
                cwe="CWE-400",
            )
        ],
        "coverage": [],
        "description": "A long title must survive the markdown and CSV writers.",
    },
]


# ---------------------------------------------------------------------------
# Export
# ---------------------------------------------------------------------------


def _render_scenario(scenario: dict[str, Any], workdir: Path) -> Path:
    """Render one scenario with the Python engine and return its run directory."""
    os.environ["STRIX_TELEMETRY"] = "false"
    os.environ.setdefault("STRIX_LLM", "mock/model")

    from strix.report.state import ReportState  # noqa: PLC0415
    from strix.tools.coverage import tools as coverage_tools  # noqa: PLC0415

    previous_cwd = Path.cwd()
    os.chdir(workdir)
    try:
        # The coverage ledger is module-global in the Python engine; reset it so
        # one scenario cannot leak entries into the next.
        with coverage_tools._coverage_lock:
            coverage_tools._coverage_storage.clear()

        state = ReportState(run_name=scenario["name"])
        state.run_record["targets_info"] = [{"target": "https://example.test/app", "type": "url"}]
        state.run_record["scan_mode"] = "quick"
        state.run_record["scope_mode"] = "auto"
        state.final_scan_result = (
            "# Executive Summary\n\nNo critical issues remain open.\n\n"
            "# Methodology\n\nAutomated review of every reachable surface.\n\n"
            "# Technical Analysis\n\nSee the individual findings.\n\n"
            "# Recommendations\n\nFix the findings above.\n"
        )

        for finding in scenario["findings"]:
            state.add_vulnerability_report(**finding)

        for entry in scenario["coverage"]:
            result = coverage_tools._record_impl(
                surface=entry["surface"],
                risk_area=entry["risk_area"],
                outcome=entry["outcome"],
                evidence=entry["evidence"],
                agent_id="agent-root",
                agent_name="Root Agent",
            )
            if not result.get("success"):
                raise RuntimeError(f"coverage entry rejected: {result}")

        state.save_run_data(mark_complete=True)
        return state.get_run_dir()
    finally:
        os.chdir(previous_cwd)


def _export_scenario(scenario: dict[str, Any], workdir: Path, destination: Path) -> dict[str, Any]:
    """Render one scenario and copy its normalized artifacts to *destination*."""
    run_dir = _render_scenario(scenario, workdir)
    destination.mkdir(parents=True, exist_ok=True)

    exported: list[str] = []
    for name in ("run.json", "vulnerabilities.json", "findings.sarif", "coverage.json"):
        source = run_dir / name
        if not source.is_file():
            continue
        payload = json.loads(source.read_text(encoding="utf-8"))
        normalized = _normalize(payload)
        (destination / name).write_text(
            json.dumps(normalized, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        exported.append(name)

    csv_source = run_dir / "vulnerabilities.csv"
    if csv_source.is_file():
        with csv_source.open("r", encoding="utf-8", newline="") as handle:
            csv_text = handle.read()
        with (destination / "vulnerabilities.csv").open(
            "w", encoding="utf-8", newline=""
        ) as handle:
            handle.write(_normalize_csv(csv_text))
        exported.append("vulnerabilities.csv")

    markdown_dir = run_dir / "vulnerabilities"
    if markdown_dir.is_dir():
        target_dir = destination / "vulnerabilities"
        target_dir.mkdir(exist_ok=True)
        for markdown in sorted(markdown_dir.glob("*.md")):
            (target_dir / markdown.name).write_text(
                _normalize_markdown(markdown.read_text(encoding="utf-8")),
                encoding="utf-8",
            )
            exported.append(f"vulnerabilities/{markdown.name}")

    return {
        "name": scenario["name"],
        "description": scenario["description"],
        "finding_count": len(scenario["findings"]),
        "coverage_count": len(scenario["coverage"]),
        "artifacts": exported,
    }


def _export_tool_schemas(destination: Path) -> dict[str, Any]:
    """Export the JSON schema of every agent tool the Python engine exposes."""
    from strix.agents.factory import _BASE_TOOLS  # noqa: PLC0415

    tools = []
    for tool in _BASE_TOOLS:
        schema = dict(getattr(tool, "params_json_schema", {}) or {})
        schema.pop("title", None)
        schema.pop("$defs", None)
        tools.append(
            {
                "name": tool.name,
                "description": tool.description,
                "parameters": schema,
            }
        )
    payload = {"count": len(tools), "tools": tools}
    destination.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return {"name": "tool-schemas", "tool_count": len(tools)}


def main() -> int:
    """Render every scenario and write the golden fixture tree."""
    if FIXTURE_ROOT.exists():
        shutil.rmtree(FIXTURE_ROOT)
    FIXTURE_ROOT.mkdir(parents=True)

    manifest: list[dict[str, Any]] = []
    workdir = Path(tempfile.mkdtemp(prefix="strix-golden-"))
    try:
        for scenario in SCENARIOS:
            scenario_dir = FIXTURE_ROOT / f"scenario-{scenario['name']}"
            manifest.append(_export_scenario(scenario, workdir, scenario_dir))
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    manifest.append(_export_tool_schemas(FIXTURE_ROOT / "tool-schemas.json"))

    (FIXTURE_ROOT / "manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "source": "python:strix.report.state",
                "scenarios": manifest,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    print(f"Exported {len(manifest)} fixture(s) to {FIXTURE_ROOT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
