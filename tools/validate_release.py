#!/usr/bin/env python3
"""Validate release tags and GitHub Actions artifact provenance."""

import argparse
import json
from pathlib import Path
import re
import sys


RELEASE_TAG = re.compile(
    r"^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$"
)
COMMIT_SHA = re.compile(r"^[0-9a-f]{40}$")


class ValidationError(ValueError):
    pass


def validate_tag(tag):
    if not RELEASE_TAG.fullmatch(tag):
        raise ValidationError(
            "release tag must use vMAJOR.MINOR.PATCH with an optional safe prerelease suffix"
        )
    return tag


def validate_run(metadata, expected_repository, expected_sha):
    if not COMMIT_SHA.fullmatch(expected_sha):
        raise ValidationError("expected tag commit is not a full lowercase commit SHA")

    expected = {
        "name": "ThistleOS CI",
        "path": ".github/workflows/build.yml",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_branch": "main",
        "head_sha": expected_sha,
    }
    for field, value in expected.items():
        if metadata.get(field) != value:
            raise ValidationError(
                f"run {field} mismatch: expected {value!r}, got {metadata.get(field)!r}"
            )

    repository = (metadata.get("repository") or {}).get("full_name")
    head_repository = (metadata.get("head_repository") or {}).get("full_name")
    if repository != expected_repository:
        raise ValidationError(
            f"run repository mismatch: expected {expected_repository!r}, got {repository!r}"
        )
    if head_repository != expected_repository:
        raise ValidationError(
            "run head repository is not the trusted release repository"
        )

    return {
        "run_id": metadata.get("id"),
        "run_url": metadata.get("html_url"),
        "repository": repository,
        "workflow": metadata["path"],
        "event": metadata["event"],
        "branch": metadata["head_branch"],
        "commit": metadata["head_sha"],
        "conclusion": metadata["conclusion"],
    }


def main():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    tag_parser = subparsers.add_parser("tag")
    tag_parser.add_argument("tag")

    run_parser = subparsers.add_parser("run")
    run_parser.add_argument("metadata", type=Path)
    run_parser.add_argument("--repository", required=True)
    run_parser.add_argument("--commit", required=True)

    args = parser.parse_args()
    try:
        if args.command == "tag":
            print(f"Validated release tag: {validate_tag(args.tag)}")
        else:
            metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
            provenance = validate_run(metadata, args.repository, args.commit)
            print("Validated artifact provenance:")
            print(json.dumps(provenance, sort_keys=True))
    except (OSError, json.JSONDecodeError, ValidationError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
