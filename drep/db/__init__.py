"""Database layer."""

from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker

from drep.db.models import Base


def init_database(database_url: str):
    """Initialize database and return session.

    Args:
        database_url: SQLAlchemy database URL (e.g., sqlite:///./drep.db)

    Returns:
        SQLAlchemy Session object
    """
    engine = create_engine(database_url)
    Base.metadata.create_all(engine)
    Session = sessionmaker(bind=engine)
    return Session()
