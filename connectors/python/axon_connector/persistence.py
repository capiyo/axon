"""Persist approved connector results as authenticated Axon workspace artifacts."""

from __future__ import annotations

from urllib.parse import quote

import httpx


class AxonPersistenceError(RuntimeError):
    """The authenticated Axon API rejected or failed to store the artifact."""


async def persist_artifact(
    api_base_url: str,
    firebase_id_token: str,
    workspace_id: str,
    title: str,
    content: str,
    *,
    client: httpx.AsyncClient | None = None,
) -> str:
    """Create an artifact using a verified Firebase ID token.

    The actor identity is derived by Axon's Firebase middleware. No caller-
    supplied actor ID is sent as an authentication claim.
    """
    if not api_base_url.strip() or not firebase_id_token.strip():
        raise ValueError("Axon API URL and Firebase ID token are required.")
    if not workspace_id.strip() or not title.strip() or not content.strip():
        raise ValueError("Workspace, artifact title, and artifact content are required.")

    url = (
        f"{api_base_url.rstrip('/')}/api/v1/workspaces/"
        f"{quote(workspace_id, safe='')}/artifacts"
    )
    payload = {"title": title.strip()[:200], "content": content}
    headers = {"Authorization": f"Bearer {firebase_id_token}"}

    async def send(session: httpx.AsyncClient) -> str:
        try:
            response = await session.post(url, json=payload, headers=headers)
            response.raise_for_status()
            result = response.json()
        except (httpx.HTTPError, ValueError) as error:
            raise AxonPersistenceError("Axon could not persist the connector artifact.") from error
        artifact_id = result.get("id") if isinstance(result, dict) else None
        if not isinstance(artifact_id, str) or not artifact_id:
            raise AxonPersistenceError("Axon returned an invalid artifact receipt.")
        return artifact_id

    if client is not None:
        return await send(client)
    async with httpx.AsyncClient(timeout=30.0) as session:
        return await send(session)
