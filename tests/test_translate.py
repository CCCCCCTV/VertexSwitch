import json

import pytest

from vertexswitch.core.errors import ProxyError
from vertexswitch.translate.openai_to_anthropic import (
    SSEParser,
    StreamTranslator,
    request_to_anthropic,
    response_to_openai,
)

TOOL = {
    "type": "function",
    "function": {
        "name": "get_weather",
        "description": "查天气",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
    },
}


def test_basic_request():
    out = request_to_anthropic(
        {
            "model": "x",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "developer", "content": [{"type": "text", "text": "really"}]},
                {"role": "user", "content": "hi"},
            ],
            "max_completion_tokens": 50,
            "temperature": 0.2,
            "stop": "END",
            "user": "u1",
            "stream": True,
        },
        default_max_tokens=8192,
    )
    assert out == {
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "system": "be brief\n\nreally",
        "max_tokens": 50,
        "temperature": 0.2,
        "stop_sequences": ["END"],
        "metadata": {"user_id": "u1"},
        "stream": True,
    }


def test_default_max_tokens():
    out = request_to_anthropic({"messages": [{"role": "user", "content": "hi"}]}, 1234)
    assert out["max_tokens"] == 1234


def test_tools_and_tool_choice():
    out = request_to_anthropic(
        {
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [TOOL],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
            "parallel_tool_calls": False,
        },
        100,
    )
    assert out["tools"] == [
        {
            "name": "get_weather",
            "description": "查天气",
            "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}},
        }
    ]
    assert out["tool_choice"] == {
        "type": "tool",
        "name": "get_weather",
        "disable_parallel_tool_use": True,
    }


@pytest.mark.parametrize(
    ("choice", "expected"),
    [("auto", {"type": "auto"}), ("none", {"type": "none"}), ("required", {"type": "any"})],
)
def test_tool_choice_strings(choice, expected):
    out = request_to_anthropic(
        {"messages": [{"role": "user", "content": "x"}], "tools": [TOOL], "tool_choice": choice},
        100,
    )
    assert out["tool_choice"] == expected


def test_tool_call_roundtrip_history():
    out = request_to_anthropic(
        {
            "messages": [
                {"role": "user", "content": "weather in Paris and Rome?"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "get_weather", "arguments": '{"city":"Paris"}'},
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "get_weather", "arguments": '{"city":"Rome"}'},
                        },
                    ],
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
                {"role": "tool", "tool_call_id": "call_2", "content": "rain"},
                {"role": "user", "content": "thanks"},
            ]
        },
        100,
    )
    assert out["messages"] == [
        {"role": "user", "content": [{"type": "text", "text": "weather in Paris and Rome?"}]},
        {
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "get_weather",
                    "input": {"city": "Paris"},
                },
                {
                    "type": "tool_use",
                    "id": "call_2",
                    "name": "get_weather",
                    "input": {"city": "Rome"},
                },
            ],
        },
        {
            "role": "user",
            "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"},
                {"type": "tool_result", "tool_use_id": "call_2", "content": "rain"},
                {"type": "text", "text": "thanks"},
            ],
        },
    ]


def test_base64_image():
    out = request_to_anthropic(
        {
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "what is this"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
                    ],
                }
            ]
        },
        100,
    )
    assert out["messages"][0]["content"][1] == {
        "type": "image",
        "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"},
    }


@pytest.mark.parametrize(
    "body",
    [
        {"messages": []},
        {"messages": [{"role": "user", "content": "x"}], "n": 2},
        {
            "messages": [
                {
                    "role": "user",
                    "content": [{"type": "image_url", "image_url": {"url": "https://x/a.png"}}],
                }
            ]
        },
        {"messages": [{"role": "user", "content": "x"}], "tools": [{"type": "retrieval"}]},
        {"messages": [{"role": "wizard", "content": "x"}]},
    ],
)
def test_invalid_requests(body):
    with pytest.raises(ProxyError) as exc:
        request_to_anthropic(body, 100)
    assert exc.value.status == 400


