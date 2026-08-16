"""Shared base for the GitLab adapter mixins.

``GitLabPrMixin`` and ``GitLabReviewMixin`` are split out of
``drep/adapters/gitlab.py`` for file-size reasons but need the same host
surface. Inheriting a real base instead of re-declaring an ``if TYPE_CHECKING``
stub block in each file means one definition, real types, and no drift when a
signature changes.
"""

import urllib.parse

from drep.adapters.base import BaseAdapter


class GitLabMixinBase(BaseAdapter):
    """Host surface shared by the GitLab adapter mixins."""

    platform_name = "GitLab"

    #: Set by GitLabAdapter.__init__ (e.g. "https://gitlab.com/api/v4").
    api_url: str

    @property
    def api_base_url(self) -> str:
        """Base URL reported in connection-failure messages."""
        return self.api_url

    def _encode_project_path(self, owner: str, repo: str) -> str:
        """Encode project path for GitLab API.

        GitLab APIs require namespace/project to be URL-encoded.
        Example: owner/repo → owner%2Frepo

        Args:
            owner: Project namespace/owner
            repo: Project name

        Returns:
            URL-encoded project path

        Example:
            _encode_project_path("myorg", "myrepo") → "myorg%2Fmyrepo"
        """
        project_path = f"{owner}/{repo}"
        return urllib.parse.quote(project_path, safe="")
