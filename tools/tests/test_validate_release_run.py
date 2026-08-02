import unittest

from tools.validate_release_run import validate_run


SHA = "a" * 40
EXPECTED = {
    "repository": {"full_name": "wan0net/thistle-os"},
    "head_repository": {"full_name": "wan0net/thistle-os"},
    "workflow_name": "ThistleOS CI",
    "path": ".github/workflows/build.yml",
    "status": "completed",
    "conclusion": "success",
    "event": "push",
    "head_branch": "main",
    "head_sha": SHA,
}


def errors(run):
    return validate_run(
        run,
        repository="wan0net/thistle-os",
        workflow_name="ThistleOS CI",
        workflow_path=".github/workflows/build.yml",
        event="push",
        ref="main",
        sha=SHA,
    )


class ReleaseRunValidationTests(unittest.TestCase):
    def test_valid_run_is_accepted(self):
        self.assertEqual(errors(EXPECTED), [])

    def test_failed_run_is_rejected(self):
        run = {**EXPECTED, "conclusion": "failure"}
        self.assertTrue(any("conclusion" in error for error in errors(run)))

    def test_wrong_sha_workflow_repository_and_event_are_rejected(self):
        for field, value in [
            ("head_sha", "b" * 40),
            ("workflow_name", "Other CI"),
            ("path", ".github/workflows/other.yml"),
            ("repository", {"full_name": "attacker/thistle-os"}),
            ("head_repository", {"full_name": "attacker/thistle-os"}),
            ("event", "pull_request"),
        ]:
            with self.subTest(field=field):
                run = {**EXPECTED, field: value}
                self.assertTrue(errors(run))


if __name__ == "__main__":
    unittest.main()
