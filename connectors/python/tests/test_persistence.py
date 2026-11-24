import asyncio

import httpx

from axon_connector.persistence import persist_artifact


def test_persist_artifact_uses_firebase_token_and_workspace_route():
    captured = {}

    async def handler(request: httpx.Request) -> httpx.Response:
        captured["url"] = str(request.url)
        captured["authorization"] = request.headers.get("authorization")
        captured["payload"] = __import__("json").loads(request.content)
        return httpx.Response(201, json={"id": "artifact-123"})

    async def run() -> str:
        async with httpx.AsyncClient(transport=httpx.MockTransport(handler)) as client:
            return await persist_artifact(
                "https://axon.example.test/",
                "firebase-id-token",
                "workspace-123",
                "Architecture review",
                "Provider result",
                client=client,
            )

    assert asyncio.run(run()) == "artifact-123"
    assert captured["url"] == (
        "https://axon.example.test/api/v1/workspaces/workspace-123/artifacts"
    )
    assert captured["authorization"] == "Bearer firebase-id-token"
    assert captured["payload"] == {
        "title": "Architecture review",
        "content": "Provider result",
    }


def test_persist_artifact_rejects_missing_authentication():
    async def run() -> None:
        await persist_artifact(
            "https://axon.example.test",
            "",
            "workspace-123",
            "Title",
            "Content",
        )

    try:
        asyncio.run(run())
    except ValueError as error:
        assert "Firebase ID token" in str(error)
    else:
        raise AssertionError("missing Firebase ID token must be rejected")
