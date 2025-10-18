"""Configuration models for drep."""

from typing import List

from pydantic import BaseModel, Field


class GiteaConfig(BaseModel):
    """Gitea platform configuration."""

    url: str = Field(..., description="Gitea base URL (e.g., http://192.168.1.14:3000)")
    token: str = Field(..., description="Gitea API token")
    repositories: List[str] = Field(..., description="Repository patterns (e.g., steve/*)")


class DocumentationConfig(BaseModel):
    """Documentation analysis settings."""

    enabled: bool = True
    custom_dictionary: List[str] = Field(default_factory=list)


class Config(BaseModel):
    """Main configuration."""

    gitea: GiteaConfig
    documentation: DocumentationConfig
    database_url: str = "sqlite:///./drep.db"
