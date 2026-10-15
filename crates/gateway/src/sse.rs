//! SSE stream metering: pass bytes through while extracting token usage.
//!
//! Behavior (confirmed M1 kickoff decision):
//! - The gateway asks upstream for `stream_options.include_usage` when the
//!   client did not ask for it itself. In that case (and only that case) the
//!   usage-only final chunk is **stripped** from the downstream stream so the
//!   client sees exactly what OpenAI would have sent.
//! - If the client requested `include_usage` itself, every byte — including
//!   the usage chunk — is forwarded untouched.
//! - Fallback (upstream never sends usage): `completion_tokens` is
//!   approximated by the number of content chunks seen (each OpenAI content
//!   delta is ~1 token) and `prompt_tokens` stays 0; `exact` marks the
//!   difference so callers/tests can tell.

/// Token usage extracted from a stream (or approximated when absent upstream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageOutcome {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// Model name from the first chunk that carries one.
    pub model: Option<String>,
    /// `true` when an explicit upstream `usage` object was seen.
    pub exact: bool,
}

/// Incremental SSE event filter/meter. Feed it raw upstream bytes; it returns
/// the bytes that must be forwarded downstream.
pub struct SseMeter {
    /// Bytes not yet forming a complete line (chunk boundaries).
    pending: Vec<u8>,
    /// Raw bytes of the SSE event currently being accumulated.
    event: Vec<u8>,
    /// Strip the usage-only chunk because *we* injected `include_usage`.
    strip_injected_usage: bool,
    prompt_tokens: i64,
    completion_tokens: i64,
    fallback_chunks: i64,
    model: Option<String>,
    exact: bool,
    /// Set when the current event should be dropped at its boundary.
    drop_current: bool,
}

impl SseMeter {
    pub fn new(strip_injected_usage: bool) -> Self {
        Self {
            pending: Vec::new(),
            event: Vec::new(),
            strip_injected_usage,
            prompt_tokens: 0,
            completion_tokens: 0,
            fallback_chunks: 0,
            model: None,
            exact: false,
            drop_current: false,
        }
    }

    /// Feed a raw upstream chunk; returns the bytes to forward downstream.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            self.handle_line(&line, &mut out);
        }
        out
    }

    /// Call once at end-of-stream; returns final bytes to forward.
    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.event.extend_from_slice(&line);
            if let Some(payload) = data_payload(&line) {
                let payload = payload.to_vec();
                self.ingest_payload(&payload);
            }
        }
        if !self.drop_current {
            out.extend_from_slice(&self.event);
        }
        self.event.clear();
        self.drop_current = false;
        out
    }

    /// Final usage (exact when seen upstream, else the chunk-count fallback).
    pub fn outcome(&self) -> UsageOutcome {
        if self.exact {
            UsageOutcome {
                prompt_tokens: self.prompt_tokens,
                completion_tokens: self.completion_tokens,
                model: self.model.clone(),
                exact: true,
            }
        } else {
            UsageOutcome {
                prompt_tokens: 0,
                completion_tokens: self.fallback_chunks,
                model: self.model.clone(),
                exact: false,
            }
        }
    }

    fn handle_line(&mut self, raw_with_nl: &[u8], out: &mut Vec<u8>) {
        let raw = &raw_with_nl[..raw_with_nl.len() - 1]; // strip the \n
        let is_blank = raw.is_empty() || raw == b"\r";
        self.event.extend_from_slice(raw_with_nl);
        if is_blank {
            // Event boundary: emit unless marked for dropping.
            if !self.drop_current {
                out.extend_from_slice(&self.event);
            }
            self.event.clear();
            self.drop_current = false;
        } else if let Some(payload) = data_payload(raw) {
            let payload = payload.to_vec();
            self.ingest_payload(&payload);
        }
    }

    fn ingest_payload(&mut self, payload: &[u8]) {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) else {
            return; // non-JSON data line: forward, ignore for metering
        };
        if self.model.is_none()
            && let Some(model) = value.get("model").and_then(|m| m.as_str())
        {
            self.model = Some(model.to_string());
        }
        if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
            if let Some(p) = usage.get("prompt_tokens").and_then(|v| v.as_i64()) {
                self.prompt_tokens = p;
            }
            if let Some(c) = usage.get("completion_tokens").and_then(|v| v.as_i64()) {
                self.completion_tokens = c;
            }
            self.exact = true;
            let empty_choices = value
                .get("choices")
                .and_then(|c| c.as_array())
                .is_none_or(|a| a.is_empty());
            if self.strip_injected_usage && empty_choices {
                // Usage-only chunk we provoked ourselves: meter it, drop it.
                self.drop_current = true;
            }
        } else if !self.exact {
            // Fallback counting: one content delta ≈ one completion token.
            if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    let has_content = choice
                        .pointer("/delta/content")
                        .and_then(|c| c.as_str())
                        .is_some_and(|s| !s.is_empty());
                    if has_content {
                        self.fallback_chunks += 1;
                    }
                }
            }
        }
    }
}

