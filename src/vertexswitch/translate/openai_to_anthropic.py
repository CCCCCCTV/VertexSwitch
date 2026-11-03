"""OpenAI Chat Completions ⇄ Anthropic Messages 转换（纯函数 + 流式状态机）。

覆盖：system/developer 消息、文本与图片（data: URL）、工具定义、tool_choice、
助手的 tool_calls、tool 角色消息 → tool_result、停止原因、用量，以及流式中的
文本、思考、工具调用参数（``input_json_delta``）。

参考：
- https://platform.claude.com/docs/en/api/messages/create
- https://platform.claude.com/docs/en/build-with-claude/streaming
"""

from __future__ import annotations

import codecs
import json
import re
import time
from typing import Any

from ..core.errors import ProxyError

_DATA_URL = re.compile(r"^data:(?P<media>[\w/+.-]+);base64,(?P<data>.+)$", re.DOTALL)

_FINISH_REASONS = {
    "end_turn": "stop",
    "stop_sequence": "stop",
    "pause_turn": "stop",
    "max_tokens": "length",
    "model_context_window_exceeded": "length",
    "tool_use": "tool_calls",
    "refusal": "content_filter",
}


# ---------------------------------------------------------------- 请求


def _text_of(content: Any) -> str:
    """OpenAI 的 content（字符串或分段数组）里的纯文本。"""
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "".join(
            p.get("text", "") for p in content if isinstance(p, dict) and p.get("type") == "text"
        )
    raise ProxyError(400, "messages[].content 格式不正确")


def _image_block(part: dict[str, Any]) -> dict[str, Any]:
    image = part.get("image_url")
    url = image.get("url") if isinstance(image, dict) else image
    if not isinstance(url, str):
        raise ProxyError(400, "image_url.url 缺失")
    m = _DATA_URL.match(url)
    if not m:
        # Vertex 上的 Claude 不支持 URL 图片来源，只能内嵌 base64
        raise ProxyError(
            400, "Vertex AI 上的 Claude 不支持 URL 图片，请改用 data:image/...;base64, 内嵌图片"
        )
    return {
        "type": "image",
        "source": {"type": "base64", "media_type": m["media"], "data": m["data"]},
    }


def _user_blocks(content: Any) -> list[dict[str, Any]]:
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    if not isinstance(content, list):
        raise ProxyError(400, "user 消息的 content 必须是字符串或数组")
    blocks = []
    for part in content:
        kind = part.get("type") if isinstance(part, dict) else None
        if kind == "text":
            blocks.append({"type": "text", "text": part.get("text", "")})
        elif kind == "image_url":
            blocks.append(_image_block(part))
        else:
            raise ProxyError(400, f"不支持的消息内容类型：{kind!r}")
    return blocks


def _assistant_blocks(msg: dict[str, Any]) -> list[dict[str, Any]]:
    blocks: list[dict[str, Any]] = []
    text = _text_of(msg.get("content"))
    if text:
        blocks.append({"type": "text", "text": text})
    for call in msg.get("tool_calls") or []:
        fn = call.get("function") or {}
        raw = fn.get("arguments") or "{}"
        try:
            args = json.loads(raw) if isinstance(raw, str) else raw
        except ValueError as e:
            raise ProxyError(400, f"tool_calls 的 arguments 不是合法 JSON：{e}") from e
        blocks.append(
            {"type": "tool_use", "id": call.get("id"), "name": fn.get("name"), "input": args}
        )
    return blocks


def _tool_result_block(msg: dict[str, Any]) -> dict[str, Any]:
    return {
        "type": "tool_result",
        "tool_use_id": msg.get("tool_call_id"),
        "content": _text_of(msg.get("content")),
    }


