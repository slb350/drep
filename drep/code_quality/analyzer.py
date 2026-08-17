"""LLM-powered code quality analyzer."""

import logging
import warnings

from drep.languages import registry
from drep.llm.client import LLMClient
from drep.models.findings import Finding
from drep.models.llm_findings import CodeAnalysisResult

logger = logging.getLogger(__name__)

# Maximum file size to analyze (in characters)
# Approximately 8k tokens (assuming ~4 chars per token)
MAX_FILE_SIZE = 32000


class CodeQualityAnalyzer:
    """LLM-powered code quality analyzer.

    Language-agnostic by construction: the model reads any language without a
    parser, so the only per-language input is the prompt, which comes from the
    registry. Deterministic style and syntax belong to the project's own tools
    (see drep.languages.runner), not here.
    """

    def __init__(self, llm_client: LLMClient):
        """Initialize analyzer with LLM client.

        Args:
            llm_client: Configured LLMClient instance for making analysis requests
        """
        self.llm_client = llm_client

    async def analyze_file(
        self, file_path: str, content: str, repo_id: str, commit_sha: str
    ) -> list[Finding]:
        """Analyze Python file for code quality issues.

        Args:
            file_path: Path to the file being analyzed
            content: File content to analyze
            repo_id: Repository identifier for rate limiting
            commit_sha: Current commit SHA for cache invalidation

        Returns:
            List of Finding objects describing issues found

        Raises:
            Exception: Any LLM transport or response-parsing failure. The file
                went unanalyzed, and callers must be able to tell that apart
                from a file that analyzed cleanly - see RepositoryScanner,
                which counts the file as failed.

        Note:
            - Files larger than MAX_FILE_SIZE (32k chars) are skipped
            - Returns empty list if the file is too large, empty, or of a type
              no registered language claims
        """
        language = registry.detect(file_path)
        if language is None:
            logger.debug(f"Skipping {file_path}: no registered language claims it")
            return []

        # Check file size limit
        if len(content) > MAX_FILE_SIZE:
            logger.warning(
                f"Skipping {file_path}: file too large ({len(content)} chars, max {MAX_FILE_SIZE})"
            )
            return []

        # Skip empty files
        if not content.strip():
            logger.debug(f"Skipping {file_path}: empty file")
            return []

        # Call LLM with structured schema
        logger.debug(f"Analyzing {file_path} ({len(content)} chars)")

        result_dict = await self.llm_client.analyze_code_json(
            system_prompt=language.analysis_prompt(),
            code=content,
            schema=CodeAnalysisResult,
            repo_id=repo_id,
            commit_sha=commit_sha,
            # Per-language key so cache entries and metrics cannot cross
            # languages - the prompt differs, so the response would too.
            analyzer=f"code_quality_{language.name}",
        )

        # Convert dict to Pydantic model
        result = CodeAnalysisResult(**result_dict)

        # Log analysis results
        critical_high_count = sum(1 for i in result.issues if i.severity in ["critical", "high"])
        logger.info(
            f"Analyzed {file_path}: found {len(result.issues)} issues "
            f"({critical_high_count} critical/high)"
        )

        # Convert to Finding objects
        return result.to_findings(file_path)

    async def analyze_files(
        self, files: list[tuple[str, str]], repo_id: str, commit_sha: str
    ) -> list[Finding]:
        """Analyze multiple Python files.

        Args:
            files: List of (file_path, content) tuples
            repo_id: Repository identifier for rate limiting
            commit_sha: Current commit SHA for cache invalidation

        Returns:
            Combined list of findings from all files

        Raises:
            Exception: Propagated from the first file that fails to analyze
        """
        warnings.warn(
            "CodeQualityAnalyzer.analyze_files is deprecated (no production callers - "
            "RepositoryScanner._analyze_files_with runs the files concurrently and "
            "reports which ones failed) and will be removed in drep 1.4.0",
            DeprecationWarning,
            stacklevel=2,
        )
        all_findings: list[Finding] = []

        for file_path, content in files:
            findings = await self.analyze_file(file_path, content, repo_id, commit_sha)
            all_findings.extend(findings)

        return all_findings

    def is_supported_file(self, file_path: str) -> bool:
        """Check if any registered language claims this file.

        Args:
            file_path: Path to check

        Returns:
            True if file should be analyzed, False otherwise
        """
        return registry.detect(file_path) is not None
