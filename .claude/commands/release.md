# Release Process for drep

Execute the complete release process for a new version of drep.

## Prerequisites

Before starting, ensure:
- All feature branches are merged to main
- All tests are passing
- `main` branch is clean and up-to-date
- `.tokens` file exists with `TESTPYPI_TOKEN` and `PYPI_TOKEN`

## Instructions

When the user runs `/release`, follow these steps:

### Step 1: Determine Version Number

Ask the user what version to release:
- Current version is in `pyproject.toml`
- Follow Semantic Versioning (MAJOR.MINOR.PATCH)
- Increment MAJOR for breaking changes, MINOR for features, PATCH for fixes

### Step 2: Create TODO List

Use TodoWrite to create a comprehensive task list with these items:
1. Update CHANGELOG.md with vX.Y.Z release notes
2. Update version in pyproject.toml to X.Y.Z
3. Update README.md if needed (document new features)
4. Update docs/technical-design.md version and recent updates
5. Run full test suite to verify (count expected tests)
6. Build package: `rm -rf dist/ && ./venv/bin/python -m build`
7. Upload to TestPyPI for verification
8. Test install from TestPyPI (optional manual verification)
9. Upload to Production PyPI
10. Create git tag: `git tag vX.Y.Z`
11. Push tag to GitHub and Gitea
12. Create GitHub release with CHANGELOG content
13. Verify PyPI listing
14. Push final commits to both remotes

### Step 3: Update CHANGELOG.md

Add a new section at the top (after `## [Unreleased]`):

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added - Feature Name 🎉

**Release Type:** Brief description

- **Feature Category**: Description
  - Bullet points of what was added
  - More details
  - Technical implementation notes

### Testing
- **N new tests added** (total count passing)

**Category of Tests (N tests)**:
- Test description 1
- Test description 2
- **Finding/Note**: Key insights

### Changed
- What changed in this release

### Improved
- What was improved

### Fixed
- What bugs were fixed

