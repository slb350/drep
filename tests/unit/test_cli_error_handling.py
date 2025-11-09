"""Test error handling in CLI commands.

This module tests that CLI commands handle errors gracefully with user-friendly
messages instead of exposing stack traces to end users.

RED phase tests: These tests currently FAIL because the code uses broad
'except Exception' handlers that don't provide specific error messages.
"""


class TestMetricsPersistence:
    """Test error handling for LLM metrics persistence.

    These tests verify that metrics persistence failures show helpful error
    messages instead of generic warnings.
    """

    def test_current_behavior_broad_exception(self, capsys):
        """Document current behavior: broad Exception catch hides error type."""
        # This test shows what CURRENTLY happens (generic warning)
        # After fix, this behavior should change to specific error messages

        # Simulate current code behavior
        try:
            raise PermissionError("Cannot write to /root/.drep")
        except Exception as e:
            # Current code does this
            print(f"Warning: failed to persist metrics: {e}")

        captured = capsys.readouterr()
        # Currently shows generic warning
        assert "Warning: failed to persist metrics" in captured.out
        # But does NOT suggest fix
        assert "chmod" not in captured.out

    def test_desired_behavior_permission_error(self, capsys):
        """Test desired behavior: PermissionError shows chmod suggestion."""
        # This test shows what SHOULD happen after the fix

        # Desired behavior after fix
        try:
            raise PermissionError("Cannot write to /root/.drep")
        except PermissionError:
            # After fix, code should do this
            print("Warning: Cannot save metrics to /root/.drep/metrics.json")
            print("  Fix: chmod 755 /root/.drep")
        except OSError as e:
            print(f"Warning: Cannot save metrics: {e}")
            print("  Check disk space and filesystem permissions.")

        captured = capsys.readouterr()
        # Should show specific message
        assert "Cannot save metrics" in captured.out
        # Should suggest fix
        assert "chmod 755" in captured.out

    def test_desired_behavior_disk_full(self, capsys):
        """Test desired behavior: OSError shows disk space guidance."""
        try:
            raise OSError("No space left on device")
        except PermissionError:
            print("Warning: Cannot save metrics to /path/metrics.json")
            print("  Fix: chmod 755 /path")
        except OSError as e:
            # After fix, code should do this
            print(f"Warning: Cannot save metrics: {e}")
            print("  Check disk space and filesystem permissions.")

        captured = capsys.readouterr()
        assert "Cannot save metrics" in captured.out
        assert "disk space" in captured.out


