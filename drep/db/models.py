"""SQLAlchemy database models."""

from datetime import UTC, datetime

from sqlalchemy import Column, DateTime, Index, Integer, String, UniqueConstraint
from sqlalchemy.orm import declarative_base

Base = declarative_base()


class RepositoryScan(Base):
    """Tracks last scan for incremental scanning."""

    __tablename__ = "repository_scans"

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    commit_sha = Column(String, nullable=False)
    scanned_at = Column(DateTime, default=lambda: datetime.now(UTC))

    # Index for faster lookups and uniqueness constraint
    __table_args__ = (
        Index("idx_owner_repo", "owner", "repo"),
        UniqueConstraint("owner", "repo", name="uq_owner_repo"),
    )


class FindingCache(Base):
    """Prevents duplicate issues (scoped per repository)."""

    __tablename__ = "finding_cache"

    id = Column(Integer, primary_key=True)
    owner = Column(String, nullable=False)
    repo = Column(String, nullable=False)
    file_path = Column(String, nullable=False)
    finding_hash = Column(String, nullable=False)  # Not globally unique
    issue_number = Column(Integer, nullable=True)
    created_at = Column(DateTime, default=lambda: datetime.now(UTC))

    # Deduplication is scoped to repository: same hash can exist across different repos
    __table_args__ = (
        Index("idx_finding_hash", "finding_hash"),
        UniqueConstraint("owner", "repo", "finding_hash", name="uq_owner_repo_hash"),
    )
