"""PR Review Analyzer - LLM-powered code review for pull requests."""

import asyncio
import logging
from dataclasses import dataclass
from typing import Any

from drep.adapters.base import BaseAdapter, ReviewAnchor
from drep.languages import registry
from drep.languages.prompts import build_review_rubric, describe_languages
from drep.llm.client import LLMClient
from drep.logging_utils import sanitize_secrets
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

    def is_added_line(self, file_path: str, line: int) -> bool:
        """Return True if the line is an added line in the reviewed diff.

        Lives here rather than on the analyzer: it reads nothing but this
        object's own added_lines index.
        """
        return line in self.added_lines.get(file_path, frozenset())


# Prompt template for PR reviews
PR_REVIEW_PROMPT = """You are a senior {languages} engineer reviewing a pull request.

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
   - Proper error handling?
   - Good variable/function names?
   - Language-specific concerns in this diff:
{language_rubric}

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

    def __init__(self, llm_client: LLMClient, adapter: BaseAdapter):
        """Initialize PR review analyzer.

        Args:
            llm_client: LLM client instance for code analysis
            adapter: Platform adapter (Gitea/GitHub/GitLab) for API calls
        """
        self.llm = llm_client
        self.adapter = adapter

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
        # The PR payload and the diff are independent fetches — issue them
        # together. The payload serves both the LLM prompt and the review
        # anchor, so it is fetched once rather than once per consumer.
        logger.info(f"Fetching PR #{pr_number} and diff from {owner}/{repo}")
        pr_data, diff_text = await asyncio.gather(
            self.adapter.get_pr(owner, repo, pr_number),
            self.adapter.get_pr_diff(owner, repo, pr_number),
        )

        # Derive the review anchor once; every inline comment of this review
        # will be posted against this single consistent snapshot
        anchor = self.adapter.anchor_from_pr(pr_data, owner, repo, pr_number)

        # Parse diff into hunks
        hunks = parse_diff(diff_text)
        logger.info(f"Parsed {len(hunks)} diff hunks")

        # Analyze with LLM
        repo_id = f"{owner}/{repo}"
        result = await self._analyze_diff_with_llm(
            pr_data, diff_text, hunks, repo_id, anchor.commit_sha
        )

        # Index added line numbers per file for validation during posting.
        # Accumulate in mutable sets and freeze once: rebuilding a frozenset per
        # hunk with `|` is quadratic in files with many hunks.
        line_index: dict[str, set[int]] = {}
        for hunk in hunks:
            line_index.setdefault(hunk.file_path, set()).update(
                line_num for line_num, _ in hunk.get_added_lines()
            )
        added_lines = {path: frozenset(lines) for path, lines in line_index.items()}

        logger.info(f"Review complete: {len(result.comments)} comments, approve={result.approve}")

        return PreparedReview(result=result, anchor=anchor, added_lines=added_lines)

    async def _analyze_diff_with_llm(
        self,
        pr_data: dict[str, Any],
        diff_text: str,
        hunks: list[DiffHunk],
        repo_id: str,
        commit_sha: str,
    ) -> PRReviewResult:
        """Send diff to LLM for review.

        Args:
            pr_data: PR details from the platform
            diff_text: Raw unified diff (already fetched; not re-serialized from hunks)
            hunks: Parsed diff hunks, used for the changed-file summary
            repo_id: Repository identifier (owner/repo), for per-repo rate limiting
            commit_sha: Head commit SHA, for cache invalidation

        Returns:
            PRReviewResult from LLM analysis
        """
        # Use the diff as fetched. It was previously re-serialized from the
        # parsed hunks — a second full pass producing near-identical text.
        diff_content = diff_text

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

        # The rubric follows the diff's languages, so a Go file in a mostly
        # Python PR is not reviewed against PEP 8.
        diff_languages = registry.detect_all(changed_files)

        # Prepare prompt
        prompt = PR_REVIEW_PROMPT.format(
            languages=describe_languages(diff_languages) or "software",
            language_rubric=build_review_rubric(diff_languages),
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
            repo_id=repo_id,
            commit_sha=commit_sha,
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
        await self.adapter.create_pr_comment(
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
            if not prepared.is_added_line(comment.file_path, comment.line):
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
                await self.adapter.create_pr_review_comment(
                    anchor=anchor,
                    file_path=comment.file_path,
                    line=comment.line,
                    body=comment_body,
                )
                posted_count += 1
            except ValueError as e:
                error_msg = sanitize_secrets(str(e))
                logger.error(
                    f"Failed to post comment for {comment.file_path}:{comment.line}: {error_msg}"
                )
                skipped_count += 1

        logger.info(f"Review posted: {posted_count} comments posted, {skipped_count} skipped")