/// Returns the payload of an SSE `data:` line (single optional space after the
/// colon per the SSE spec; trailing `\r` tolerated for CRLF streams).
fn data_payload(line: &[u8]) -> Option<&[u8]> {
    let rest = line.strip_prefix(b"data:")?;
    let rest = rest.strip_prefix(b" ").unwrap_or(rest);
    let rest = rest.strip_suffix(b"\r").unwrap_or(rest);
    Some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_CHUNKS: &str = "\
data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"}}]}\n\
\n\
data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"}}]}\n\
\n";

    const USAGE_CHUNK: &str = "\
data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"model\":\"m-test\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\
\n";

    const DONE: &str = "data: [DONE]\n\n";

    fn full_stream(with_usage: bool) -> String {
        let mut s = TWO_CHUNKS.to_string();
        if with_usage {
            s.push_str(USAGE_CHUNK);
        }
        s.push_str(DONE);
        s
    }

    #[test]
    fn strips_usage_chunk_only_when_injected() {
        let input = full_stream(true);

        // Gateway injected include_usage → usage chunk must not reach client.
        let mut meter = SseMeter::new(true);
        let out = meter.feed(input.as_bytes());
        let out = [out, meter.finish()].concat();
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("usage"), "injected usage chunk leaked: {out}");
        assert!(out.contains("[DONE]"));
        assert!(out.contains("Hel") && out.contains("lo"));
        assert_eq!(
            meter.outcome(),
            UsageOutcome {
                prompt_tokens: 9,
                completion_tokens: 3,
                model: Some("m-test".into()),
                exact: true,
            }
        );

        // Client asked for usage itself → byte-identical passthrough.
        let mut meter = SseMeter::new(false);
        let out = meter.feed(input.as_bytes());
        let out = [out, meter.finish()].concat();
        assert_eq!(out, input.as_bytes());
        assert_eq!(
            meter.outcome(),
            UsageOutcome {
                prompt_tokens: 9,
                completion_tokens: 3,
                model: Some("m-test".into()),
                exact: true,
            }
        );
    }

    #[test]
    fn chunk_boundaries_do_not_matter() {
        let input = full_stream(true);
        let mut meter = SseMeter::new(true);
        let mut collected = Vec::new();
        // Feed one byte at a time to exercise mid-line splits.
        for byte in input.bytes() {
            collected.extend(meter.feed(&[byte]));
        }
        collected.extend(meter.finish());
        let one_shot_input = input.clone();
        let expected: Vec<u8> = {
            let mut m = SseMeter::new(true);
            let out = m.feed(one_shot_input.as_bytes());
            [out, m.finish()].concat()
        };
        assert_eq!(collected, expected);
        assert!(!std::str::from_utf8(&collected).unwrap().contains("usage"));
    }

    #[test]
    fn fallback_counts_content_chunks_when_no_usage() {
        let input = full_stream(false);
        let mut meter = SseMeter::new(true);
        let _ = meter.feed(input.as_bytes());
        let _ = meter.finish();
        assert_eq!(
            meter.outcome(),
            UsageOutcome {
                prompt_tokens: 0,
                completion_tokens: 2,
                model: Some("m-test".into()),
                exact: false,
            }
        );
    }

    #[test]
    fn usage_chunk_with_choices_is_never_dropped() {
        // Anthropic-style: usage embedded in a chunk that still carries content.
        let input = format!(
            "{TWO_CHUNKS}data: {{\"id\":\"c1\",\"model\":\"m-test\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":5,\"completion_tokens\":2}}}}\n\n{DONE}"
        );
        let mut meter = SseMeter::new(true);
        let out = meter.feed(input.as_bytes());
        let out = [out, meter.finish()].concat();
        assert!(String::from_utf8(out).unwrap().contains("usage"));
        assert_eq!(meter.outcome().completion_tokens, 2);
        assert!(meter.outcome().exact);
    }

    #[test]
    fn crlf_streams_are_forwarded_verbatim() {
        let lf = full_stream(true).replace("\n", "\r\n");
        let mut meter = SseMeter::new(false);
        let out = meter.feed(lf.as_bytes());
        let out = [out, meter.finish()].concat();
        assert_eq!(out, lf.as_bytes());
        assert_eq!(meter.outcome().prompt_tokens, 9);
    }

    #[test]
    fn trailing_partial_line_is_flushed() {
        let input = format!(
            "{TWO_CHUNKS}data: {{\"model\":\"m2\",\"choices\":[],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":1}}}}"
        );
        let mut meter = SseMeter::new(true);
        let out = meter.feed(input.as_bytes());
        let out = [out, meter.finish()].concat();
        // Usage-only event without trailing newline: still dropped + metered.
        assert!(!String::from_utf8(out).unwrap().contains("usage"));
        assert_eq!(meter.outcome().model.as_deref(), Some("m-test"));
        assert_eq!(meter.outcome().completion_tokens, 1);
    }

    #[test]
    fn non_json_data_lines_pass_through() {
        let input = "data: not-json\n\ndata: [DONE]\n\n";
        let mut meter = SseMeter::new(true);
        let out = meter.feed(input.as_bytes());
        let out = [out, meter.finish()].concat();
        assert_eq!(out, input.as_bytes());
        assert!(!meter.outcome().exact);
    }
}
