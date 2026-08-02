#!/usr/bin/env python3
"""Fail-closed validation for the CI run used to publish a release."""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any


def _get(data: dict[str, Any], *keys: str) -> Any:
    value: Any = data
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def validate_run(
    run: dict[str, Any], *, repository: str, workflow_name: str, workflow_path: str,
    event: str, ref: str, sha: str,
) -> list[str]:
    """Return every violated release provenance requirement."""
    checks = [
        ("repository.full_name", _get(run, "repository", "full_name"), repository),
        ("head_repository.full_name", _get(run, "head_repository", "full_name"), repository),
        ("workflow_name", run.get("workflow_name"), workflow_name),
        ("path", run.get("path"), workflow_path),
        ("status", run.get("status"), "completed"),
        ("conclusion", run.get("conclusion"), "success"),
        ("event", run.get("event"), event),
        ("head_branch", run.get("head_branch"), ref),
        ("head_sha", run.get("head_sha"), sha),
    ]
    return [f"{name}: expected {expected!r}, got {actual!r}" for name, actual, expected in checks if actual != expected]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-json", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow-name", required=True)
    parser.add_argument("--workflow-path", required=True)
    parser.add_argument("--event", default="push")
    parser.add_argument("--ref", default="main")
    parser.add_argument("--sha", required=True)
    args = parser.parse_args()

    try:
        with open(args.run_json, encoding="utf-8") as source:
            run = json.load(source)
    except (OSError, json.JSONDecodeError) as error:
        print(f"ERROR: cannot parse Actions run metadata: {error}", file=sys.stderr)
        return 2

    errors = validate_run(
        run,
        repository=args.repository,
        workflow_name=args.workflow_name,
        workflow_path=args.workflow_path,
        event=args.event,
        ref=args.ref,
        sha=args.sha,
    )
    if errors:
        print("ERROR: selected CI run is not trusted for this release:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print(f"Validated trusted CI run {run.get('id')} for {args.sha}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
