"""Tests for drep.adapters.base.BaseAdapter abstract class."""

from abc import ABC
from typing import Dict, List, Optional


def test_base_adapter_is_abstract_base_class():
    """Test that BaseAdapter cannot be instantiated directly."""
    from drep.adapters.base import BaseAdapter

    # Should be an abstract base class
    assert issubclass(BaseAdapter, ABC)

    # Should raise TypeError when trying to instantiate
    try:
        BaseAdapter()
        assert False, "Should not be able to instantiate BaseAdapter directly"
    except TypeError as e:
        # Expected - can't instantiate abstract class
        assert "abstract" in str(e).lower()


def test_base_adapter_has_required_abstract_methods():
    """Test that BaseAdapter defines all required abstract methods."""
    from drep.adapters.base import BaseAdapter

    # Get all abstract methods
    abstract_methods = BaseAdapter.__abstractmethods__

    # Required methods for platform adapters
    required_methods = {
        "create_issue",
        "get_pr",
        "get_pr_diff",
        "create_pr_comment",
        "post_review_comment",
        "get_file_content",
        "close",
    }

    # All required methods should be abstract
    assert required_methods.issubset(
        abstract_methods
    ), f"Missing abstract methods: {required_methods - abstract_methods}"


def test_subclass_without_implementation_raises_error():
    """Test that subclass missing abstract methods raises TypeError."""
    from drep.adapters.base import BaseAdapter

    # Define incomplete subclass
    class IncompleteAdapter(BaseAdapter):
        """Incomplete adapter missing some abstract methods."""

        async def create_issue(self, owner, repo, title, body, labels=None):
            pass

        # Missing other required methods

    # Should not be able to instantiate
    try:
        IncompleteAdapter()
        assert False, "Should not allow instantiation of incomplete subclass"
    except TypeError as e:
        assert "abstract" in str(e).lower()


def test_complete_subclass_can_be_instantiated():
    """Test that complete implementation of BaseAdapter can be instantiated."""
    from drep.adapters.base import BaseAdapter

    # Define complete subclass with all abstract methods
    class CompleteAdapter(BaseAdapter):
        """Complete adapter with all required methods."""

        async def create_issue(
            self, owner: str, repo: str, title: str, body: str, labels: Optional[List[str]] = None
        ) -> int:
            return 1

        async def get_pr(self, owner: str, repo: str, pr_number: int) -> Dict:
            return {}

        async def get_pr_diff(self, owner: str, repo: str, pr_number: int) -> str:
            return ""

        async def create_pr_comment(self, owner: str, repo: str, pr_number: int, body: str) -> None:
            pass

        async def post_review_comment(
            self,
            owner: str,
            repo: str,
            pr_number: int,
            file_path: str,
            line: int,
            body: str,
        ) -> None:
            pass

        async def get_file_content(self, owner: str, repo: str, file_path: str, ref: str) -> str:
            return ""

        async def get_default_branch(self, owner: str, repo: str) -> str:
            return "main"

        async def close(self) -> None:
            pass

    # Should be able to instantiate
    adapter = CompleteAdapter()
    assert adapter is not None
    assert isinstance(adapter, BaseAdapter)


def test_base_adapter_method_signatures():
    """Test that BaseAdapter abstract methods have correct signatures."""
    import inspect

    from drep.adapters.base import BaseAdapter

    # Check create_issue signature
    create_issue = getattr(BaseAdapter, "create_issue")
    sig = inspect.signature(create_issue)
    params = list(sig.parameters.keys())
    assert "owner" in params
    assert "repo" in params
    assert "title" in params
    assert "body" in params
    assert "labels" in params

    # Check get_pr signature
    get_pr = getattr(BaseAdapter, "get_pr")
    sig = inspect.signature(get_pr)
    params = list(sig.parameters.keys())
    assert "owner" in params
    assert "repo" in params
    assert "pr_number" in params

    # Check post_review_comment signature
    post_review_comment = getattr(BaseAdapter, "post_review_comment")
    sig = inspect.signature(post_review_comment)
    params = list(sig.parameters.keys())
    assert "owner" in params
    assert "repo" in params
    assert "pr_number" in params
    assert "file_path" in params
    assert "line" in params
    assert "body" in params


def test_gitea_adapter_inherits_from_base_adapter():
    """Test that GiteaAdapter inherits from BaseAdapter."""
    from drep.adapters.base import BaseAdapter
    from drep.adapters.gitea import GiteaAdapter

    # GiteaAdapter should be a subclass of BaseAdapter
    assert issubclass(GiteaAdapter, BaseAdapter)

    # Instance should also be instance of BaseAdapter
    adapter = GiteaAdapter(url="http://example.com", token="test-token")
    assert isinstance(adapter, BaseAdapter)
    assert isinstance(adapter, GiteaAdapter)
