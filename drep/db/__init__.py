"""Database layer."""

import logging

from sqlalchemy import create_engine
from sqlalchemy.engine import Engine
from sqlalchemy.orm import sessionmaker

from drep.db.models import Base

logger = logging.getLogger(__name__)


def _migrate_finding_cache_issue_number(engine: Engine) -> None:
    """Migrate legacy finding_cache tables: issue_number nullable -> NOT NULL.

    SQLite's create_all cannot alter existing tables. Rows with NULL
    issue_number (unreachable via current production code) are deleted:
    they permanently suppressed their findings with no issue filed, so
    dropping them lets the findings be re-reported.
    """
    if engine.dialect.name != "sqlite":
        return

    with engine.begin() as conn:
        cols = conn.exec_driver_sql("PRAGMA table_info(finding_cache)").fetchall()
        issue_col = next((c for c in cols if c[1] == "issue_number"), None)
        if issue_col is None or issue_col[3]:
            # Table absent (create_all just made it current) or already NOT NULL
            return

        logger.info("Migrating finding_cache.issue_number to NOT NULL")
        conn.exec_driver_sql(
            """
            CREATE TABLE finding_cache_new (
                id INTEGER PRIMARY KEY,
                owner VARCHAR NOT NULL,
                repo VARCHAR NOT NULL,
                file_path VARCHAR NOT NULL,
                finding_hash VARCHAR NOT NULL,
                issue_number INTEGER NOT NULL,
                created_at DATETIME,
                CONSTRAINT uq_owner_repo_hash UNIQUE (owner, repo, finding_hash)
            )
            """
        )
        conn.exec_driver_sql(
            """
            INSERT INTO finding_cache_new
                (id, owner, repo, file_path, finding_hash, issue_number, created_at)
            SELECT id, owner, repo, file_path, finding_hash, issue_number, created_at
            FROM finding_cache
            WHERE issue_number IS NOT NULL
            """
        )
        conn.exec_driver_sql("DROP TABLE finding_cache")
        conn.exec_driver_sql("ALTER TABLE finding_cache_new RENAME TO finding_cache")
        conn.exec_driver_sql("CREATE INDEX idx_finding_hash ON finding_cache (finding_hash)")


def init_database(database_url: str):
    """Initialize database and return session.

    Args:
        database_url: SQLAlchemy database URL (e.g., sqlite:///./drep.db)

    Returns:
        SQLAlchemy Session object
    """
    engine = create_engine(database_url)
    Base.metadata.create_all(engine)
    _migrate_finding_cache_issue_number(engine)
    Session = sessionmaker(bind=engine)
    return Session()
