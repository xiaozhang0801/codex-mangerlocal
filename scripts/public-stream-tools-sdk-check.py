"""Replay the two actual public gateway stream:true HTTP tool fixtures.

Run gateway_logs with CM_REPAIR_FIXTURE_DIR set to a fresh directory, then pass
its stream-http subdirectory here. Requires openai==2.29.0 and httpx==0.28.1.
Rust exercises loopback HTTP; this official SDK check uses MockTransport replay,
never a live account, upstream, or a direct connection to the Rust gateway.
"""

import sys

sys.dont_write_bytecode = True

import argparse
import json
from pathlib import Path

import httpx
import openai

REQUIRED_CASES = {
    "stream_http_native_responses_function_tools",
    "stream_http_account_chat_function_tools",
}


def check_fixture(path):
    fixture = json.loads(path.read_text(encoding="utf-8"))
    assert fixture["case"] == path.stem
    assert fixture["status"] == 200
    assert fixture["request"]["stream"] is True
    assert isinstance(fixture["actual_sse"], str) and fixture["actual_sse"]
    expected = fixture["expected"]
    requests = 0

    def handle(request):
        nonlocal requests
        requests += 1
        assert request.method == "POST"
        assert request.url.path == fixture["path"]
        sent = json.loads(request.content)
        for key, value in fixture["request"].items():
            assert sent[key] == value, key
        return httpx.Response(
            200,
            headers={"content-type": "text/event-stream"},
            content=fixture["actual_sse"].encode("utf-8"),
        )

    with openai.OpenAI(
        api_key="s02-synthetic-http-replay",
        base_url="https://s02.invalid/v1",
        max_retries=0,
        http_client=httpx.Client(transport=httpx.MockTransport(handle)),
    ) as client:
        if fixture["path"] == "/v1/chat/completions":
            request = dict(fixture["request"])
            request.pop("stream")
            with client.chat.completions.stream(**request) as stream:
                result = stream.get_final_completion()
            assert len(result.choices) == 1
            choice = result.choices[0]
            assert choice.finish_reason == "tool_calls"
            assert choice.message.role == "assistant"
            assert choice.message.content == expected["text"]
            tools = [
                {
                    "id": tool.id,
                    "type": tool.type,
                    "function": {
                        "name": tool.function.name,
                        "arguments": tool.function.arguments,
                    },
                }
                for tool in choice.message.tool_calls or []
            ]
            assert tools == expected["tools"], tools
            assert result.usage is not None
            assert result.usage.model_dump(exclude_unset=True) == expected["usage"]
        else:
            assert fixture["path"] == "/v1/responses"
            events = list(client.responses.create(**fixture["request"]))
            completed = [event for event in events if event.type == "response.completed"]
            assert len(completed) == 1
            assert not any(event.type in {"error", "response.failed", "response.incomplete"} for event in events)
            response = completed[0].response.model_dump(mode="json", exclude_unset=True)
            for key in ("id", "model", "status", "output", "usage"):
                assert response[key] == expected["response"][key], key
            tools = [
                {
                    "id": item["call_id"],
                    "type": "function",
                    "function": {"name": item["name"], "arguments": item["arguments"]},
                }
                for item in response["output"]
                if item["type"] == "function_call"
            ]
            assert tools == expected["tools"], tools
    assert requests == 1
    assert len(tools) == 3
    for tool in tools:
        json.loads(tool["function"]["arguments"])
    print(f"PASS {path.stem}: public HTTP SSE replay preserved three exact tools and mixed content")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture_dir", type=Path)
    root = parser.parse_args().fixture_dir.resolve(strict=True)
    paths = sorted(root.glob("*.json"))
    assert {path.stem for path in paths} == REQUIRED_CASES, "Expected both complete public HTTP stream fixtures"
    print(f"openai={openai.__version__} httpx={httpx.__version__}")
    for path in paths:
        check_fixture(path)
    print("PASS: checked 2 public HTTP stream fixture(s); SDK HTTP transport was fully mocked")


if __name__ == "__main__":
    main()
