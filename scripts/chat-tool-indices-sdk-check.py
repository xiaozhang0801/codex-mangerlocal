"""Validate exported Chat tool SSE with the official OpenAI stream helper.

Example from the repository root in PowerShell:
    $env:CM_REPAIR_FIXTURE_DIR = (Join-Path $PWD 'target/q02-sdk-final')
    cargo test -p codexmanager-service --lib stream_readers::chat_completions::tool_tests -j 2 -- --test-threads=1
    python -B -m pip install --no-cache-dir --no-compile --target target/q02-sdk-python openai==2.29.0
    $env:PYTHONPATH = (Join-Path $PWD 'target/q02-sdk-python')
    python -B scripts/chat-tool-indices-sdk-check.py target/q02-sdk-final

The baseline sparse-index fixture can be checked with:
    python -B scripts/chat-tool-indices-sdk-check.py target/q02-baseline-sdk --expect-index-error

Positive checks require all 14 q02-0.sse through q02-13.sse fixtures and
their matching q02-*-expected.json files. Use a fresh export directory.
The HTTP transport is fully mocked; no OpenAI account or running gateway
is required.
"""

import sys

sys.dont_write_bytecode = True

import argparse
import json
from pathlib import Path

import httpx
import openai

REQUIRED_FIXTURE_STEMS = {f"q02-{index}" for index in range(14)}


def fixture_paths(root, expect_index_error):
    paths = sorted(root.glob("q02-*.sse"))
    assert paths, f"No q02-*.sse fixture found in {root}"
    if expect_index_error:
        # Keep the original sparse-index negative control independent of the
        # complete positive fixture set.
        baseline = root / "q02-tools.sse"
        assert baseline in paths, "The baseline Q02 fixture is required"
        assert (root / "q02-tools-expected.json").is_file(), "Missing baseline expectation"
        return [baseline]
    actual = {path.stem for path in paths}
    assert actual == REQUIRED_FIXTURE_STEMS, (
        "Incomplete or stale streaming fixture set: "
        f"missing={sorted(REQUIRED_FIXTURE_STEMS - actual)} "
        f"unexpected={sorted(actual - REQUIRED_FIXTURE_STEMS)}"
    )
    expected = {path.name for path in root.glob("q02-*-expected.json")}
    required_expected = {f"{stem}-expected.json" for stem in REQUIRED_FIXTURE_STEMS}
    assert expected == required_expected, (
        "Unpaired streaming fixture expectations: "
        f"missing={sorted(required_expected - expected)} "
        f"unexpected={sorted(expected - required_expected)}"
    )
    return paths


def check_fixture(sse_path, expect_index_error):
    sse = sse_path.read_bytes()
    expected_path = sse_path.with_name(sse_path.stem + "-expected.json")
    expected = json.loads(expected_path.read_text(encoding="utf-8"))
    assert isinstance(expected, list) and expected, "Expected a nonempty tool list"
    if sse_path.stem == "q02-tools":
        assert len(expected) == 3, "The main Q02 fixture must contain three tools"
    expected_json = [json.loads(tool["function"]["arguments"]) for tool in expected]
    request_count = 0

    def handle(request):
        nonlocal request_count
        request_count += 1
        assert request.method == "POST"
        assert request.url.path == "/v1/chat/completions"
        assert json.loads(request.content)["stream"] is True
        return httpx.Response(
            200,
            headers={"content-type": "text/event-stream"},
            content=sse,
        )

    try:
        with openai.OpenAI(
            api_key="q02-synthetic",
            base_url="https://q02.invalid/v1",
            max_retries=0,
            http_client=httpx.Client(transport=httpx.MockTransport(handle)),
        ) as client:
            with client.chat.completions.stream(
                model="synthetic",
                messages=[{"role": "user", "content": "Q02 synthetic fixture"}],
            ) as stream:
                result = stream.get_final_completion()
    except IndexError:
        if not expect_index_error:
            raise
        assert request_count == 1
        print(
            f"PASS {sse_path.name}: baseline sparse tool indices raise IndexError "
            "in the official helper"
        )
        return

    assert not expect_index_error, "Baseline unexpectedly passed the official helper"
    assert request_count == 1
    choice = result.choices[0]
    assert choice.finish_reason == "tool_calls", choice.finish_reason
    assert choice.message.role == "assistant", choice.message.role
    actual = [
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
    assert actual == expected, f"Tool mismatch:\nactual={actual!r}\nexpected={expected!r}"
    actual_json = [json.loads(tool["function"]["arguments"]) for tool in actual]
    assert actual_json == expected_json, (actual_json, expected_json)
    print(
        f"PASS {sse_path.name}: official helper reconstructed all {len(actual)} "
        "tool IDs, names, and exact arguments"
    )
    print(f"PASS {sse_path.name}: all argument strings are valid JSON with the expected values")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture_dir", type=Path)
    parser.add_argument("--expect-index-error", action="store_true")
    args = parser.parse_args()
    root = args.fixture_dir.resolve(strict=True)
    paths = fixture_paths(root, args.expect_index_error)
    print(f"openai={openai.__version__} httpx={httpx.__version__}")
    for path in paths:
        check_fixture(path, args.expect_index_error)
    print(f"PASS: checked {len(paths)} fixture(s); HTTP transport was fully mocked")


if __name__ == "__main__":
    main()
