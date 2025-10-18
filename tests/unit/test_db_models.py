"""Tests for database models."""

from datetime import datetime

import pytest
from sqlalchemy import create_engine, inspect
from sqlalchemy.orm import sessionmaker


@pytest.fixture
def engine():
    """Create an in-memory SQLite database for testing."""
    return create_engine("sqlite:///:memory:")


@pytest.fixture
def session(engine):
    """Create a database session for testing."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    Session = sessionmaker(bind=engine)
    return Session()


def test_repository_scan_table_exists(engine):
    """Test that repository_scans table is created."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)

    assert "repository_scans" in inspector.get_table_names()


def test_repository_scan_columns(engine):
    """Test repository_scans table has correct columns."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)
    columns = {col["name"]: col for col in inspector.get_columns("repository_scans")}

    assert "id" in columns
    assert "owner" in columns
    assert "repo" in columns
    assert "commit_sha" in columns
    assert "scanned_at" in columns


def test_repository_scan_create(session):
    """Test creating a RepositoryScan record."""
    from drep.db.models import RepositoryScan

    scan = RepositoryScan(owner="steve", repo="drep", commit_sha="abc123def456")

    session.add(scan)
    session.commit()

    assert scan.id is not None
    assert scan.owner == "steve"
    assert scan.repo == "drep"
    assert scan.commit_sha == "abc123def456"
    assert isinstance(scan.scanned_at, datetime)


def test_repository_scan_index_exists(engine):
    """Test that idx_owner_repo index exists."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)
    indexes = inspector.get_indexes("repository_scans")

    # Check for index on owner and repo
    index_names = [idx["name"] for idx in indexes]
    assert "idx_owner_repo" in index_names


def test_finding_cache_table_exists(engine):
    """Test that finding_cache table is created."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)

    assert "finding_cache" in inspector.get_table_names()


def test_finding_cache_columns(engine):
    """Test finding_cache table has correct columns."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)
    columns = {col["name"]: col for col in inspector.get_columns("finding_cache")}

    assert "id" in columns
    assert "owner" in columns
    assert "repo" in columns
    assert "file_path" in columns
    assert "finding_hash" in columns
    assert "issue_number" in columns
    assert "created_at" in columns


def test_finding_cache_create(session):
    """Test creating a FindingCache record."""
    from drep.db.models import FindingCache

    cache = FindingCache(
        owner="steve", repo="drep", file_path="test.py", finding_hash="abc123", issue_number=42
    )

    session.add(cache)
    session.commit()

    assert cache.id is not None
    assert cache.owner == "steve"
    assert cache.repo == "drep"
    assert cache.file_path == "test.py"
    assert cache.finding_hash == "abc123"
    assert cache.issue_number == 42
    assert isinstance(cache.created_at, datetime)


def test_finding_cache_hash_unique_per_repository(session):
    """Test that finding_hash must be unique within a repository (scoped deduplication)."""
    from sqlalchemy.exc import IntegrityError

    from drep.db.models import FindingCache

    # Create first entry in steve/drep
    cache1 = FindingCache(
        owner="steve", repo="drep", file_path="test.py", finding_hash="abc123", issue_number=42
    )
    session.add(cache1)
    session.commit()

    # Try to create second entry with same hash in SAME repository - should fail
    cache2 = FindingCache(
        owner="steve",
        repo="drep",  # Same repo
        file_path="test2.py",
        finding_hash="abc123",  # Same hash
        issue_number=43,
    )
    session.add(cache2)

    with pytest.raises(IntegrityError):
        session.commit()


def test_finding_cache_hash_allowed_across_repositories(session):
    """Test that same finding_hash is allowed in different repositories (CRITICAL BUG FIX).

    This prevents cross-repository collision where repo B's findings are skipped
    because repo A already has the same hash.
    """
    from drep.db.models import FindingCache

    # Create first entry in alice/repo-a
    cache1 = FindingCache(
        owner="alice",
        repo="repo-a",
        file_path="README.md",
        finding_hash="same_hash",
        issue_number=42,
    )
    session.add(cache1)
    session.commit()

    # Create second entry with same hash in DIFFERENT repository - should succeed
    cache2 = FindingCache(
        owner="bob",
        repo="repo-b",  # Different repo
        file_path="README.md",
        finding_hash="same_hash",  # Same hash - this is OK across different repos
        issue_number=99,
    )
    session.add(cache2)
    session.commit()  # Should NOT raise IntegrityError

    # Verify both entries exist
    all_caches = session.query(FindingCache).filter_by(finding_hash="same_hash").all()
    assert len(all_caches) == 2
    assert {(c.owner, c.repo) for c in all_caches} == {("alice", "repo-a"), ("bob", "repo-b")}


def test_finding_cache_index_exists(engine):
    """Test that idx_finding_hash index exists."""
    from drep.db.models import Base

    Base.metadata.create_all(engine)
    inspector = inspect(engine)
    indexes = inspector.get_indexes("finding_cache")

    # Check for index on finding_hash
    index_names = [idx["name"] for idx in indexes]
    assert "idx_finding_hash" in index_names


def test_finding_cache_nullable_issue_number(session):
    """Test that issue_number can be null."""
    from drep.db.models import FindingCache

    cache = FindingCache(
        owner="steve",
        repo="drep",
        file_path="test.py",
        finding_hash="abc123",
        issue_number=None,  # Explicitly None
    )

    session.add(cache)
    session.commit()

    assert cache.issue_number is None


def test_repository_scan_query_by_owner_repo(session):
    """Test querying repository scans by owner and repo."""
    from drep.db.models import RepositoryScan

    # Create multiple scans
    scan1 = RepositoryScan(owner="steve", repo="drep", commit_sha="abc123")
    scan2 = RepositoryScan(owner="steve", repo="other", commit_sha="def456")
    scan3 = RepositoryScan(owner="john", repo="drep", commit_sha="ghi789")

    session.add_all([scan1, scan2, scan3])
    session.commit()

    # Query for steve/drep
    result = session.query(RepositoryScan).filter_by(owner="steve", repo="drep").first()

    assert result.commit_sha == "abc123"


def test_finding_cache_query_by_hash(session):
    """Test querying finding cache by hash."""
    from drep.db.models import FindingCache

    cache1 = FindingCache(
        owner="steve", repo="drep", file_path="test.py", finding_hash="abc123", issue_number=1
    )
    cache2 = FindingCache(
        owner="steve", repo="drep", file_path="test2.py", finding_hash="def456", issue_number=2
    )

    session.add_all([cache1, cache2])
    session.commit()

    # Query by hash
    result = session.query(FindingCache).filter_by(finding_hash="abc123").first()

    assert result.file_path == "test.py"
    assert result.issue_number == 1


def test_repository_scan_owner_repo_unique(session):
    """Test that (owner, repo) combination must be unique."""
    from sqlalchemy.exc import IntegrityError

    from drep.db.models import RepositoryScan

    # Create first scan
    scan1 = RepositoryScan(owner="steve", repo="drep", commit_sha="abc123")
    session.add(scan1)
    session.commit()

    # Try to create second scan with same owner/repo
    scan2 = RepositoryScan(owner="steve", repo="drep", commit_sha="def456")
    session.add(scan2)

    # Should raise IntegrityError due to unique constraint
    with pytest.raises(IntegrityError):
        session.commit()