def test_response_with_text_thinking_and_tools():
    msg = {
        "id": "msg_1",
        "content": [
            {"type": "thinking", "thinking": "hmm"},
            {"type": "text", "text": "Checking."},
            {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "巴黎"}},
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "cache_read_input_tokens": 5, "output_tokens": 7},
    }
    out = response_to_openai(msg, "claude-x")
    assert out["object"] == "chat.completion"
    assert out["model"] == "claude-x"
    choice = out["choices"][0]
    assert choice["finish_reason"] == "tool_calls"
    assert choice["message"]["content"] == "Checking."
    assert choice["message"]["reasoning_content"] == "hmm"
    (call,) = choice["message"]["tool_calls"]
    assert call["id"] == "toolu_1"
    assert json.loads(call["function"]["arguments"]) == {"city": "巴黎"}
    assert out["usage"] == {
        "prompt_tokens": 15,
        "completion_tokens": 7,
        "total_tokens": 22,
        "prompt_tokens_details": {"cached_tokens": 5},
    }


@pytest.mark.parametrize(
    ("stop", "finish"),
    [
        ("end_turn", "stop"),
        ("max_tokens", "length"),
        ("stop_sequence", "stop"),
        ("refusal", "content_filter"),
    ],
)
def test_finish_reasons(stop, finish):
    out = response_to_openai({"content": [], "stop_reason": stop}, "m")
    assert out["choices"][0]["finish_reason"] == finish
    assert out["choices"][0]["message"]["content"] is None


def _events():
    return [
        ("message_start", {"type": "message_start", "message": {"id": "msg_9", "usage": {"input_tokens": 12}}}),
        ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ("ping", {"type": "ping"}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Let me check."}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        ("content_block_start", {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_a", "name": "get_weather", "input": {}}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": '{"city": "Pa'}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": 'ris"}'}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 1}),
        ("content_block_start", {"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_b", "name": "get_weather", "input": {}}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": '{"city": "Rome"}'}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 2}),
        ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 30}}),
        ("message_stop", {"type": "message_stop"}),
    ]  # fmt: skip


def test_stream_translator_tool_calls():
    t = StreamTranslator("claude-x", include_usage=True)
    chunks = [c for ev, data in _events() for c in t.handle(ev, data)]
    assert t.finished
    assert all(c["id"] == "msg_9" for c in chunks)
    deltas = [c["choices"][0]["delta"] for c in chunks if c["choices"]]
    assert deltas[0] == {"role": "assistant", "content": ""}
    assert "".join(d.get("content", "") for d in deltas) == "Let me check."

    # 按 index 重新拼出每个工具调用
    calls: dict[int, dict] = {}
    for d in deltas:
        for tc in d.get("tool_calls", []):
            entry = calls.setdefault(tc["index"], {"id": None, "name": None, "args": ""})
            entry["id"] = tc.get("id") or entry["id"]
            entry["name"] = tc.get("function", {}).get("name") or entry["name"]
            entry["args"] += tc.get("function", {}).get("arguments", "")
    assert calls == {
        0: {"id": "toolu_a", "name": "get_weather", "args": '{"city": "Paris"}'},
        1: {"id": "toolu_b", "name": "get_weather", "args": '{"city": "Rome"}'},
    }
    finishes = [c["choices"][0]["finish_reason"] for c in chunks if c["choices"]]
    assert finishes[-1] == "tool_calls"
    assert chunks[-1]["choices"] == []
    assert chunks[-1]["usage"]["completion_tokens"] == 30
    assert chunks[-1]["usage"]["prompt_tokens"] == 12


def test_stream_translator_without_usage_chunk():
    t = StreamTranslator("m")
    chunks = [c for ev, data in _events() for c in t.handle(ev, data)]
    assert all(c["choices"] for c in chunks)


def test_sse_parser_handles_arbitrary_splits():
    raw = (
        b'event: message_start\r\ndata: {"a": 1}\r\n\r\n'
        b"event: ping\ndata: {}\n\n"
        b'event: content_block_delta\ndata: {"t": "\xe4\xbd\xa0"}\n\n'
    )
    for size in (1, 3, 7, len(raw)):
        p = SSEParser()
        events = []
        for i in range(0, len(raw), size):
            events.extend(p.feed(raw[i : i + size]))
        assert events == [
            ("message_start", '{"a": 1}'),
            ("ping", "{}"),
            ("content_block_delta", '{"t": "你"}'),
        ]
