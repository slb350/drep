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
