"""`drep init-hooks` tests.

This command does the only genuinely dangerous thing in the install flow: it
edits the user's pre-commit config and touches git's hook configuration. It
lives in Python precisely so these failure modes can be tested - the shell
version could leave a repo with every hook silently disabled and no way to
notice.
"""

import subprocess
from pathlib import Path

import pytest
import yaml

from drep.cli import cli


def _git(*args, cwd):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=False)


@pytest.fixture
def repo(tmp_path):
    """A real git repository, since the command reads git config."""
    _git("init", "-q", cwd=tmp_path)
    (tmp_path / "a.py").write_text("x = 1\n")
    return tmp_path


class TestConfigMerging:
    """Never append text: parse, merge, rewrite."""

    def test_creates_a_config_when_none_exists(self, runner, repo):
        result = runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert result.exit_code == 0
        config = yaml.safe_load((repo / ".pre-commit-config.yaml").read_text())
        ids = [h["id"] for r in config["repos"] for h in r["hooks"]]
        assert "drep-check-push" in ids

    def test_merges_into_a_config_using_zero_indent_style(self, runner, repo):
        """pre-commit's own documented style. Appending text broke this."""
        (repo / ".pre-commit-config.yaml").write_text(
            "repos:\n-   repo: https://github.com/psf/black\n    rev: 24.1.0\n"
            "    hooks:\n    -   id: black\n"
        )

        result = runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert result.exit_code == 0
        config = yaml.safe_load((repo / ".pre-commit-config.yaml").read_text())
        repos = [r["repo"] for r in config["repos"]]
        assert "https://github.com/psf/black" in repos
        assert any("drep" in r for r in repos)

    def test_merges_when_other_top_level_keys_follow_repos(self, runner, repo):
        """`ci:` or `default_language_version:` after repos broke appending."""
        (repo / ".pre-commit-config.yaml").write_text(
            yaml.dump(
                {
                    "repos": [
                        {"repo": "local", "hooks": [{"id": "x", "name": "x", "entry": "true"}]}
                    ],
                    "fail_fast": True,
                    "default_language_version": {"python": "python3.11"},
                }
            )
        )

        result = runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert result.exit_code == 0
        config = yaml.safe_load((repo / ".pre-commit-config.yaml").read_text())
        # Their keys survive, ours is added
        assert config["fail_fast"] is True
        assert config["default_language_version"] == {"python": "python3.11"}
        assert any("drep" in r["repo"] for r in config["repos"])

    def test_is_idempotent(self, runner, repo):
        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])
        first = (repo / ".pre-commit-config.yaml").read_text()

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert (repo / ".pre-commit-config.yaml").read_text() == first

    def test_pins_a_real_version_not_a_moving_ref(self, runner, repo):
        """`rev: main` is unpinned; pre-commit warns and autoupdate rewrites it."""
        from drep import __version__

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        config = yaml.safe_load((repo / ".pre-commit-config.yaml").read_text())
        drep_repo = next(r for r in config["repos"] if "drep" in r["repo"])
        assert drep_repo["rev"] == f"v{__version__}"

    def test_refuses_to_touch_an_unparseable_config(self, runner, repo):
        (repo / ".pre-commit-config.yaml").write_text("repos: [ unclosed\n")

        result = runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert result.exit_code == 1
        assert "could not be parsed" in result.output.lower()


class TestHooksPathSafety:
    """The failure mode that made the shell version unsafe.

    Emptying core.hooksPath disables *every* hook in the repo. If anything
    fails between setting and restoring it, the repo is left silently
    hook-less, and a re-run cannot tell - an empty value reads as absent.
    """

    def test_a_local_hooks_path_is_restored_exactly(self, runner, repo):
        _git("config", "--local", "core.hooksPath", ".githooks", cwd=repo)

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        got = _git("config", "--local", "--get", "core.hooksPath", cwd=repo).stdout.strip()
        assert got == ".githooks"

    def test_a_local_hooks_path_survives_a_failure_midway(self, runner, repo, monkeypatch):
        """The critical case: restore must happen even when install raises."""
        _git("config", "--local", "core.hooksPath", ".githooks", cwd=repo)

        import drep.cli_init_hooks as mod

        def boom(*args, **kwargs):
            raise RuntimeError("pre-commit exploded")

        monkeypatch.setattr(mod, "_run_pre_commit_install", boom)

        runner.invoke(cli, ["init-hooks", "--path", str(repo)])

        got = _git("config", "--local", "--get", "core.hooksPath", cwd=repo).stdout.strip()
        assert got == ".githooks", "a failed install must not strip the repo's hooks config"

    def test_no_local_override_is_left_behind(self, runner, repo):
        """With only a global hooksPath, the repo must end with no local key."""
        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        result = _git("config", "--local", "--get", "core.hooksPath", cwd=repo)
        assert result.returncode != 0, "a leftover local override disables every hook"


class TestChainer:
    """A global hooksPath means git ignores .git/hooks entirely."""

    def test_writes_a_chainer_when_a_global_hooks_path_is_set(self, runner, repo, tmp_path):
        hooks_dir = tmp_path / "globalhooks"
        hooks_dir.mkdir()
        _git("config", "--local", "core.hooksPath", str(hooks_dir), cwd=repo)

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        chainer = hooks_dir / "pre-push"
        assert chainer.exists()
        assert chainer.stat().st_mode & 0o111, "a non-executable hook is ignored by git"
        # git-common-dir, not $REPO/.git: the latter is a *file* in a worktree
        assert "git-common-dir" in chainer.read_text()

    def test_an_existing_chainer_that_does_not_chain_is_flagged(self, runner, repo, tmp_path):
        hooks_dir = tmp_path / "globalhooks"
        hooks_dir.mkdir()
        unrelated = hooks_dir / "pre-push"
        unrelated.write_text("#!/bin/sh\necho unrelated\n")
        unrelated.chmod(0o755)
        _git("config", "--local", "core.hooksPath", str(hooks_dir), cwd=repo)

        result = runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        # Assuming it chains would silently mean drep never runs
        assert "does not appear to chain" in result.output.lower()

    def test_a_relative_hooks_path_resolves_against_the_repo(self, runner, repo):
        """git resolves it relative to the repository, not the cwd.

        Resolving against the cwd wrote the chainer into whatever directory the
        caller happened to be standing in - it landed in drep's own checkout.
        """
        _git("config", "--local", "core.hooksPath", ".githooks", cwd=repo)

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert (repo / ".githooks" / "pre-push").exists()
        assert not (Path.cwd() / ".githooks").exists()

    def test_leaves_an_existing_chainer_alone(self, runner, repo, tmp_path):
        hooks_dir = tmp_path / "globalhooks"
        hooks_dir.mkdir()
        chainer = hooks_dir / "pre-push"
        chainer.write_text('#!/bin/bash\nexec "$(git rev-parse --git-common-dir)/hooks/pre-push"\n')
        chainer.chmod(0o755)
        original = chainer.read_text()
        _git("config", "--local", "core.hooksPath", str(hooks_dir), cwd=repo)

        runner.invoke(cli, ["init-hooks", "--path", str(repo), "--skip-install"])

        assert chainer.read_text() == original


class TestOutsideARepo:
    def test_refuses_politely(self, runner, tmp_path):
        result = runner.invoke(cli, ["init-hooks", "--path", str(tmp_path), "--skip-install"])

        assert result.exit_code == 1
        assert "git repository" in result.output.lower()
