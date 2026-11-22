"""Authenticated Axon API bridge for importing provider context and results."""

from __future__ import annotations

from urllib.parse import urlsplit
from uuid import UUID

import httpx


class AxonApiError(RuntimeError):
    """Raised when Axon rejects or cannot complete an API operation."""


class AxonApiClient:
    def __init__(
        self,
        base_url: str,
        firebase_id_token: str,
        *,
        timeout_seconds: float = 20.0,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        parsed = urlsplit(base_url)
        local_http = parsed.scheme == "http" and parsed.hostname in {
            "localhost",
            "127.0.0.1",
            "::1",
        }
        if parsed.scheme != "https" and not local_http:
            raise ValueError("Axon API URL must use HTTPS (HTTP is allowed only for localhost).")
        if not parsed.netloc or parsed.username or parsed.password:
            raise ValueError("Axon API URL must be an absolute URL without embedded credentials.")
        if not firebase_id_token.strip():
            raise ValueError("A Firebase ID token is required for Axon API access.")
        self._base_url = base_url.rstrip("/")
        self._token = firebase_id_token.strip()
        self._timeout = timeout_seconds
        self._transport = transport

    async def create_artifact(
        self,
        workspace_id: str,
        title: str,
        content: str,
        *,
        source_agent_id: str | None = None,
    ) -> dict[str, object]:
        try:
            normalized_workspace_id = str(UUID(workspace_id))
        except (ValueError, AttributeError):
            raise ValueError("Workspace ID must be a valid UUID.") from None
        if not title.strip() or len(title.strip()) > 200:
            raise ValueError("Artifact title must contain between 1 and 200 characters.")
        if not content or len(content.encode("utf-8")) > 512_000:
            raise ValueError("Artifact content must contain between 1 and 512000 UTF-8 bytes.")

        payload: dict[str, object] = {
            "title": title.strip(),
            "content": content,
            "source_agent_id": source_agent_id,
        }
        try:
            async with httpx.AsyncClient(
                timeout=self._timeout, transport=self._transport
            ) as client:
                response = await client.post(
                    f"{self._base_url}/api/v1/workspaces/{normalized_workspace_id}/artifacts",
                    headers={"Authorization": f"Bearer {self._token}"},
                    json=payload,
                )
                response.raise_for_status()
                result = response.json()
        except httpx.HTTPStatusError as exc:
            raise AxonApiError(
                f"Axon rejected the artifact (HTTP {exc.response.status_code})."
            ) from None
        except httpx.HTTPError as exc:
            raise AxonApiError(f"Axon API request failed: {type(exc).__name__}.") from None
        except ValueError:
            raise AxonApiError("Axon returned an invalid JSON response.") from None

        if not isinstance(result, dict) or not isinstance(result.get("id"), str):
            raise AxonApiError("Axon returned an invalid artifact response.")
        return result
