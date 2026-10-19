"""Consume actual gateway HTTP JSON exported by the R04 regression tests.

Set CM_REPAIR_FIXTURE_DIR, run the gateway_logs chat_non_stream_tools tests,
then run this script with that directory. Requires openai==2.29.0 and httpx.
The SDK transport replays captured gateway JSON; it never contacts an
account or live upstream. Rust tests exercise the gateway over loopback HTTP.
"""

import sys

sys.dont_write_bytecode = True

import argparse
import json
from pathlib import Path

import httpx
import openai

REQUIRED_CASES = {
    "non_stream_chat_preserves_function_calls_from_responses",
    "non_stream_chat_mixed_output_json",
    "non_stream_chat_mixed_output_sse",
    "non_stream_chat_assembles_sse_tool_deltas_before_final_response",
    "non_stream_chat_pure_text_json",
    "non_stream_chat_pure_text_sse",
    "non_stream_chat_custom_tools_only_json",
    "non_stream_chat_custom_tools_only_sse",
    "non_stream_chat_custom_tools_mixed_json",
    "non_stream_chat_custom_tools_mixed_sse",
    "non_stream_native_responses_json",
    "non_stream_native_responses_sse",
    "non_stream_native_chat_json_preserves_tools_without_responses_mapping",
    "non_stream_native_chat_sse_interleaved_pure_tools",
    "non_stream_native_chat_sse_interleaved_mixed",
    "non_stream_chat_repeated_deltas_without_done",
    "non_stream_chat_repeated_deltas_with_done",
}


def check_fixture(path):
    fixture = json.loads(path.read_text(encoding="utf-8"))
    assert fixture["case"] == path.stem
    expected = fixture["expected"]
    request_count = 0
    sent = fixture["upstream_request"]
    if fixture["native_client"] and fixture["path"] == "/v1/chat/completions":
        assert isinstance(sent["messages"], list) and "input" not in sent
        assert sent["stream"] is False
    else:
        assert "input" in sent and "messages" not in sent

    def handle(request):
        nonlocal request_count
        request_count += 1
        assert request.method == "POST"
        assert request.url.path == fixture["path"]
        assert json.loads(request.content)["stream"] is False
        return httpx.Response(
            fixture["status"],
            headers={"content-type": "application/json"},
            json=fixture["actual"],
        )

    with openai.OpenAI(
        api_key="r04-synthetic",
        base_url="https://r04.invalid/v1",
        max_retries=0,
        http_client=httpx.Client(transport=httpx.MockTransport(handle)),
    ) as client:
        if fixture["path"] == "/v1/responses":
            result = client.responses.create(**fixture["request"])
            actual = result.model_dump(exclude_unset=True)
            # The native route is a passthrough: SDK parsing must retain the
            # original function calls, text, reasoning, and token accounting.
            assert actual["output"] == expected["output"]
            assert actual["usage"] == expected["usage"]
        else:
            result = client.chat.completions.create(**fixture["request"])
            choice = result.choices[0]
            message = choice.message.model_dump(exclude_unset=True)
            assert message == expected["message"], (message, expected["message"])
            assert choice.finish_reason == expected["finish_reason"]
            usage = result.usage.model_dump(exclude_unset=True) if result.usage else None
            assert usage == expected["usage"], (usage, expected["usage"])
            for tool in choice.message.tool_calls or []:
                assert tool.type == "function"
                # Exact argument strings were checked above. Also verify that
                # object arguments did not become a double-encoded JSON string.
                parsed = json.loads(tool.function.arguments)
                assert isinstance(parsed, (dict, list)), parsed
    assert request_count == 1
    print(f"PASS {path.name}: official SDK retained the actual gateway result")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture_dir", type=Path)
    parser.add_argument("--allow-partial", action="store_true",
                        help="Only for an explicit failing baseline, not final verification")
    args = parser.parse_args()
    paths = sorted(args.fixture_dir.resolve(strict=True).glob("non_stream_*.json"))
    assert paths, "No R04 HTTP fixtures found"
    if not args.allow_partial:
        assert {path.stem for path in paths} == REQUIRED_CASES, "Incomplete or stale R04 fixture set"
    print(f"openai={openai.__version__} httpx={httpx.__version__}")
    for path in paths:
        check_fixture(path)
    print(f"PASS: {len(paths)} actual HTTP fixture(s); SDK transport replay only")


if __name__ == "__main__":
    main()
