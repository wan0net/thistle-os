#!/usr/bin/env python3
"""Regression tests for release input and artifact provenance validation."""

import copy
from pathlib import Path
import unittest

import validate_release


GOOD_SHA = "a" * 40
GOOD_RUN = {
    "id": 12345,
    "html_url": "https://github.com/wan0net/thistle-os/actions/runs/12345",
    "name": "ThistleOS CI",
    "path": ".github/workflows/build.yml",
    "event": "push",
    "status": "completed",
    "conclusion": "success",
    "head_branch": "main",
    "head_sha": GOOD_SHA,
    "repository": {"full_name": "wan0net/thistle-os"},
    "head_repository": {"full_name": "wan0net/thistle-os"},
}


class ReleaseValidationTests(unittest.TestCase):
    def test_safe_release_tags_are_accepted(self):
        for tag in ["v0.5.1", "v1.0.0-rc.1", "v12.34.56-beta"]:
            self.assertEqual(validate_release.validate_tag(tag), tag)

    def test_shell_and_ref_injection_tags_are_rejected(self):
        for tag in [
            "v0.5.1; id",
            "v0.5.1$(id)",
            "v0.5.1`id`",
            "--help",
            "refs/heads/main",
            "v01.2.3",
        ]:
            with self.subTest(tag=tag), self.assertRaises(validate_release.ValidationError):
                validate_release.validate_tag(tag)

    def test_matching_successful_trusted_run_is_accepted(self):
        provenance = validate_release.validate_run(
            GOOD_RUN, "wan0net/thistle-os", GOOD_SHA
        )
        self.assertEqual(provenance["run_id"], 12345)
        self.assertEqual(provenance["commit"], GOOD_SHA)

    def test_failed_and_different_commit_runs_are_rejected(self):
        for field, value in [
            ("conclusion", "failure"),
            ("status", "in_progress"),
            ("head_sha", "b" * 40),
        ]:
            metadata = copy.deepcopy(GOOD_RUN)
            metadata[field] = value
            with self.subTest(field=field), self.assertRaises(validate_release.ValidationError):
                validate_release.validate_run(
                    metadata, "wan0net/thistle-os", GOOD_SHA
                )

    def test_untrusted_workflow_event_ref_and_repository_are_rejected(self):
        cases = [
            ("path", ".github/workflows/tests.yml"),
            ("event", "pull_request"),
            ("head_branch", "feature"),
            ("repository", {"full_name": "attacker/fork"}),
            ("head_repository", {"full_name": "attacker/fork"}),
        ]
        for field, value in cases:
            metadata = copy.deepcopy(GOOD_RUN)
            metadata[field] = value
            with self.subTest(field=field), self.assertRaises(validate_release.ValidationError):
                validate_release.validate_run(
                    metadata, "wan0net/thistle-os", GOOD_SHA
                )

    def test_workflow_does_not_render_inputs_into_shell_source(self):
        workflow = (
            Path(__file__).resolve().parents[1] / ".github/workflows/release.yml"
        ).read_text(encoding="utf-8")
        self.assertNotIn('tag="${{ inputs.tag }}"', workflow)
        self.assertNotIn('gh run download "${{ inputs.run_id }}"', workflow)
        self.assertIn("RELEASE_TAG: ${{ inputs.tag }}", workflow)
        self.assertIn("SOURCE_RUN_ID: ${{ inputs.run_id }}", workflow)


if __name__ == "__main__":
    unittest.main()
