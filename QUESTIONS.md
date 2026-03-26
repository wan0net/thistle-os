# ThistleOS — Open Questions

## Iteration 1 — 2026-03-26

(No open questions yet — first iteration.)

### Q1: CI only triggers on main/PR-to-main
GitHub Actions workflows (`build.yml`, `tests.yml`) only run on `push: main` and `pull_request: main`. Feature branches don't trigger CI unless a PR is opened. The loop constraint says "never merge to main" — should we open draft PRs to trigger CI, or add branch triggers to the workflows?
**Workaround used:** Ran full cargo test locally (632/632 pass including 44 new). Feature branch is push-verified but not CI-verified.
