"""Language registry tests.

The registry is what keeps `if python: ... else: ...` out of the codebase.
Every language fact - which extensions it owns, which deterministic tools
check it, how its prompt names it - lives on a LanguageSupport, and callers
ask the registry rather than branching.
"""

import pytest

from drep.languages import LanguageSupport, ToolSpec, registry


class TestExtensionRouting:
    """Which language owns a file is a registry lookup, never a conditional."""

    @pytest.mark.parametrize(
        ("path", "expected"),
        [
            ("src/main.py", "python"),
            ("SRC/MAIN.PY", "python"),
            ("app/index.ts", "typescript"),
            ("app/index.tsx", "typescript"),
            ("app/index.js", "javascript"),
            ("app/index.jsx", "javascript"),
            ("cmd/server.go", "go"),
            ("src/lib.rs", "rust"),
        ],
    )
    def test_detects_language_by_extension(self, path, expected):
        language = registry.detect(path)
        assert language is not None
        assert language.name == expected

    def test_unknown_extension_has_no_language(self):
        assert registry.detect("notes.txt") is None
        assert registry.detect("Makefile") is None

    def test_markdown_is_not_a_code_language(self):
        """Docs are handled by the documentation analyzer, not a code analyzer."""
        assert registry.detect("README.md") is None

    def test_source_extensions_cover_every_registered_language(self):
        extensions = registry.source_extensions()
        for language in registry.languages():
            assert set(language.extensions) <= extensions


class TestLanguagePrompts:
    """The analysis prompt names the language instead of hardcoding PEP 8."""

    def test_every_language_names_itself_in_its_prompt(self):
        for language in registry.languages():
            prompt = language.analysis_prompt()
            assert language.display_name in prompt

    def test_python_prompt_mentions_its_own_conventions(self):
        python = registry.get("python")
        assert "PEP 8" in python.analysis_prompt()

    def test_go_prompt_does_not_mention_python_conventions(self):
        go = registry.get("go")
        prompt = go.analysis_prompt()
        assert "PEP 8" not in prompt
        assert "Python" not in prompt


class TestDeterministicTools:
    """Each language declares the tools that check it deterministically."""

    def test_python_declares_ruff(self):
        python = registry.get("python")
        assert any(t.name == "ruff" for t in python.tools)

    def test_every_tool_declares_how_to_find_and_run_it(self):
        for language in registry.languages():
            for tool in language.tools:
                assert isinstance(tool, ToolSpec)
                assert tool.command, f"{tool.name} has no command"
                # A repo-local path is what makes `node_modules/.bin/eslint`
                # win over a globally installed one
                assert tool.local_paths is not None

    def test_tools_are_only_run_when_the_repo_configures_them(self):
        """ "Style adherence where defined" - a tool with no config is not the
        project's style, so running it would invent findings the project never
        asked for."""
        for language in registry.languages():
            for tool in language.tools:
                assert tool.config_files, f"{tool.name} declares no config files"


class TestRegistryIsClosed:
    """Registration is the only way in, so nothing can special-case a language."""

    def test_get_rejects_an_unregistered_language(self):
        with pytest.raises(KeyError):
            registry.get("cobol")

    def test_languages_are_LanguageSupport_instances(self):
        for language in registry.languages():
            assert isinstance(language, LanguageSupport)


class TestDiffLanguages:
    """A PR diff can span languages, so its rubric must name all of them."""

    def test_detects_the_distinct_languages_in_a_file_list(self):
        languages = registry.detect_all(["a.py", "b.py", "app/x.ts", "cmd/y.go", "notes.txt"])
        assert [lang.name for lang in languages] == ["python", "typescript", "go"]

    def test_ignores_files_no_language_claims(self):
        assert registry.detect_all(["notes.txt", "style.css"]) == []

    def test_review_rubric_names_a_single_language(self):
        from drep.languages.prompts import build_review_rubric

        rubric = build_review_rubric(registry.detect_all(["a.py"]))
        assert "Python" in rubric
        assert "PEP 8" in rubric

    def test_review_rubric_names_every_language_in_a_mixed_diff(self):
        from drep.languages.prompts import build_review_rubric

        rubric = build_review_rubric(registry.detect_all(["a.py", "b.go"]))
        assert "Python" in rubric
        assert "Go" in rubric

    def test_review_rubric_for_an_unrecognised_diff_is_generic(self):
        """A docs-only PR still gets reviewed, just without a language rubric."""
        from drep.languages.prompts import build_review_rubric

        rubric = build_review_rubric([])
        assert "PEP 8" not in rubric
        assert rubric.strip()


class TestPublishedHookCoverage:
    """The hooks other repos consume must fire for every language we support.

    `types: [python]` once meant they never fired in a Go repo. Replacing it
    with a longer hand-written list reintroduces the same silent failure the
    next time a language is registered, so this ties the YAML to the registry.
    """

    @staticmethod
    def _hook_tags():
        import pathlib

        import yaml

        hooks = yaml.safe_load(pathlib.Path(".pre-commit-hooks.yaml").read_text())
        return {
            hook["id"]: set(hook.get("types_or", []) or hook.get("types", [])) for hook in hooks
        }

    # `identify` has no tag for these TypeScript module variants, and
    # pre-commit's types_or cannot match an untagged file. `drep check` still
    # analyzes them - only the published hook cannot target them. Listed here
    # so the gap stays this size: anything new failing this test is a bug.
    KNOWN_UNTAGGABLE = frozenset({".mts", ".cts"})

    def test_every_registered_extension_maps_to_a_declared_tag(self):
        from identify import identify

        declared = self._hook_tags()["drep-check-push"]
        unreachable = []
        for language in registry.languages():
            for extension in language.extensions:
                if extension in self.KNOWN_UNTAGGABLE:
                    continue
                tags = identify.tags_from_filename(f"x{extension}")
                if not tags:
                    unreachable.append((extension, "identify knows no tag for it"))
                elif not (tags & declared):
                    unreachable.append((extension, f"tags {sorted(tags)} not in hook types"))

        assert not unreachable, (
            f"these registered extensions would never trigger the published hook: {unreachable}"
        )

    def test_the_code_hooks_all_declare_the_same_tags(self):
        tags = self._hook_tags()
        assert tags["drep-check"] == tags["drep-check-push"] == tags["drep-check-all"]

    def test_the_known_gap_is_still_only_a_gap_in_the_hook(self):
        """Those extensions must still work for a direct `drep check`."""
        for extension in self.KNOWN_UNTAGGABLE:
            assert registry.detect(f"a{extension}") is not None
