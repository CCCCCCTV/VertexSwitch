//! 上游 SSE 字节流的解析（后端模块把上游的流式回复拆成事件交给翻译器时用）。

/// 把任意切分的字节流解析成 (event, data) 对。
///
/// 在字节层面找事件边界（完整的事件不会切开多字节 UTF-8 字符）；每次对整个缓冲区把 CRLF
/// 归一为 LF，这样被切开的 CRLF 也能正确识别。
#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.windows(2).any(|w| w == b"\r\n") {
            let mut normalized = Vec::with_capacity(self.buffer.len());
            let mut i = 0;
            while i < self.buffer.len() {
                if self.buffer[i] == b'\r' && self.buffer.get(i + 1) == Some(&b'\n') {
                    i += 1;
                    continue;
                }
                normalized.push(self.buffer[i]);
                i += 1;
            }
            self.buffer = normalized;
        }
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let raw: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            let raw = String::from_utf8_lossy(&raw[..pos]);
            let mut event = "message".to_owned();
            let mut data: Vec<&str> = Vec::new();
            for line in raw.split('\n') {
                if let Some(rest) = line.strip_prefix("event:") {
                    event = rest.trim().to_owned();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data.push(rest.trim_start());
                }
            }
            if !data.is_empty() {
                events.push((event, data.join("\n")));
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_arbitrary_splits() {
        let raw = "event: message_start\r\ndata: {\"a\": 1}\r\n\r\nevent: ping\ndata: {}\n\nevent: content_block_delta\ndata: {\"t\": \"你\"}\n\n".as_bytes();
        for size in [1, 3, 7, raw.len()] {
            let mut p = SseParser::default();
            let events: Vec<(String, String)> = raw.chunks(size).flat_map(|c| p.feed(c)).collect();
            assert_eq!(
                events,
                vec![
                    ("message_start".into(), "{\"a\": 1}".into()),
                    ("ping".into(), "{}".into()),
                    ("content_block_delta".into(), "{\"t\": \"你\"}".into()),
                ],
                "size {size}"
            );
        }
    }
}