def _convert_tools(tools: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for tool in tools:
        if tool.get("type") != "function":
            raise ProxyError(400, f"不支持的工具类型：{tool.get('type')!r}")
        fn = tool.get("function") or {}
        item: dict[str, Any] = {
            "name": fn.get("name"),
            "input_schema": fn.get("parameters") or {"type": "object", "properties": {}},
        }
        if fn.get("description"):
            item["description"] = fn["description"]
        out.append(item)
    return out


def _convert_tool_choice(choice: Any, parallel: Any) -> dict[str, Any] | None:
    if choice is None:
        result = None
    elif choice == "auto":
        result = {"type": "auto"}
    elif choice == "none":
        result = {"type": "none"}
    elif choice == "required":
        result = {"type": "any"}
    elif isinstance(choice, dict) and choice.get("type") == "function":
        result = {"type": "tool", "name": (choice.get("function") or {}).get("name")}
    else:
        raise ProxyError(400, f"不支持的 tool_choice：{choice!r}")
    if parallel is False:
        result = result or {"type": "auto"}
        if result["type"] != "none":
            result["disable_parallel_tool_use"] = True
    return result


def request_to_anthropic(body: dict[str, Any], default_max_tokens: int) -> dict[str, Any]:
    """OpenAI 请求体 → Anthropic Messages 请求体（不含 model，由调用方填）。"""
    if body.get("n", 1) not in (1, None):
        raise ProxyError(400, "Claude 不支持 n > 1")
    messages = body.get("messages")
    if not isinstance(messages, list) or not messages:
        raise ProxyError(400, "messages 不能为空")

    system_parts: list[str] = []
    out_messages: list[dict[str, Any]] = []

    def append(role: str, blocks: list[dict[str, Any]]) -> None:
        if not blocks:
            return
        # Anthropic 要求 user / assistant 交替：同角色的连续消息合并
        if out_messages and out_messages[-1]["role"] == role:
            out_messages[-1]["content"].extend(blocks)
        else:
            out_messages.append({"role": role, "content": blocks})

    for msg in messages:
        if not isinstance(msg, dict):
            raise ProxyError(400, "messages[] 必须是对象")
        role = msg.get("role")
        if role in ("system", "developer"):
            system_parts.append(_text_of(msg.get("content")))
        elif role == "user":
            append("user", _user_blocks(msg.get("content")))
        elif role == "assistant":
            append("assistant", _assistant_blocks(msg))
        elif role == "tool":
            append("user", [_tool_result_block(msg)])
        else:
            raise ProxyError(400, f"不支持的消息角色：{role!r}")

    out: dict[str, Any] = {
        "messages": out_messages,
        "max_tokens": body.get("max_completion_tokens")
        or body.get("max_tokens")
        or default_max_tokens,
    }
    system = "\n\n".join(p for p in system_parts if p)
    if system:
        out["system"] = system
    for key in ("temperature", "top_p"):
        if body.get(key) is not None:
            out[key] = body[key]
    stop = body.get("stop")
    if stop:
        out["stop_sequences"] = [stop] if isinstance(stop, str) else list(stop)
    if body.get("tools"):
        out["tools"] = _convert_tools(body["tools"])
    choice = _convert_tool_choice(body.get("tool_choice"), body.get("parallel_tool_calls"))
    if choice is not None:
        out["tool_choice"] = choice
    if isinstance(body.get("user"), str):
        out["metadata"] = {"user_id": body["user"]}
    if body.get("stream"):
        out["stream"] = True
    return out


# ---------------------------------------------------------------- 非流式响应


def _openai_usage(usage: dict[str, Any] | None) -> dict[str, int]:
    usage = usage or {}
    cached = usage.get("cache_read_input_tokens") or 0
    prompt = (
        (usage.get("input_tokens") or 0) + cached + (usage.get("cache_creation_input_tokens") or 0)
    )
    completion = usage.get("output_tokens") or 0
    out = {
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
    }
    if cached:
        out["prompt_tokens_details"] = {"cached_tokens": cached}
    return out


def finish_reason(stop_reason: str | None) -> str | None:
    if stop_reason is None:
        return None
    return _FINISH_REASONS.get(stop_reason, "stop")


def response_to_openai(msg: dict[str, Any], model: str) -> dict[str, Any]:
    texts: list[str] = []
    reasoning: list[str] = []
    tool_calls: list[dict[str, Any]] = []
    for block in msg.get("content") or []:
        kind = block.get("type")
        if kind == "text":
            texts.append(block.get("text", ""))
        elif kind == "thinking":
            reasoning.append(block.get("thinking", ""))
        elif kind == "tool_use":
            tool_calls.append(
                {
                    "id": block.get("id"),
                    "type": "function",
                    "function": {
                        "name": block.get("name"),
                        "arguments": json.dumps(block.get("input") or {}, ensure_ascii=False),
                    },
                }
            )
    message: dict[str, Any] = {"role": "assistant", "content": "".join(texts) or None}
    if reasoning:
        message["reasoning_content"] = "".join(reasoning)
    if tool_calls:
        message["tool_calls"] = tool_calls
    return {
        "id": msg.get("id") or "chatcmpl-vertexswitch",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": message,
                "finish_reason": finish_reason(msg.get("stop_reason")),
            }
        ],
        "usage": _openai_usage(msg.get("usage")),
    }