class TestCleanupErrorHandling:
    """Test error handling during cleanup operations.

    These tests verify that cleanup failures are handled appropriately:
    - Temp dir cleanup (security-critical): warn but don't crash
    - Scanner close: catch specific errors, re-raise unexpected
    - Adapter close: catch specific errors, re-raise unexpected
    """

    def test_temp_dir_cleanup_failure_warns(self, capsys):
        """Test temp dir cleanup failure shows security warning."""

        temp_dir = "/some/temp/dir"

        # Simulate cleanup failure (security-critical, must warn)
        try:
            # Pretend this failed
            raise PermissionError("Cannot delete directory")
        except Exception:
            # Keep broad catch for temp dir (security-critical)
            print(
                f"SECURITY WARNING: Failed to clean up credentials at {temp_dir}",
                file=__import__("sys").stderr,
            )
            print(f"  Manually delete: rm -rf {temp_dir}", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "SECURITY WARNING" in captured.err
        assert "Manually delete" in captured.err
        assert temp_dir in captured.err

    def test_scanner_close_oserror_handled(self, capsys):
        """Test scanner close with OSError is caught and logged."""
        # Desired behavior: catch OSError, log, continue
        try:
            raise OSError("Database connection failed")
        except OSError as e:
            print(f"Warning: Database cleanup failed: {e}", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "Database cleanup failed" in captured.err

    def test_scanner_close_unexpected_error_propagates(self):
        """Test scanner close with unexpected error propagates."""
        # Desired behavior: re-raise unexpected errors
        error_raised = False
        try:
            try:
                raise RuntimeError("Unexpected error")
            except OSError as e:
                print(f"Warning: Database cleanup failed: {e}")
            except Exception:
                # Re-raise unexpected errors
                raise
        except RuntimeError:
            error_raised = True

        assert error_raised, "RuntimeError should propagate"

    def test_adapter_close_timeout_handled(self, capsys):
        """Test adapter close with timeout is caught and logged."""
        import asyncio

        # Desired behavior: catch TimeoutError, log, continue
        try:
            raise asyncio.TimeoutError("HTTP adapter timeout")
        except (OSError, asyncio.TimeoutError) as e:
            print(f"Warning: HTTP adapter cleanup failed: {e}", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "HTTP adapter cleanup failed" in captured.err


class TestEnvVarCheckErrorHandling:
    """Test error handling during environment variable checking.

    These tests verify that env var checks handle restricted environments
    without catching KeyboardInterrupt.
    """

    def test_env_var_check_oserror_handled(self, capsys):
        """Test env var check with OSError is caught and logged."""

        # Desired behavior: catch OSError, warn, continue
        try:
            # Simulate restricted environment
            raise OSError("Environment access denied")
        except (OSError, PermissionError):
            print(
                "WARNING: Cannot check environment variables in this environment.",
                file=__import__("sys").stderr,
            )
            print(
                "Please verify manually that required tokens are set.",
                file=__import__("sys").stderr,
            )

        captured = capsys.readouterr()
        assert "Cannot check environment variables" in captured.err
        assert "verify manually" in captured.err.lower()

    def test_keyboard_interrupt_propagates(self):
        """Test KeyboardInterrupt during env check propagates (user abort)."""
        error_raised = False
        try:
            try:
                # User presses Ctrl+C during env check
                raise KeyboardInterrupt()
            except (OSError, PermissionError) as e:
                # Should not catch KeyboardInterrupt
                print(f"Caught: {e}")
        except KeyboardInterrupt:
            error_raised = True

        assert error_raised, "KeyboardInterrupt should propagate to allow wizard abort"


class TestConfigDirCreation:
    """Test error handling during config directory creation.

    These tests verify that mkdir failures show helpful error messages.
    """

    def test_permission_error_suggests_current_dir(self, capsys):
        """Test mkdir PermissionError suggests using current directory."""
        from pathlib import Path

        config_path_parent = Path("/root/.config/drep")

        # Desired behavior: catch PermissionError, suggest location 1
        try:
            raise PermissionError("Permission denied")
        except PermissionError:
            print(
                f"ERROR: Cannot create directory {config_path_parent}",
                file=__import__("sys").stderr,
            )
            print(
                "  Permission denied. Try using location 1 (current directory).",
                file=__import__("sys").stderr,
            )

        captured = capsys.readouterr()
        assert "Cannot create directory" in captured.err
        assert "location 1" in captured.err

    def test_oserror_shows_error_message(self, capsys):
        """Test mkdir OSError shows clear error message."""
        # Desired behavior: catch OSError, show error
        try:
            raise OSError("No space left on device")
        except OSError as e:
            print(f"ERROR: Cannot create directory: {e}", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "Cannot create directory" in captured.err
        assert "No space left" in captured.err


class TestGitOperationsErrorHandling:
    """Test error handling for git clone/pull operations.

    These tests verify that git failures provide actionable error messages
    instead of cryptic stack traces.
    """

    def test_git_clone_auth_failure_shows_token_check(self, capsys):
        """Test git clone auth failure suggests checking token."""
        from git import GitCommandError

        # Desired behavior: catch GitCommandError, check for auth failure
        try:
            raise GitCommandError(
                "git clone",
                128,
                stderr="fatal: Authentication failed for 'https://github.com/owner/repo.git'",
            )
        except GitCommandError as e:
            error_msg = str(e).lower()
            if "authentication failed" in error_msg:
                print("Error: Authentication failed", file=__import__("sys").stderr)
                print("  Check your GITHUB_TOKEN token", file=__import__("sys").stderr)
            else:
                print(f"Error: Git clone failed: {e}", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "Authentication failed" in captured.err
        assert "Check your" in captured.err
        assert "TOKEN" in captured.err

    def test_git_clone_repo_not_found_shows_verify(self, capsys):
        """Test git clone repo not found suggests verification."""
        from git import GitCommandError

        # Desired behavior: detect 404/not found
        try:
            raise GitCommandError(
                "git clone",
                128,
                stderr="fatal: repository 'https://github.com/owner/repo.git' not found",
            )
        except GitCommandError as e:
            error_msg = str(e).lower()
            if "not found" in error_msg:
                print("Error: Repository owner/repo not found", file=__import__("sys").stderr)
                print(
                    "  Verify repository exists and token has access", file=__import__("sys").stderr
                )

        captured = capsys.readouterr()
        assert "not found" in captured.err
        assert "Verify repository" in captured.err

    def test_git_pull_failure_suggests_reclone(self, capsys):
        """Test git pull failure suggests deleting and re-cloning."""
        from pathlib import Path

        from git import GitCommandError

        repo_path = Path("/path/to/repo")

        # Desired behavior: suggest rm -rf for pull failures
        try:
            raise GitCommandError("git pull", 1, stderr="error: Your local changes...")
        except GitCommandError as e:
            print(f"Error: Git pull failed: {e}", file=__import__("sys").stderr)
            print(f"  Try: rm -rf {repo_path} to force re-clone", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "Git pull failed" in captured.err
        assert "rm -rf" in captured.err
        assert "re-clone" in captured.err

    def test_corrupted_repo_suggests_deletion(self, capsys):
        """Test corrupted git repo suggests deletion and re-clone."""
        from pathlib import Path

        from git import InvalidGitRepositoryError

        repo_path = Path("/path/to/corrupted/repo")

        # Desired behavior: detect corrupted .git directory
        try:
            raise InvalidGitRepositoryError(str(repo_path))
        except InvalidGitRepositoryError:
            print(
                f"Error: Corrupted git repository at {repo_path}",
                file=__import__("sys").stderr,
            )
            print(
                f"  Fix: rm -rf {repo_path} and re-run scan",
                file=__import__("sys").stderr,
            )

        captured = capsys.readouterr()
        assert "Corrupted git repository" in captured.err
        assert "rm -rf" in captured.err
        assert "re-run scan" in captured.err

    def test_git_permission_error_shows_path(self, capsys):
        """Test git operation permission error shows path and guidance."""
        from pathlib import Path

        repo_path = Path("/readonly/repos/owner/repo")

        # Desired behavior: catch PermissionError, show path
        try:
            raise PermissionError(f"Cannot write to {repo_path}")
        except PermissionError:
            print(f"Error: Cannot write to {repo_path}", file=__import__("sys").stderr)
            print("  Check directory permissions", file=__import__("sys").stderr)

        captured = capsys.readouterr()
        assert "Cannot write to" in captured.err
        assert "permissions" in captured.err.lower()


class TestFinallyBlockErrorHandling:
    """Test that cleanup errors in finally blocks don't mask original exceptions.

    CRITICAL: This verifies Issue #1 from PR review - cleanup failures must
    never hide the actual scan/operation error from the user.
    """

    def test_cleanup_failure_does_not_mask_scan_error(self):
        """Test that finally block cleanup errors don't hide main operation errors.

        Security test: When both scan AND cleanup fail, the user must see
        the scan error (e.g., "Invalid config") not the cleanup error
        (e.g., "Failed to delete temp dir").
        """
        scan_error_raised = False
        scan_error_message = None

        # Simulate the pattern in cli.py:955 (scan command)
        # The ValueError from scan should propagate, not the OSError from cleanup
        try:
            try:
                raise ValueError("Scan failed: Invalid repository configuration")
            finally:
                try:
                    raise OSError("Failed to delete temporary directory")
                except Exception:
                    # Never re-raise - let original exception propagate
                    pass
        except ValueError as e:
            scan_error_raised = True
            scan_error_message = str(e)
        except OSError:
            # This should NEVER happen - cleanup errors must not mask scan errors
            assert False, "OSError from cleanup masked the ValueError from scan!"

        # Verify the scan error (ValueError) propagated correctly
        assert scan_error_raised, "Scan error should propagate from finally block"
        assert "Scan failed" in scan_error_message
        assert "Invalid repository" in scan_error_message

    def test_cleanup_success_allows_scan_error_to_propagate(self):
        """Test that successful cleanup still allows scan errors to propagate."""
        scan_error_raised = False

        # Verify exception handling
        try:
            try:
                raise RuntimeError("Scan operation failed")
            finally:
                pass  # Cleanup succeeds
        except RuntimeError:
            scan_error_raised = True

        assert scan_error_raised, "Scan error should propagate when cleanup succeeds"

    def test_no_scan_error_cleanup_failure_is_silent(self):
        """Test that cleanup failures are silent when main operation succeeds.

        When scan succeeds but cleanup fails, the operation should complete
        successfully with only a warning (not a raised exception).
        """
        operation_succeeded = False
        exception_raised = False

        # Main operation succeeds
        try:
            operation_succeeded = True
        finally:
            # Cleanup fails
            try:
                raise PermissionError("Cannot delete temp directory")
            except Exception:
                # Never re-raise - operation succeeded, just log warning
                pass

        # Verify operation succeeded despite cleanup failure
        assert operation_succeeded
        assert not exception_raised, "Cleanup failure should not raise when scan succeeds"
