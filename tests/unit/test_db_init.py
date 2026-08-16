"""Tests for database initialization."""

from sqlalchemy import inspect


def test_init_database_creates_tables(tmp_path):
    """Test that init_database creates all required tables."""
    from drep.db import init_database

    db_path = tmp_path / "test.db"
    database_url = f"sqlite:///{db_path}"

    session = init_database(database_url)

    # Check that tables exist
    inspector = inspect(session.bind)
    tables = inspector.get_table_names()

    assert "repository_scans" in tables
    assert "finding_cache" in tables

    session.close()


def test_init_database_returns_session(tmp_path):
    """Test that init_database returns a valid session."""
    from sqlalchemy.orm import Session

    from drep.db import init_database

    db_path = tmp_path / "test.db"
    database_url = f"sqlite:///{db_path}"

    session = init_database(database_url)

    assert isinstance(session, Session)
    assert session.bind is not None

    session.close()


def test_init_database_creates_file(tmp_path):
    """Test that SQLite database file is created."""
    from drep.db import init_database

    db_path = tmp_path / "test.db"
    database_url = f"sqlite:///{db_path}"

    assert not db_path.exists()

    session = init_database(database_url)

    assert db_path.exists()

    session.close()


def test_init_database_idempotent(tmp_path):
    """Test that init_database can be called multiple times safely."""
    from drep.db import init_database

    db_path = tmp_path / "test.db"
    database_url = f"sqlite:///{db_path}"

    # First init
    session1 = init_database(database_url)
    session1.close()

    # Second init - should not error
    session2 = init_database(database_url)

    inspector = inspect(session2.bind)
    tables = inspector.get_table_names()

    assert "repository_scans" in tables
    assert "finding_cache" in tables

    session2.close()


def test_init_database_memory():
    """Test that init_database works with in-memory database."""
    from drep.db import init_database

    database_url = "sqlite:///:memory:"

    session = init_database(database_url)

    inspector = inspect(session.bind)
    tables = inspector.get_table_names()

    assert "repository_scans" in tables
    assert "finding_cache" in tables

    session.close()


def test_init_database_session_can_query(tmp_path):
    """Test that returned session can perform queries."""
    from drep.db import init_database
    from drep.db.models import RepositoryScan

    db_path = tmp_path / "test.db"
    database_url = f"sqlite:///{db_path}"

    session = init_database(database_url)

    # Add a record
    scan = RepositoryScan(owner="steve", repo="drep", commit_sha="abc123")
    session.add(scan)
    session.commit()

    # Query it back
    result = session.query(RepositoryScan).filter_by(owner="steve").first()

    assert result is not None
    assert result.repo == "drep"

    session.close()


def test_init_database_with_absolute_path(tmp_path):
    """Test init_database with absolute file path."""
    from drep.db import init_database

    db_path = tmp_path / "subdir" / "test.db"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    database_url = f"sqlite:///{db_path}"

    session = init_database(database_url)

    assert db_path.exists()

    session.close()


class TestFindingCacheIssueNumberMigration:
    """C20: issue_number must be NOT NULL — NULL meant permanent silent suppression."""

    def _make_legacy_db(self, db_path):
        """Create a legacy finding_cache table with a nullable issue_number."""
        import sqlite3

        conn = sqlite3.connect(db_path)
        conn.executescript(
            """
            CREATE TABLE finding_cache (
                id INTEGER PRIMARY KEY,
                owner VARCHAR NOT NULL,
                repo VARCHAR NOT NULL,
                file_path VARCHAR NOT NULL,
                finding_hash VARCHAR NOT NULL,
                issue_number INTEGER,
                created_at DATETIME
            );
            CREATE INDEX idx_finding_hash ON finding_cache (finding_hash);
            INSERT INTO finding_cache (owner, repo, file_path, finding_hash, issue_number)
                VALUES ('o', 'r', 'f.py', 'hash-ok', 7);
            INSERT INTO finding_cache (owner, repo, file_path, finding_hash, issue_number)
                VALUES ('o', 'r', 'g.py', 'hash-null', NULL);
            """
        )
        conn.commit()
        conn.close()

    def test_migration_makes_issue_number_not_null(self, tmp_path):
        from drep.db import init_database

        db_path = tmp_path / "legacy.db"
        self._make_legacy_db(db_path)

        session = init_database(f"sqlite:///{db_path}")
        session.close()

        import sqlite3

        conn = sqlite3.connect(db_path)
        cols = {row[1]: row[3] for row in conn.execute("PRAGMA table_info(finding_cache)")}
        rows = conn.execute("SELECT finding_hash, issue_number FROM finding_cache").fetchall()
        conn.close()

        assert cols["issue_number"] == 1  # NOT NULL enforced
        # NULL row reconciled away; valid row preserved with its issue number
        assert rows == [("hash-ok", 7)]

    def test_new_insert_with_null_issue_number_rejected(self, tmp_path):
        import pytest
        from sqlalchemy.exc import IntegrityError

        from drep.db import init_database
        from drep.db.models import FindingCache

        db_path = tmp_path / "legacy.db"
        self._make_legacy_db(db_path)

        session = init_database(f"sqlite:///{db_path}")
        session.add(
            FindingCache(
                owner="o", repo="r", file_path="h.py", finding_hash="h2", issue_number=None
            )
        )
        with pytest.raises(IntegrityError):
            session.commit()
        session.rollback()
        session.close()
