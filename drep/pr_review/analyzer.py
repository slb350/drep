"""PR Review Analyzer - LLM-powered code review for pull requests."""

import logging
import re
from dataclasses import dataclass
from typing import Any

from drep.adapters.base import BaseAdapter, ReviewAnchor
from drep.llm.client import LLMClient
from drep.models.pr_review_findings import PRReviewResult
from drep.pr_review.diff_parser import DiffHunk, parse_diff

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class PreparedReview:
    """Immutable, self-contained result of a PR review.

    Bundles everything ``post_review`` needs, so a prepared review can be
    posted later (or a different review started) without hidden analyzer
    state going stale: the LLM result, the platform anchor the diff was
    taken from, and the per-file set of added line numbers for validation.

    Attributes:
        result: Structured review result from the LLM
        anchor: Review anchor (head SHA / positioning data) for the reviewed diff
        added_lines: Mapping of file path -> added line numbers in the diff
    """

    result: PRReviewResult
    anchor: ReviewAnchor
    added_lines: dict[str, frozenset[int]]


# Prompt template for PR reviews
PR_REVIEW_PROMPT = """You are a senior Python engineer reviewing a pull request.

**PR Details:**
Title: {pr_title}
Description: {pr_description}
Author: {pr_author}
Base: {base_branch} → Head: {head_branch}

**Changed Files:**
{diff_summary}

**Review Focus:**

1. **Correctness**
   - Does the code do what the PR claims?
   - Are there logic errors or bugs?
   - Edge cases handled?

2. **Best Practices**
   - Follows Python conventions (PEP 8)?
   - Proper error handling?
   - Type hints present?
   - Good variable/function names?

3. **Testing**
   - Are tests included?
   - Are they comprehensive?

4. **Documentation**
   - Docstrings added/updated?
   - Comments explain "why" not "what"?

5. **Security & Performance**
   - Any vulnerabilities?
   - Performance concerns?
   - Resource leaks?

**Diff:**
```diff
{diff_content}
```

**Instructions:**
- Be constructive, not just critical
- Suggest specific improvements with code examples
- Highlight good changes too
- Consider the PR's stated goal
- ONLY comment on CHANGED lines (lines starting with +), not unchanged code

**Output Format:**
Return JSON only:
{{
  "comments": [
    {{
      "file_path": "src/module.py",
      "line": 42,
      "severity": "suggestion",
      "comment": "Consider adding error handling here for X...",
      "suggestion": "try:\\n    ...\\nexcept ValueError:\\n    ..."
    }}
  ],
  "summary": "Overall assessment of PR... Main points and recommendations",
  "approve": true,
  "concerns": []
}}

**Severity Levels (MUST use exactly one of these):**
- "info": Informational note or minor style issue
- "suggestion": Suggested improvement (not required)
- "warning": Potential issue that should be addressed
- "critical": Serious bug, security issue, or blocker that MUST be fixed
"""


