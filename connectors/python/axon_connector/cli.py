"""Command-line entry point for explicit provider and Axon API handoffs."""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import sys

from .api import AxonApiClient, AxonApiError
from .models import AxonEnvelope
from .providers.chatgpt import load_chatgpt_export
from .providers.claude import load_claude_export
from .providers.deepseek import DeepSeekClient, DeepSeekError
from .transfer import transfer_to_deepseek


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="axon-bridge")
    parser.add_argument("claude_export", help="Path to a Claude or ChatGPT export JSON file.")
    parser.add_argument("--provider", choices=["claude", "chatgpt"], default="claude")
    parser.add_argument("--conversation-id", help="Select one conversation from a multi-conversation ChatGPT export.")
    parser.add_argument("--workspace-id", required=True)
    parser.add_argument(
        "--actor-id",
        required=True,
        help="Provenance label only; Axon derives the authenticated actor from the Firebase token.",
    )
    parser.add_argument("--instruction", required=True, help="What the receiving provider should do.")
    parser.add_argument("--model", default=os.getenv("DEEPSEEK_MODEL", "deepseek-chat"))
    parser.add_argument("--approve-transfer", action="store_true")
    parser.add_argument(
        "--import-to-axon",
        action="store_true",
        help="Explicitly save the selected source conversation as an Axon artifact.",
    )
    parser.add_argument(
        "--persist-response",
        action="store_true",
        help="After an approved provider transfer, save its response as an Axon artifact.",
    )
    parser.add_argument(
        "--axon-api-url",
        default=os.getenv("AXON_API_URL", "http://127.0.0.1:8080"),
    )
    parser.add_argument(
        "--axon-id-token",
        default=os.getenv("AXON_FIREBASE_ID_TOKEN"),
        help="Firebase ID token; prefer AXON_FIREBASE_ID_TOKEN to avoid shell history.",
    )
    parser.add_argument(
        "--print-context",
        action="store_true",
        help="Print the exact normalized AXP context during preview; keep output private.",
    )
    parser.add_argument("--max-context-chars", type=int, default=120_000)
    return parser


async def _persist_artifact(
    args: argparse.Namespace, title: str, content: str
) -> dict[str, object]:
    if not args.axon_id_token:
        raise ValueError(
            "Set AXON_FIREBASE_ID_TOKEN or pass --axon-id-token to use the Axon API bridge."
        )
    client = AxonApiClient(args.axon_api_url, args.axon_id_token)
    return await client.create_artifact(args.workspace_id, title, content)


async def _run(args: argparse.Namespace) -> int:
    try:
        if args.max_context_chars <= 0:
            raise ValueError("--max-context-chars must be greater than zero.")
        if not args.instruction.strip():
            raise ValueError("A non-empty instruction is required.")
        if args.persist_response and not args.approve_transfer:
            raise ValueError("--persist-response requires --approve-transfer.")
        if args.persist_response and not args.axon_id_token:
            raise ValueError("--persist-response requires AXON_FIREBASE_ID_TOKEN.")

        if args.provider == "chatgpt":
            conversation = load_chatgpt_export(args.claude_export, args.conversation_id)
        else:
            if args.conversation_id:
                raise ValueError("--conversation-id is only supported with --provider chatgpt.")
            conversation = load_claude_export(args.claude_export)
        envelope = AxonEnvelope.from_conversation(
            conversation, workspace_id=args.workspace_id, actor_id=args.actor_id
        )
        serialized = json.dumps(envelope.to_dict(), ensure_ascii=False)
        if len(serialized) > args.max_context_chars:
            raise ValueError(
                f"Context is {len(serialized)} characters; limit is {args.max_context_chars}."
            )
        print(
            f"Source: {conversation.provider} export | Conversation: {conversation.title}\n"
            f"Messages: {len(conversation.messages)} | Context size: {len(serialized)} chars"
        )

        if args.import_to_axon:
            imported = await _persist_artifact(
                args,
                f"Imported conversation: {conversation.title}",
                serialized,
            )
            print(f"Saved source conversation to Axon artifact {imported['id']}.")

        if not args.approve_transfer:
            if args.print_context:
                print("\n--- Exact AXP context preview ---\n")
                print(json.dumps(envelope.to_dict(), ensure_ascii=False, indent=2))
            if not args.import_to_axon:
                print("Preview only. Use --import-to-axon to save this selected context.")
            return 0

        provider = DeepSeekClient(model=args.model)
        result = await transfer_to_deepseek(envelope, args.instruction, provider, approved=True)
        print("\n--- DeepSeek response ---\n")
        print(result)

        if args.persist_response:
            saved = await _persist_artifact(
                args,
                f"DeepSeek response: {conversation.title}",
                result,
            )
            print(f"Saved provider response to Axon artifact {saved['id']}.")
        return 0
    except (
        OSError,
        ValueError,
        PermissionError,
        AxonApiError,
        DeepSeekError,
        json.JSONDecodeError,
    ) as exc:
        print(f"axon-bridge: {exc}", file=sys.stderr)
        return 2


def main() -> None:
    raise SystemExit(asyncio.run(_run(build_parser().parse_args())))


if __name__ == "__main__":
    main()
