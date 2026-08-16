"""Database layer."""

import logging
from typing import cast

from sqlalchemy import MetaData, Table, create_engine
from sqlalchemy.engine import Engine
from sqlalchemy.orm import Session, sessionmaker
from sqlalchemy.schema import CreateTable

from drep.db.models import Base, FindingCache

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

    # Probe on a read-only connection: engine.begin() opens a write transaction,
    # and init_database() runs on every scan and check.
    with engine.connect() as conn:
        cols = conn.exec_driver_sql("PRAGMA table_info(finding_cache)").fetchall()

    issue_col = next((c for c in cols if c[1] == "issue_number"), None)
    if issue_col is None or issue_col[3]:
        # Table absent (create_all just made it current) or already NOT NULL
        return

    logger.info("Migrating finding_cache.issue_number to NOT NULL")

    # Build the replacement table from the model rather than hand-written DDL,
    # so a future column change cannot leave this migration silently recreating
    # a stale schema that disagrees with FindingCache.
    # __table__ is typed as FromClause on the declarative base; it is a Table.
    table = cast(Table, FindingCache.__table__)
    staging = table.to_metadata(MetaData(), name="finding_cache_new")
    column_list = ", ".join(column.name for column in table.columns)

    with engine.begin() as conn:
        conn.execute(CreateTable(staging))
        conn.exec_driver_sql(
            f"INSERT INTO finding_cache_new ({column_list}) "
            f"SELECT {column_list} FROM finding_cache WHERE issue_number IS NOT NULL"
        )
        conn.exec_driver_sql("DROP TABLE finding_cache")
        conn.exec_driver_sql("ALTER TABLE finding_cache_new RENAME TO finding_cache")
        # Indexes are not part of CreateTable; recreate them from the model now
        # that the table carries its final name.
        for index in table.indexes:
            index.create(bind=conn)


def init_database(database_url: str) -> Session:
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
