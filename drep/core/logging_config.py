"""Structured logging configuration for production."""

import json
import logging
import sys
from datetime import datetime
from pathlib import Path
from typing import Optional


class StructuredFormatter(logging.Formatter):
    """Format logs as structured JSON for production systems.

    Output format:
    {
        "timestamp": "2025-01-01T12:00:00.123456",
        "level": "INFO",
        "logger": "drep.llm.client",
        "message": "LLM request successful",
        "repo_id": "owner/repo",
        "file_path": "src/main.py",
        "analyzer": "code_quality",
        "exception": "Traceback..."  # if present
    }
    """

    def format(self, record: logging.LogRecord) -> str:
        """Format log record as JSON.

        Args:
            record: Log record to format

        Returns:
            JSON string
        """
        log_data = {
            "timestamp": datetime.now().isoformat(),
            "level": record.levelname,
            "logger": record.name,
            "message": record.getMessage(),
        }

        # Add context fields if present
        context_fields = [
            "repo_id",
            "file_path",
            "analyzer",
            "commit_sha",
            "tokens_used",
            "latency_ms",
        ]

        for field in context_fields:
            if hasattr(record, field):
                log_data[field] = getattr(record, field)

        # Add exception info if present
        if record.exc_info:
            log_data["exception"] = self.formatException(record.exc_info)

        return json.dumps(log_data)


def setup_logging(
    level: str = "INFO",
    structured: bool = False,
    log_file: Optional[Path] = None,
):
    """Configure logging for drep.

    Args:
        level: Log level (DEBUG, INFO, WARNING, ERROR)
        structured: Use JSON structured logging (for production)
        log_file: Optional file to write logs to
    """
    # Get root logger
    root_logger = logging.getLogger()
    root_logger.setLevel(getattr(logging, level.upper()))

    # Remove existing handlers
    root_logger.handlers.clear()

    # Console handler
    console_handler = logging.StreamHandler(sys.stdout)

    if structured:
        # JSON formatting for production
        console_handler.setFormatter(StructuredFormatter())
    else:
        # Human-readable formatting for development
        formatter = logging.Formatter(
            "%(asctime)s - %(name)s - %(levelname)s - %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S",
        )
        console_handler.setFormatter(formatter)

    root_logger.addHandler(console_handler)

    # File handler (if specified)
    if log_file:
        log_file = Path(log_file)
        log_file.parent.mkdir(parents=True, exist_ok=True)

        file_handler = logging.FileHandler(log_file)

        if structured:
            file_handler.setFormatter(StructuredFormatter())
        else:
            file_handler.setFormatter(formatter)

        root_logger.addHandler(file_handler)

    # Set levels for noisy libraries
    logging.getLogger("httpx").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)
    logging.getLogger("git").setLevel(logging.WARNING)
