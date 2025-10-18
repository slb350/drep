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
