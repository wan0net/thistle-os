You are operating in a continuous autonomous development loop. Do not stop to ask questions or request permission. If you encounter ambiguity, document it as an open question in QUESTIONS.md and continue.

## Loop Structure

Each iteration must follow this sequence:

### 1. Review Phase
- Load personas from persona.md
- Each persona independently reviews the full project state: codebase, open issues, CHANGELOG, QUESTIONS.md, and any running service outputs
- Each persona produces a prioritised list of missing or underdeveloped features
- Synthesise into a BACKLOG.md update — add new items, do not remove existing ones

### 2. Build Phase (Persona: Senior Engineer)
- Select the highest-priority unblocked item from BACKLOG.md
- Implement it on a new feature branch (branch naming: feat/<short-slug>)
- Do not merge to main under any circumstances
- Document what was built and why in CHANGELOG.md

### 3. Test Phase
- Write or update test cases covering the new feature
- Run the full test suite
- If tests fail: document the failure in QUESTIONS.md, revert the feature branch, mark the backlog item as BLOCKED, and continue to the next iteration
- If tests pass: mark the backlog item as DONE (unmerged)

### 4. Build Integrity Check
- Verify the GitHub Actions / CI build passes on the feature branch
- If it fails: document in QUESTIONS.md, mark item BLOCKED, continue

### 5. Loop
- Return to step 1 with updated project state

## Hard Constraints
- Always reload persona.md at the start of each Review Phase — do not cache persona definitions between iterations
- Never merge feature branches to main
- Never delete or overwrite QUESTIONS.md or BACKLOG.md entries — append only
- Never make changes outside the current feature branch scope
- If a loop iteration produces no actionable backlog items, document this and halt — do not spin idle
- Maximum one feature per iteration
