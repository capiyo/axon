from __future__ import annotations

import asyncio
import json
from uuid import uuid4

import httpx
import pytest

from axon_connector.api import AxonApiClient, AxonApiError


def test_artifact_request_uses_firebase_bearer_and_workspace_scope():
    workspace_id = str(uuid4())
    observed: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        observed["url"] = str(request.url)
        observed["authorization"] = request.headers.get("Authorization")
        observed["body"] = json.loads(request.content)
        return httpx.Response(201, json={"id": str(uuid4()), "title": "Imported"})

    client = AxonApiClient(
        "https://axon.example",
        "firebase-id-token",
        transport=httpx.MockTransport(handler),
    )
    result = asyncio.run(
        client.create_artifact(workspace_id, "Imported", "Selected conversation")
    )

    assert result["title"] == "Imported"
    assert observed["url"] == (
        f"https://axon.example/api/v1/workspaces/{workspace_id}/artifacts"
    )
    assert observed["authorization"] == "Bearer firebase-id-token"
    assert observed["body"] == {
        "title": "Imported",
        "content": "Selected conversation",
        "source_agent_id": None,
    }


def test_remote_http_and_embedded_credentials_are_rejected():
    with pytest.raises(ValueError, match="HTTPS"):
        AxonApiClient("http://axon.example", "token")
    with pytest.raises(ValueError, match="embedded credentials"):
        AxonApiClient("https://user:password@axon.example", "token")


def test_api_errors_do_not_echo_provider_response_or_token():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(401, json={"error": "private diagnostic"})

    client = AxonApiClient(
        "https://axon.example",
        "secret-token",
        transport=httpx.MockTransport(handler),
    )
    with pytest.raises(AxonApiError, match="HTTP 401") as error:
        asyncio.run(client.create_artifact(str(uuid4()), "Title", "Content"))
    assert "secret-token" not in str(error.value)
    assert "private diagnostic" not in str(error.value)


def test_artifact_limits_are_checked_before_network_access():
    client = AxonApiClient("https://axon.example", "token")
    with pytest.raises(ValueError, match="512000"):
        asyncio.run(client.create_artifact(str(uuid4()), "Title", "x" * 512_001))


def test_workspace_id_must_be_a_uuid():
    client = AxonApiClient("https://axon.example", "token")
    with pytest.raises(ValueError, match="valid UUID"):
        asyncio.run(client.create_artifact("../other-workspace", "Title", "Content"))


def test_generic_request_uses_firebase_bearer_and_query_parameters():
    observed: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        observed["authorization"] = request.headers.get("Authorization")
        observed["query"] = request.url.params.get("q")
        return httpx.Response(200, json=[{"status": "active"}])

    client = AxonApiClient(
        "https://axon.example",
        "firebase-id-token",
        transport=httpx.MockTransport(handler),
    )
    result = asyncio.run(
        client.request_json(
            "GET",
            f"/api/v1/workspaces/{uuid4()}/memory/context",
            params={"q": "project state", "limit": 5},
        )
    )

    assert result == [{"status": "active"}]
    assert observed == {
        "authorization": "Bearer firebase-id-token",
        "query": "project state",
    }


def test_generic_request_rejects_paths_outside_api_v1():
    client = AxonApiClient("https://axon.example", "token")
    with pytest.raises(ValueError, match="relative /api/v1/ path"):
        asyncio.run(client.request_json("GET", "/admin/secrets"))