### Development
- Development process notes (e.g., Zero Tech Debt Policy, TDD)
```

**Template Categories to Choose From:**
- Added - New features
- Changed - Changes in existing functionality
- Deprecated - Soon-to-be removed features
- Removed - Now removed features
- Fixed - Bug fixes
- Security - Security fixes
- Testing - Test improvements
- Development - Development process improvements

### Step 4: Update pyproject.toml

Change the version line:
```toml
version = "X.Y.Z"
```

### Step 5: Update README.md

Update the version badge/note at the top:
```markdown
> **vX.Y.Z:** Brief description of key feature! Full support for...
```

Add Quick Start examples if there are new features that need documentation.

### Step 6: Update docs/technical-design.md

Update the document header:
```markdown
**Document Version:** X.Y
**Last Updated:** YYYY-MM-DD
**Status:** Production (vX.Y.Z) - Phase 1, 2, & 3 Complete
```

Add a new entry to Recent Updates:
```markdown
**vX.Y (YYYY-MM-DD):**
- **🎉 Version X.Y.Z Release - Feature Name**
- Key feature 1
- Key feature 2
- N tests passing (previous + new)
- Notable achievements
```

### Step 7: Run Full Test Suite

```bash
./venv/bin/pytest tests/ -k "not integration" -q | tail -5
```

Verify the expected test count passes.

### Step 8: Build Package

```bash
rm -rf dist/
./venv/bin/python -m build
```

Verify output shows:
```
Successfully built drep_ai-X.Y.Z.tar.gz and drep_ai-X.Y.Z-py3-none-any.whl
```

### Step 9: Upload to TestPyPI

```bash
source .tokens
./venv/bin/twine upload --repository testpypi dist/* \
  --username __token__ --password $TESTPYPI_TOKEN
```

Verify the URL shown: `https://test.pypi.org/project/drep-ai/X.Y.Z/`

### Step 10: Upload to Production PyPI

```bash
source .tokens
./venv/bin/twine upload dist/* \
  --username __token__ --password $PYPI_TOKEN
```

Verify the URL shown: `https://pypi.org/project/drep-ai/X.Y.Z/`

### Step 11: Create Git Tag

```bash
git tag vX.Y.Z
git tag | tail -5  # Verify tag created
```

### Step 12: Push Tag to Both Remotes

```bash
git push origin vX.Y.Z
GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent' git push github vX.Y.Z
```

### Step 13: Create GitHub Release

Extract the relevant section from CHANGELOG.md and create the release:

```bash
gh release create vX.Y.Z \
  --title "vX.Y.Z - Feature Name" \
  --notes "# vX.Y.Z - Feature Name

**Release Type:** Brief description

## What's New

### Feature Category
- Key feature 1
- Key feature 2
- Key feature 3

### Another Category
- More details

## Testing & Security

- **N new tests added** (total passing)
- Test category 1 (count)
- Test category 2 (count)

## Installation

\`\`\`bash
pip install drep-ai
# or upgrade
pip install --upgrade drep-ai
\`\`\`

**Full Changelog**: https://github.com/slb350/drep/blob/main/CHANGELOG.md#XYZ---YYYY-MM-DD"
```

Replace URL anchor with lowercase version-date format (e.g., `#110---2025-11-09`)

### Step 14: Push Final Commits

```bash
git push origin main
GIT_SSH_COMMAND='ssh -i ~/.ssh/github_any_agent' git push github main
```

### Step 15: Verification Checklist

Present this checklist to the user for manual verification:

- [ ] PyPI listing: https://pypi.org/project/drep-ai/X.Y.Z/
- [ ] TestPyPI listing: https://test.pypi.org/project/drep-ai/X.Y.Z/
- [ ] GitHub release: https://github.com/slb350/drep/releases/tag/vX.Y.Z
- [ ] GitHub tag visible: https://github.com/slb350/drep/tags
- [ ] Gitea tag visible: http://192.168.1.14:3000/steve/drep/tags
- [ ] CHANGELOG.md updated on GitHub
- [ ] README.md updated on GitHub
- [ ] Technical design updated

### Step 16: Mark All Todos Complete

Use TodoWrite to mark all tasks as completed.

## Success Message

Present this summary to the user:

```
🎉 vX.Y.Z Release Complete!

📊 Release Statistics:
- Version: X.Y.Z
- Release Date: YYYY-MM-DD
- Test Count: N tests passing
- New Tests: N
- New Features: Brief list
- Commits: N (documentation + version)

🔗 Links:
- PyPI: https://pypi.org/project/drep-ai/X.Y.Z/
- GitHub Release: https://github.com/slb350/drep/releases/tag/vX.Y.Z
- Full Changelog: https://github.com/slb350/drep/blob/main/CHANGELOG.md

Installation:
```bash
pip install --upgrade drep-ai
```

All release tasks completed successfully! 🎊
```

## Common Issues

### Issue: .tokens file not found
**Solution**: Copy from `~/Dev/any-agent/.tokens` or create new:
```bash
cp ~/Dev/any-agent/.tokens .tokens
# or create manually with TESTPYPI_TOKEN and PYPI_TOKEN
```

### Issue: Git push fails to GitHub
**Solution**: Ensure SSH key configured:
```bash
git config core.sshCommand "ssh -i ~/.ssh/github_any_agent"
# or use GIT_SSH_COMMAND prefix for each push
```

### Issue: Version already exists on PyPI
**Solution**: Cannot re-upload same version. Increment version and retry.

### Issue: Tests failing
**Solution**: Fix tests before releasing. Never release with failing tests.

## Notes

- **NEVER commit .tokens file** - it's in .gitignore
- **Package name on PyPI**: `drep-ai` (not `drep`)
- **Module name**: `drep` (import path unchanged)
- **CLI command**: `drep` (executable name unchanged)
- **Zero tech debt policy**: All tests must pass before release
- **Follow semantic versioning**: MAJOR.MINOR.PATCH format
