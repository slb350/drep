"""PR Review Analyzer - LLM-powered code review for pull requests."""

import logging
from typing import Any, Dict, List

from drep.adapters.gitea import GiteaAdapter
from drep.llm.client import LLMClient
from drep.models.pr_review_findings import PRReviewResult
from drep.pr_review.diff_parser import DiffHunk, parse_diff

logger = logging.getLogger(__name__)


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
"""


class PRReviewAnalyzer:
    """Analyzes PR diffs using LLM and posts review comments."""

    def __init__(self, llm_client: LLMClient, gitea_adapter: GiteaAdapter):
        """Initialize PR review analyzer.

        Args:
            llm_client: LLM client instance for code analysis
            gitea_adapter: Gitea adapter for API calls
        """
        self.llm = llm_client
        self.gitea = gitea_adapter
        self._current_hunks: List[DiffHunk] = []  # Store hunks for validation

    async def review_pr(
        self,
        owner: str,
        repo: str,
        pr_number: int,
    ) -> PRReviewResult:
        """Review a pull request end-to-end.

        Workflow:
        1. Fetch PR details
        2. Fetch PR diff
        3. Parse diff into hunks
        4. Truncate if too large (> 20k chars)
        5. Analyze with LLM
        6. Return structured result

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: PR number

        Returns:
            PRReviewResult with comments and summary

        Raises:
            ValueError: If PR not found or other Gitea errors
            Exception: If LLM analysis fails
        """
        # Fetch PR details
        logger.info(f"Fetching PR #{pr_number} from {owner}/{repo}")
        pr_data = await self.gitea.get_pr(owner, repo, pr_number)

        # Fetch PR diff
        logger.info(f"Fetching diff for PR #{pr_number}")
        diff_text = await self.gitea.get_pr_diff(owner, repo, pr_number)

        # Parse diff into hunks
        hunks = parse_diff(diff_text)
        logger.info(f"Parsed {len(hunks)} diff hunks")

        # Store hunks for line number validation during posting
        self._current_hunks = hunks

        # Analyze with LLM
        repo_id = f"{owner}/{repo}"
        result = await self._analyze_diff_with_llm(pr_data, hunks, repo_id)

        logger.info(
            f"Review complete: {len(result.comments)} comments, " f"approve={result.approve}"
        )

        return result

    def _is_valid_comment_line(self, file_path: str, line: int) -> bool:
        """Validate that a line number corresponds to an added line in the diff.

        Args:
            file_path: File path from comment
            line: Line number from comment

        Returns:
            True if line is valid (exists in diff as added line), False otherwise
        """
        for hunk in self._current_hunks:
            if hunk.file_path == file_path:
                added_lines = hunk.get_added_lines()
                valid_lines = {line_num for line_num, _ in added_lines}
                if line in valid_lines:
                    return True
        return False

    async def _analyze_diff_with_llm(
        self,
        pr_data: Dict[str, Any],
        hunks: List[DiffHunk],
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
                f"@@ -{hunk.old_start},{hunk.old_count} " f"+{hunk.new_start},{hunk.new_count} @@"
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
                f"{first_part}\n\n"
                f"... [TRUNCATED: {omitted} characters omitted] ...\n\n"
                f"{last_part}"
            )

        # Build diff summary (list of changed files)
        changed_files = list(set(hunk.file_path for hunk in hunks))
        diff_summary = "\n".join(f"- {f}" for f in changed_files)

        # Prepare prompt
        prompt = PR_REVIEW_PROMPT.format(
            pr_title=pr_data.get("title", ""),
            pr_description=pr_data.get("body") or "(no description)",
            pr_author=pr_data.get("user", {}).get("login", "unknown"),
            base_branch=pr_data.get("base", {}).get("ref", "main"),
            head_branch=pr_data.get("head", {}).get("ref", "unknown"),
            diff_summary=diff_summary if diff_summary else "(no files changed)",
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
        result = PRReviewResult(**result_json)

        return result

    async def post_review(
        self,
        owner: str,
        repo: str,
        pr_number: int,
        commit_sha: str,
        result: PRReviewResult,
    ) -> None:
        """Post review comments to PR.

        Args:
            owner: Repository owner
            repo: Repository name
            pr_number: PR number
            commit_sha: HEAD commit SHA
            result: Review result to post
        """
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
            # Validate that the line number exists in the diff
            if not self._is_valid_comment_line(comment.file_path, comment.line):
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
                    owner=owner,
                    repo=repo,
                    pr_number=pr_number,
                    commit_sha=commit_sha,
                    file_path=comment.file_path,
                    line=comment.line,
                    body=comment_body,
                )
                posted_count += 1
            except ValueError as e:
                logger.error(f"Failed to post comment for {comment.file_path}:{comment.line}: {e}")
                skipped_count += 1

        logger.info(f"Review posted: {posted_count} comments posted, {skipped_count} skipped")
