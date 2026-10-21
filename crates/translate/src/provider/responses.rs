//! Reading an OpenAI Responses stream (and a whole Responses answer) from
//! OpenAI or Azure into the common stream events.

use serde_json::Value;

use super::{saturate, with_calls, StreamState};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{FinishReason, StreamEvent, Usage};

/// `usage` of a Responses object: `input_tokens` includes the cached ones, so
/// the uncached part is the difference.
fn usage_of(u: &Value) -> Option<Usage> {
    if !u.is_object() {
        return None;
    }
    let cached = u["input_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    let input = u["input_tokens"].as_u64().unwrap_or(0);
    Some(Usage {
        input_tokens: saturate(input.saturating_sub(cached)),
        output_tokens: saturate(u["output_tokens"].as_u64().unwrap_or(0)),
        cache_read_input_tokens: saturate(cached),
        cache_write_input_tokens: 0,
    })
}

/// Usage of a whole (non-stream) Responses answer.
pub fn parse_responses_usage(body: &[u8]) -> Option<Usage> {
    let v: Value = serde_json::from_slice(body).ok()?;
    usage_of(&v["usage"])
}

fn failure(v: &Value) -> TranslateError {
    let e = if v["error"].is_object() {
        &v["error"]
    } else if v["response"]["error"].is_object() {
        &v["response"]["error"]
    } else {
        v
    };
    let message = e["message"]
        .as_str()
        .or_else(|| v["message"].as_str())
        .unwrap_or("the provider reported a failed response")
        .to_string();
    let credential = e["code"] == "invalid_api_key";
    TranslateError::Provider {
        status: if credential { 401 } else { 502 },
        retryable: false,
        message,
    }
}

fn finish_of(response: &Value, completed: bool) -> Option<FinishReason> {
    if completed {
        return Some(FinishReason::Stop);
    }
    match response["incomplete_details"]["reason"].as_str() {
        Some("max_output_tokens") => Some(FinishReason::Length),
        Some("content_filter") => Some(FinishReason::ContentFilter),
        _ => Some(FinishReason::Stop),
    }
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    if ev.data == "[DONE]" {
        return Ok(());
    }
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let kind = v["type"].as_str().or(ev.event.as_deref());
    match kind {
        Some("response.output_text.delta") | Some("response.refusal.delta") => {
            if let Some(t) = v["delta"].as_str().filter(|t| !t.is_empty()) {
                out.push(StreamEvent::Delta {
                    text: t.to_string(),
                });
            }
        }
        Some("response.output_item.added") => {
            let item = &v["item"];
            if matches!(
                item["type"].as_str(),
                Some("function_call") | Some("custom_tool_call")
            ) {
                let output_index = v["output_index"].as_u64().unwrap_or(0);
                let index = u32::try_from(state.tool_blocks.len())
                    .map_err(|_| TranslateError::Malformed("too many tool calls".into()))?;
                state.tool_blocks.push((output_index, index));
                let id = item["call_id"]
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .unwrap_or_default();
                out.push(StreamEvent::ToolCallStart {
                    index,
                    id: id.to_string(),
                    name: item["name"].as_str().unwrap_or_default().to_string(),
                });
            }
        }
        Some("response.function_call_arguments.delta")
        | Some("response.custom_tool_call_input.delta") => {
            let at = v["output_index"].as_u64().unwrap_or(0);
            let Some(index) = state
                .tool_blocks
                .iter()
                .find(|(b, _)| *b == at)
                .map(|(_, i)| *i)
            else {
                return Err(TranslateError::Malformed(
                    "tool call delta before its start".into(),
                ));
            };
            if let Some(d) = v["delta"].as_str().filter(|d| !d.is_empty()) {
                out.push(StreamEvent::ToolCallDelta {
                    index,
                    arguments: d.to_string(),
                });
            }
        }
        Some("response.completed") | Some("response.incomplete") => {
            let completed = kind == Some("response.completed");
            let usage = usage_of(&v["response"]["usage"]);
            let finish = with_calls(
                finish_of(&v["response"], completed),
                !state.tool_blocks.is_empty(),
            );
            out.push(StreamEvent::Done {
                finish_reason: finish,
                usage,
            });
        }
        Some("response.failed") | Some("error") => return Err(failure(&v)),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::TranslateError;
    use crate::provider::*;
    use crate::types::*;

    const STREAM: &str = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"hi\"}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"f\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{}\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"input_tokens_details\":{\"cached_tokens\":0}}}}\n\n",
    );

    fn expected() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "hi".into() },
            StreamEvent::ToolCallStart {
                index: 0,
                id: "c1".into(),
                name: "f".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "{}".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                    ..Default::default()
                }),
            },
        ]
    }

    #[test]
    fn decodes_text_tools_and_usage() {
        let mut d = StreamDecoder::responses();
        assert_eq!(d.feed(STREAM.as_bytes()).unwrap(), expected());
    }

    #[test]
    fn decodes_identically_at_every_split_point() {
        let bytes = STREAM.as_bytes();
        for i in 0..=bytes.len() {
            let mut d = StreamDecoder::responses();
            let mut got = d.feed(&bytes[..i]).unwrap();
            got.extend(d.feed(&bytes[i..]).unwrap());
            assert_eq!(got, expected(), "split at byte {i}");
        }
    }

    #[test]
    fn a_custom_tool_call_streams_its_input() {
        let mut d = StreamDecoder::responses();
        let got = d
            .feed(
                concat!(
                    "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"custom_tool_call\",\"call_id\":\"c9\",\"name\":\"apply_patch\",\"input\":\"\"}}\n\n",
                    "event: response.custom_tool_call_input.delta\ndata: {\"type\":\"response.custom_tool_call_input.delta\",\"output_index\":0,\"delta\":\"*** Begin\"}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c9".into(),
                    name: "apply_patch".into()
                },
                StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments: "*** Begin".into()
                },
            ]
        );
    }

    #[test]
    fn cached_tokens_are_cache_reads_and_incomplete_is_length() {
        let mut d = StreamDecoder::responses();
        let got = d
            .feed(b"event: response.incomplete\ndata: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":120,\"output_tokens\":5,\"input_tokens_details\":{\"cached_tokens\":100}}}}\n\n")
            .unwrap();
        assert_eq!(
            got,
            vec![StreamEvent::Done {
                finish_reason: Some(FinishReason::Length),
                usage: Some(Usage {
                    input_tokens: 20,
                    output_tokens: 5,
                    cache_read_input_tokens: 100,
                    cache_write_input_tokens: 0
                }),
            }]
        );
    }

    #[test]
    fn a_failed_response_is_an_upstream_error() {
        let mut d = StreamDecoder::responses();
        let e = d
            .feed(b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\",\"message\":\"boom\"}}}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Provider { message, .. } if message == "boom"));
        let mut d = StreamDecoder::responses();
        let e = d
            .feed(b"event: error\ndata: {\"type\":\"error\",\"message\":\"bad\"}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Provider { message, .. } if message == "bad"));
    }

    #[test]
    fn whole_answer_usage() {
        let body = br#"{"id":"resp_1","status":"completed","usage":{"input_tokens":120,"output_tokens":5,"input_tokens_details":{"cached_tokens":100}}}"#;
        assert_eq!(
            parse_responses_usage(body),
            Some(Usage {
                input_tokens: 20,
                output_tokens: 5,
                cache_read_input_tokens: 100,
                cache_write_input_tokens: 0
            })
        );
        assert_eq!(parse_responses_usage(b"{}"), None);
        assert_eq!(parse_responses_usage(b"nope"), None);
    }
}