# ---------------------------------------------------------------- 流式


class SSEParser:
    """把任意切分的字节流解析成 (event, data) 对。

    用增量解码器处理被切开的多字节 UTF-8 字符；每次对整个缓冲区归一化换行，
    这样被切开的 CRLF 也能正确识别。
    """

    def __init__(self) -> None:
        self._decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
        self._buffer = ""

    def feed(self, chunk: bytes) -> list[tuple[str, str]]:
        self._buffer = (self._buffer + self._decoder.decode(chunk)).replace("\r\n", "\n")
        events = []
        while "\n\n" in self._buffer:
            raw, self._buffer = self._buffer.split("\n\n", 1)
            event, data = "message", []
            for line in raw.split("\n"):
                if line.startswith("event:"):
                    event = line[6:].strip()
                elif line.startswith("data:"):
                    data.append(line[5:].lstrip())
            if data:
                events.append((event, "\n".join(data)))
        return events


class StreamTranslator:
    """Anthropic 流式事件 → OpenAI ``chat.completion.chunk``。

    工具调用参数以 ``input_json_delta`` 分段到达，按 content block 下标映射到
    OpenAI 的 ``tool_calls[].index`` 后逐段转发。
    """

    def __init__(self, model: str, include_usage: bool = False):
        self.model = model
        self.include_usage = include_usage
        self.id = "chatcmpl-vertexswitch"
        self.created = int(time.time())
        self._tool_index: dict[int, int] = {}
        self._usage: dict[str, Any] = {}
        self.finished = False

    def _chunk(self, delta: dict[str, Any], finish: str | None = None) -> dict[str, Any]:
        return {
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        }

    def handle(self, event: str, data: dict[str, Any]) -> list[dict[str, Any]]:
        kind = data.get("type", event)
        if kind == "message_start":
            msg = data.get("message") or {}
            self.id = msg.get("id") or self.id
            self._usage.update(msg.get("usage") or {})
            return [self._chunk({"role": "assistant", "content": ""})]
        if kind == "content_block_start":
            block = data.get("content_block") or {}
            if block.get("type") != "tool_use":
                return []
            index = len(self._tool_index)
            self._tool_index[data.get("index", 0)] = index
            call = {
                "index": index,
                "id": block.get("id"),
                "type": "function",
                "function": {"name": block.get("name"), "arguments": ""},
            }
            return [self._chunk({"tool_calls": [call]})]
        if kind == "content_block_delta":
            delta = data.get("delta") or {}
            dtype = delta.get("type")
            if dtype == "text_delta":
                return [self._chunk({"content": delta.get("text", "")})]
            if dtype == "thinking_delta":
                return [self._chunk({"reasoning_content": delta.get("thinking", "")})]
            if dtype == "input_json_delta":
                index = self._tool_index.get(data.get("index", 0))
                if index is None:
                    return []
                call = {"index": index, "function": {"arguments": delta.get("partial_json", "")}}
                return [self._chunk({"tool_calls": [call]})]
            return []
        if kind == "message_delta":
            self._usage.update(data.get("usage") or {})
            reason = finish_reason((data.get("delta") or {}).get("stop_reason"))
            return [self._chunk({}, finish=reason)] if reason else []
        if kind == "message_stop":
            self.finished = True
            if not self.include_usage:
                return []
            final = self._chunk({})
            final["choices"] = []
            final["usage"] = _openai_usage(self._usage)
            return [final]
        return []


def sse_data(payload: dict[str, Any] | str) -> bytes:
    text = payload if isinstance(payload, str) else json.dumps(payload, ensure_ascii=False)
    return f"data: {text}\n\n".encode()
