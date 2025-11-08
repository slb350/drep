.. drep documentation master file, created by
   sphinx-quickstart on Fri Nov  7 15:47:22 2025.
   You can adapt this file completely to your liking, but it should at least
   contain the root `toctree` directive.

drep - AI-Powered Code Review Tool
===================================

**drep** (PyPI: **drep-ai**) is an AI-powered code review and documentation quality tool
that works with Gitea, GitHub, and GitLab.

Features
--------

* **AI-Powered Analysis**: LLM-based code quality and documentation review
* **Documentation Specialist**: Three-tiered approach for typos, grammar, and syntax
* **Markdown Linting**: 10 comprehensive checks for documentation quality
* **Code Quality**: AST parsing and LLM-based bug detection
* **Automated PR Reviews**: Inline comments with actionable feedback
* **Performance Optimized**: Caching, circuit breakers, metrics, progress tracking

Quick Start
-----------

Install from PyPI::

    pip install drep-ai

Usage::

    from drep.llm.client import LLMClient

    # Create LLM client
    client = LLMClient(
        endpoint="http://localhost:1234/v1",
        model="local-model",
    )

    # Analyze code
    result = await client.analyze_code(
        system_prompt="Review this code for bugs",
        code="def foo(): pass"
    )

Contents
--------

.. toctree::
   :maxdepth: 2
   :caption: API Documentation:

   modules

