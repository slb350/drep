"""Issue manager for deduplication and creation."""

import hashlib
from typing import List

from drep.models.findings import Finding


class IssueManager:
    """Manages issue creation with deduplication."""

    def __init__(self, adapter, db_session):
        """Initialize IssueManager.

        Args:
            adapter: Platform adapter (e.g., GiteaAdapter) for API calls
            db_session: SQLAlchemy database session for caching
        """
        self.adapter = adapter
        self.db = db_session

    def _generate_hash(self, finding: Finding) -> str:
        """Generate unique hash for finding.

        Hash is based on: file_path, line, type, message
        This ensures the same issue in the same location is deduplicated.

        Args:
            finding: Finding object to hash

        Returns:
            MD5 hex digest (32 characters)
        """
        content = f"{finding.file_path}:{finding.line}:{finding.type}:{finding.message}"
        return hashlib.md5(content.encode()).hexdigest()

    def _generate_issue_body(self, finding: Finding) -> str:
        """Generate markdown issue body for a finding.

        Args:
            finding: Finding object to format

        Returns:
            Markdown-formatted issue body
        """
        body = f"""## Finding

**Type:** {finding.type}
**Severity:** {finding.severity}
**File:** {finding.file_path}
**Line:** {finding.line}

**Issue:** {finding.message}
"""

        # Only include suggestion if present
        if finding.suggestion:
            body += f"\n**Suggestion:** {finding.suggestion}\n"

        # Add footer with attribution
        body += "\n---\n*Automatically created by [drep](https://github.com/stephenbrandon/drep)*"

        return body

    async def create_issues_for_findings(self, owner: str, repo: str, findings: List[Finding]):
        """Create issues for findings, skipping duplicates.

        Args:
            owner: Repository owner
            repo: Repository name
            findings: List of Finding objects to create issues for
        """
        from drep.db.models import FindingCache

        for finding in findings:
            # Generate hash for deduplication
            finding_hash = self._generate_hash(finding)

            # Check if we've already created this issue (scoped to this repository)
            existing = (
                self.db.query(FindingCache)
                .filter_by(owner=owner, repo=repo, finding_hash=finding_hash)
                .first()
            )

            if existing:
                # Skip duplicate - already created issue for this finding in this repo
                continue

            # Create issue via adapter
            title = f"[drep] {finding.type}: {finding.file_path}:{finding.line}"
            body = self._generate_issue_body(finding)

            issue_number = await self.adapter.create_issue(
                owner=owner,
                repo=repo,
                title=title,
                body=body,
                labels=["documentation", "automated"],
            )

            # Cache this finding to prevent duplicates
            cache_entry = FindingCache(
                owner=owner,
                repo=repo,
                file_path=finding.file_path,
                finding_hash=finding_hash,
                issue_number=issue_number,
            )
            self.db.add(cache_entry)
            self.db.commit()