class PRReviewAnalyzer:
    """Analyzes PR diffs using LLM and posts review comments."""

    def __init__(self, llm_client: LLMClient, gitea_adapter: BaseAdapter):
        """Initialize PR review analyzer.

        Args:
            llm_client: LLM client instance for code analysis
            gitea_adapter: Platform adapter (Gitea/GitHub/GitLab) for API calls
        """
        self.llm = llm_client
        self.gitea = gitea_adapter

    async def review_pr(
        self,
        owner: str,
        repo: str,
        pr_number: int,
    ) -> PreparedReview:
        """Review a pull request end-to-end.

        Workflow:
        1. Fetch review anchor (head SHA / platform positioning data) once
        2. Fetch PR diff
        3. Parse diff into hunks
        4. Truncate if too large (> 20k chars)
        5. Analyze with LLM
        6. Return immutable PreparedReview binding result, anchor, and added lines

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: PR number

        Returns:
            PreparedReview (pass to post_review to publish)

        Raises:
            ValueError: If PR not found or other platform errors
            Exception: If LLM analysis fails
        """
        # Fetch the review anchor once; every inline comment of this review
        # will be posted against this single consistent snapshot
        anchor = await self.gitea.get_review_anchor(owner, repo, pr_number)

        # Fetch PR details for the LLM prompt
        logger.info(f"Fetching PR #{pr_number} from {owner}/{repo}")
        pr_data = await self.gitea.get_pr(owner, repo, pr_number)

        # Fetch PR diff
        logger.info(f"Fetching diff for PR #{pr_number}")
        diff_text = await self.gitea.get_pr_diff(owner, repo, pr_number)

        # Parse diff into hunks
        hunks = parse_diff(diff_text)
        logger.info(f"Parsed {len(hunks)} diff hunks")

        # Analyze with LLM
        repo_id = f"{owner}/{repo}"
        result = await self._analyze_diff_with_llm(pr_data, hunks, repo_id)

        # Index added line numbers per file for validation during posting
        added_lines: dict[str, frozenset[int]] = {}
        for hunk in hunks:
            hunk_lines = {line_num for line_num, _ in hunk.get_added_lines()}
            added_lines[hunk.file_path] = added_lines.get(hunk.file_path, frozenset()) | hunk_lines

        logger.info(f"Review complete: {len(result.comments)} comments, approve={result.approve}")

        return PreparedReview(result=result, anchor=anchor, added_lines=added_lines)

    @staticmethod
    def _is_valid_comment_line(prepared: PreparedReview, file_path: str, line: int) -> bool:
        """Validate that a line number corresponds to an added line in the reviewed diff."""
        return line in prepared.added_lines.get(file_path, frozenset())

    async def _analyze_diff_with_llm(
        self,
        pr_data: dict[str, Any],
        hunks: list[DiffHunk],
        repo_id: str,
    ) -> PRReviewResult:
        """Send diff to LLM for review.

        Args:
            pr_data: PR details from Gitea
            hunks: Parsed diff hunks
            repo_id: Repository identifier (owner/repo)

        Returns:
            PRReviewResult from LLM analysis
        """
        # Reconstruct diff from hunks
        diff_lines = []
        for hunk in hunks:
            diff_lines.append(f"diff --git a/{hunk.file_path} b/{hunk.file_path}")
            diff_lines.append(
                f"@@ -{hunk.old_start},{hunk.old_count} +{hunk.new_start},{hunk.new_count} @@"
            )
            diff_lines.extend(hunk.lines)

        diff_content = "\n".join(diff_lines)

        # Truncate if too large (> 20k chars)
        max_diff_size = 20000
        if len(diff_content) > max_diff_size:
            logger.warning(
                f"Diff too large ({len(diff_content)} chars), truncating to {max_diff_size}"
            )

            # Keep first 15k and last 5k
            first_part = diff_content[:15000]
            last_part = diff_content[-5000:]
            omitted = len(diff_content) - 20000

            diff_content = (
                f"{first_part}\n\n... [TRUNCATED: {omitted} characters omitted] ...\n\n{last_part}"
            )

        # Build diff summary (list of changed files)
        changed_files = list({hunk.file_path for hunk in hunks})
        diff_summary = "\n".join(f"- {f}" for f in changed_files)

        # Prepare prompt
        prompt = PR_REVIEW_PROMPT.format(
            pr_title=pr_data.get("title", ""),
            pr_description=pr_data.get("body") or "(no description)",
            pr_author=pr_data.get("user", {}).get("login", "unknown"),
            base_branch=pr_data.get("base", {}).get("ref", "main"),
            head_branch=pr_data.get("head", {}).get("ref", "unknown"),
            diff_summary=diff_summary or "(no files changed)",
            diff_content=diff_content,
        )

        # Call LLM
        logger.info(f"Analyzing diff with LLM ({len(prompt)} chars)")
        result_json = await self.llm.analyze_code_json(
            system_prompt=prompt,
            code="",  # Diff is in prompt
            schema=PRReviewResult,
            analyzer="pr_review",
        )

        # Convert to Pydantic model
        return PRReviewResult(**result_json)

    async def post_review(self, prepared: PreparedReview) -> None:
        """Post a prepared review to its PR.

        Args:
            prepared: The PreparedReview to post (from review_pr)
        """
        anchor = prepared.anchor
        owner, repo, pr_number = anchor.owner, anchor.repo, anchor.pr_number
        result = prepared.result
        # Post summary comment
        summary_body = f"""## 🤖 drep AI Code Review

{result.summary}

**Recommendation:** {"✅ Approve" if result.approve else "🔍 Needs Changes"}

{"**Concerns:**" if result.concerns else ""}
{chr(10).join(f"- {concern}" for concern in result.concerns)}

---
*Generated by drep using {self.llm.model}*
"""

        logger.info(f"Posting summary comment to PR #{pr_number}")
        await self.gitea.create_pr_comment(
            owner=owner,
            repo=repo,
            pr_number=pr_number,
            body=summary_body,
        )

        # Post inline comments with validation
        logger.info(f"Posting {len(result.comments)} inline comments")
        posted_count = 0
        skipped_count = 0

        for comment in result.comments:
            # Validate that the line number exists in the reviewed diff
            if not self._is_valid_comment_line(prepared, comment.file_path, comment.line):
                logger.warning(
                    f"Skipping comment for {comment.file_path}:{comment.line} - "
                    f"line not found in diff (LLM may have miscounted or diff was truncated)"
                )
                skipped_count += 1
                continue

            # Format comment with severity
            severity_emoji = {
                "info": "ℹ️",
                "suggestion": "💡",
                "warning": "⚠️",
                "critical": "🚨",
            }
            emoji = severity_emoji.get(comment.severity, "")

            comment_body = f"{emoji} **{comment.severity.upper()}**: {comment.comment}"

            if comment.suggestion:
                comment_body += f"\n\n**Suggested fix:**\n```python\n{comment.suggestion}\n```"

            try:
                await self.gitea.create_pr_review_comment(
                    anchor=anchor,
                    file_path=comment.file_path,
                    line=comment.line,
                    body=comment_body,
                )
                posted_count += 1
            except ValueError as e:
                # Sanitize error message to avoid logging tokens in URLs
                error_msg = str(e)
                error_msg = re.sub(
                    r"(token|api_?key|password|secret)=[^&\s]+",
                    r"\1=***",
                    error_msg,
                    flags=re.IGNORECASE,
                )
                error_msg = re.sub(r"://[^:]+:[^@]+@", r"://***:***@", error_msg)
                logger.error(
                    f"Failed to post comment for {comment.file_path}:{comment.line}: {error_msg}"
                )
                skipped_count += 1

        logger.info(f"Review posted: {posted_count} comments posted, {skipped_count} skipped")
