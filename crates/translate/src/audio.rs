//! Audio: transcription and translation (a multipart upload) and speech
//! (JSON in, binary audio out). Served by OpenAI, Azure and OpenAI-compatible
//! providers; the others have no such API this gateway speaks. Only fields
//! OpenAI documents on `POST /v1/audio/{transcriptions,translations,speech}`
//! are passed on; any other field is refused.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::provider::DEFAULT_AZURE_API_VERSION;
use crate::provider::{path_segment, provider_error, saturate, HttpRequest, ProviderKind, Target};

/// What a caller is told when the target cannot serve an audio call.
pub const NOT_SUPPORTED: &str = "This model does not support audio.";

/// OpenAI's limit on the `input` of speech, in characters.
pub const MAX_SPEECH_CHARS: usize = 4096;

impl ProviderKind {
    /// Whether the provider has audio APIs this gateway speaks.
    pub fn supports_audio(self) -> bool {
        matches!(self, ProviderKind::OpenAi | ProviderKind::Azure)
    }
}

fn invalid(m: impl Into<String>) -> TranslateError {
    TranslateError::InvalidRequest(m.into())
}

// ---------------------------------------------------------------------
// Transcription and translation
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Transcription,
    Translation,
}

impl Task {
    fn path(self) -> &'static str {
        match self {
            Task::Transcription => "audio/transcriptions",
            Task::Translation => "audio/translations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    Json,
    Text,
    VerboseJson,
    Srt,
    Vtt,
}

impl TextFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            TextFormat::Json => "json",
            TextFormat::Text => "text",
            TextFormat::VerboseJson => "verbose_json",
            TextFormat::Srt => "srt",
            TextFormat::Vtt => "vtt",
        }
    }

    /// The content type the caller is answered with.
    pub fn content_type(self) -> &'static str {
        match self {
            TextFormat::Json | TextFormat::VerboseJson => "application/json",
            TextFormat::Text => "text/plain; charset=utf-8",
            TextFormat::Srt => "application/x-subrip; charset=utf-8",
            TextFormat::Vtt => "text/vtt; charset=utf-8",
        }
    }
}

/// The file of an upload, as far as the request needs to know it. The bytes
/// are held by the gateway.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    pub content_type: String,
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscribeRequest {
    pub task: Task,
    pub model: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub response_format: TextFormat,
    pub temperature: Option<f64>,
    /// `word` and/or `segment`; needs `verbose_json`.
    pub timestamp_granularities: Vec<String>,
    pub file: FileInfo,
}

/// Parses the text fields of the multipart form of a transcription or
/// translation (everything but the file), with the file's description.
pub fn parse_transcription(
    task: Task,
    fields: &[(String, String)],
    file: FileInfo,
) -> Result<TranscribeRequest, TranslateError> {
    let mut model = None;
    let mut language = None;
    let mut prompt = None;
    let mut format = None;
    let mut temperature = None;
    let mut granularities: Vec<String> = Vec::new();
    for (name, value) in fields {
        let slot = match name.as_str() {
            "model" => &mut model,
            "language" if task == Task::Transcription => &mut language,
            "prompt" => &mut prompt,
            "response_format" => &mut format,
            "temperature" => &mut temperature,
            "timestamp_granularities" | "timestamp_granularities[]"
                if task == Task::Transcription =>
            {
                if !["word", "segment"].contains(&value.as_str()) {
                    return Err(invalid(
                        "timestamp_granularities must be one of word, segment",
                    ));
                }
                if !granularities.contains(value) {
                    granularities.push(value.clone());
                }
                continue;
            }
            // `stream: false` is the default.
            "stream" if value == "false" => continue,
            other => {
                return Err(TranslateError::Unsupported(format!(
                    "field '{other}' is not supported yet"
                )))
            }
        };
        if slot.replace(value.clone()).is_some() {
            return Err(invalid(format!("field '{name}' is given twice")));
        }
    }
    let model = model
        .filter(|m: &String| !m.is_empty())
        .ok_or_else(|| invalid("model is required"))?;
    let response_format = match format.as_deref() {
        None | Some("json") => TextFormat::Json,
        Some("text") => TextFormat::Text,
        Some("verbose_json") => TextFormat::VerboseJson,
        Some("srt") => TextFormat::Srt,
        Some("vtt") => TextFormat::Vtt,
        Some(_) => {
            return Err(invalid(
                "response_format must be one of json, text, verbose_json, srt, vtt",
            ))
        }
    };
    let temperature = match temperature.as_deref() {
        None => None,
        Some(t) => match t.trim().parse::<f64>() {
            Ok(n) if (0.0..=1.0).contains(&n) => Some(n),
            _ => return Err(invalid("temperature must be a number from 0 to 1")),
        },
    };
    if !granularities.is_empty() && response_format != TextFormat::VerboseJson {
        return Err(invalid(
            "timestamp_granularities needs response_format verbose_json",
        ));
    }
    if file.len == 0 {
        return Err(invalid("file is required and must not be empty"));
    }
    Ok(TranscribeRequest {
        task,
        model,
        language: language.filter(|l: &String| !l.is_empty()),
        prompt: prompt.filter(|p: &String| !p.is_empty()),
        response_format,
        temperature,
        timestamp_granularities: granularities,
        file,
    })
}

/// A multipart request for a provider: the headers (without a content type,
/// which carries the boundary), the text fields, and where the file goes.
/// The gateway streams the file.
#[derive(Debug, Clone, PartialEq)]
pub struct UploadRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub fields: Vec<(String, String)>,
}

fn auth_headers(target: &Target) -> Vec<(String, String)> {
    match (target.kind, &target.api_key) {
        (ProviderKind::Azure, Some(k)) => vec![("api-key".into(), k.clone())],
        (_, Some(k)) => vec![("authorization".into(), format!("Bearer {k}"))],
        _ => Vec::new(),
    }
}

/// The URL of an audio call at the target.
fn url_of(target: &Target, path: &str) -> Result<String, TranslateError> {
    let base = target.base_url.trim_end_matches('/');
    match target.kind {
        ProviderKind::OpenAi => Ok(format!("{base}/{path}")),
        ProviderKind::Azure => Ok(format!(
            "{base}/openai/deployments/{}/{path}?api-version={}",
            path_segment(&target.model),
            target
                .api_version
                .as_deref()
                .unwrap_or(DEFAULT_AZURE_API_VERSION)
        )),
        ProviderKind::Gemini | ProviderKind::Anthropic => {
            Err(TranslateError::Unsupported(NOT_SUPPORTED.into()))
        }
    }
}

pub fn build_upload(
    target: &Target,
    req: &TranscribeRequest,
) -> Result<UploadRequest, TranslateError> {
    let url = url_of(target, req.task.path())?;
    // The gpt-4o transcription models answer json or text only, and give no
    // timestamps: the caller is told before a request is sent.
    if target.model.starts_with("gpt-4o")
        && !matches!(req.response_format, TextFormat::Json | TextFormat::Text)
    {
        return Err(invalid(
            "this model supports only the json and text response formats",
        ));
    }
    if target.model.starts_with("gpt-4o") && !req.timestamp_granularities.is_empty() {
        return Err(invalid("this model does not give timestamps"));
    }
    let mut fields = Vec::new();
    if target.kind == ProviderKind::OpenAi {
        fields.push(("model".to_string(), target.model.clone()));
    }
    fields.push((
        "response_format".to_string(),
        req.response_format.as_str().to_string(),
    ));
    if let Some(l) = &req.language {
        fields.push(("language".to_string(), l.clone()));
    }
    if let Some(p) = &req.prompt {
        fields.push(("prompt".to_string(), p.clone()));
    }
    if let Some(t) = req.temperature {
        fields.push(("temperature".to_string(), t.to_string()));
    }
    for g in &req.timestamp_granularities {
        fields.push(("timestamp_granularities[]".to_string(), g.clone()));
    }
    Ok(UploadRequest {
        url,
        headers: auth_headers(target),
        fields,
    })
}

/// A piece of a subtitle file: lines that are kept as they came (numbers,
/// timings, headers, blank lines), or the spoken text of one cue, whose
/// lines are checked together (a phrase may run over a line break) and may
/// be rewritten.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Kept(String),
    Cue {
        /// The spoken lines of the cue, joined with `\n`.
        text: String,
        /// The line break after each line, as it came.
        breaks: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transcript {
    /// `json` or `verbose_json`, cut to the documented fields.
    Json(Map<String, Value>, TextFormat),
    /// `text`: the whole answer is text.
    Text(String),
    /// `srt` or `vtt`.
    Subtitles(Vec<Piece>, TextFormat),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptAnswer {
    pub transcript: Transcript,
    /// Tokens, when the provider reports them (the gpt-4o models).
    pub usage: Option<(u32, u32)>,
}

impl Transcript {
    pub fn content_type(&self) -> &'static str {
        match self {
            Transcript::Json(_, f) | Transcript::Subtitles(_, f) => f.content_type(),
            Transcript::Text(_) => TextFormat::Text.content_type(),
        }
    }

    /// Every piece of spoken text in the answer: what guardrails read and
    /// may rewrite. Never a number, a timing or a field name.
    pub fn slots(&mut self) -> Vec<&mut String> {
        match self {
            Transcript::Text(t) => vec![t],
            Transcript::Subtitles(pieces, _) => pieces
                .iter_mut()
                .filter_map(|p| match p {
                    Piece::Cue { text, .. } => Some(text),
                    Piece::Kept(_) => None,
                })
                .collect(),
            Transcript::Json(map, _) => {
                let mut out = Vec::new();
                for (k, v) in map.iter_mut() {
                    match (k.as_str(), v) {
                        ("text", Value::String(s)) => out.push(s),
                        ("segments", Value::Array(items)) => {
                            for item in items {
                                if let Some(Value::String(s)) = item.get_mut("text") {
                                    out.push(s);
                                }
                            }
                        }
                        ("words", Value::Array(items)) => {
                            for item in items {
                                if let Some(Value::String(s)) = item.get_mut("word") {
                                    out.push(s);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                out
            }
        }
    }

    /// Drops what cannot be kept in step with rewritten text: the timed
    /// words of a verbose answer (a rewrite changes the words, not their
    /// timings).
    pub fn drop_words(&mut self) {
        if let Transcript::Json(map, _) = self {
            map.remove("words");
        }
    }

    /// The body to send.
    pub fn render(&self) -> Vec<u8> {
        match self {
            Transcript::Json(map, _) => serde_json::to_vec(map).unwrap_or_default(),
            Transcript::Text(t) => t.clone().into_bytes(),
            Transcript::Subtitles(pieces, _) => {
                let mut out = String::new();
                for piece in pieces {
                    match piece {
                        Piece::Kept(t) => out.push_str(t),
                        Piece::Cue { text, breaks } => {
                            // A rewrite may have changed the number of lines;
                            // an empty line would end the cue, so none is written.
                            let lines: Vec<&str> =
                                text.split('\n').filter(|l| !l.trim().is_empty()).collect();
                            for (i, line) in lines.iter().enumerate() {
                                out.push_str(line);
                                let last = breaks.last().map_or("\n", String::as_str);
                                out.push_str(breaks.get(i).map_or(last, String::as_str));
                            }
                        }
                    }
                }
                out.into_bytes()
            }
        }
    }
}

/// The lines of a text with the break after each: `\r\n`, `\n` or a lone
/// `\r` (some providers end lines so).
fn lines_with_breaks(raw: &str) -> Vec<(&str, &str)> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push((&raw[start..i], &raw[i..=i]));
                i += 1;
                start = i;
            }
            b'\r' => {
                let end = if bytes.get(i + 1) == Some(&b'\n') {
                    i + 2
                } else {
                    i + 1
                };
                out.push((&raw[start..i], &raw[i..end]));
                i = end;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < raw.len() {
        out.push((&raw[start..], ""));
    }
    out
}

/// Splits a subtitle file into pieces. A cue begins at a line holding
/// `-->`; its text runs to the next blank line.
fn subtitle_pieces(raw: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut in_cue = false;
    let mut cue: Option<(Vec<&str>, Vec<String>)> = None;
    let flush = |cue: &mut Option<(Vec<&str>, Vec<String>)>, pieces: &mut Vec<Piece>| {
        if let Some((lines, breaks)) = cue.take() {
            pieces.push(Piece::Cue {
                text: lines.join("\n"),
                breaks,
            });
        }
    };
    for (text, eol) in lines_with_breaks(raw) {
        if text.trim().is_empty() {
            in_cue = false;
            flush(&mut cue, &mut pieces);
            pieces.push(Piece::Kept(format!("{text}{eol}")));
        } else if in_cue {
            let (lines, breaks) = cue.get_or_insert_with(|| (Vec::new(), Vec::new()));
            lines.push(text);
            breaks.push(eol.to_string());
        } else {
            if text.contains("-->") {
                in_cue = true;
            }
            pieces.push(Piece::Kept(format!("{text}{eol}")));
        }
    }
    flush(&mut cue, &mut pieces);
    pieces
}

pub fn parse_transcription_response(
    kind: ProviderKind,
    format: TextFormat,
    status: u16,
    body: &[u8],
) -> Result<TranscriptAnswer, TranslateError> {
    if !kind.supports_audio() {
        return Err(TranslateError::Unsupported(NOT_SUPPORTED.into()));
    }
    if status >= 400 {
        return Err(provider_error(status, body));
    }
    match format {
        TextFormat::Json | TextFormat::VerboseJson => {
            let v: Value = serde_json::from_slice(body)
                .map_err(|e| TranslateError::Malformed(e.to_string()))?;
            let Value::Object(wire) = v else {
                return Err(TranslateError::Malformed(
                    "response is not an object".into(),
                ));
            };
            if !wire.get("text").is_some_and(Value::is_string) {
                return Err(TranslateError::Malformed("response has no text".into()));
            }
            let mut out = Map::new();
            for k in [
                "task", "language", "duration", "text", "words", "segments", "usage",
            ] {
                if let Some(v) = wire.get(k) {
                    out.insert(k.to_string(), v.clone());
                }
            }
            let usage = out.get("usage").and_then(|u| {
                let t = u.get("input_tokens")?.as_u64()?;
                let o = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                Some((saturate(t), saturate(o)))
            });
            Ok(TranscriptAnswer {
                transcript: Transcript::Json(out, format),
                usage,
            })
        }
        TextFormat::Text | TextFormat::Srt | TextFormat::Vtt => {
            let text = String::from_utf8(body.to_vec())
                .map_err(|_| TranslateError::Malformed("response is not text".into()))?;
            let transcript = match format {
                TextFormat::Text => Transcript::Text(text),
                f => Transcript::Subtitles(subtitle_pieces(&text), f),
            };
            Ok(TranscriptAnswer {
                transcript,
                usage: None,
            })
        }
    }
}

// ---------------------------------------------------------------------
// Speech
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    /// A built-in voice name, or a custom voice as `{"id": ...}`.
    pub voice: Value,
    pub response_format: Option<String>,
    pub speed: Option<f64>,
    pub instructions: Option<String>,
}

#[derive(Deserialize)]
struct WireSpeech {
    model: Option<String>,
    input: Option<String>,
    voice: Option<Value>,
    #[serde(default)]
    response_format: Option<String>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    stream_format: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub fn parse_speech(body: &[u8]) -> Result<SpeechRequest, TranslateError> {
    let wire: WireSpeech =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if let Some((field, _)) = wire.extra.iter().find(|(_, v)| !v.is_null()) {
        return Err(TranslateError::Unsupported(format!(
            "field '{field}' is not supported yet"
        )));
    }
    // The audio itself is streamed; server-sent events are not carried.
    match wire.stream_format.as_deref() {
        None | Some("audio") => {}
        Some("sse") => {
            return Err(TranslateError::Unsupported(
                "stream_format sse is not supported yet".into(),
            ))
        }
        Some(_) => return Err(invalid("stream_format must be one of sse, audio")),
    }
    let model = wire
        .model
        .filter(|m| !m.is_empty())
        .ok_or_else(|| invalid("model is required"))?;
    let input = wire
        .input
        .filter(|i| !i.is_empty())
        .ok_or_else(|| invalid("input is required"))?;
    if input.chars().count() > MAX_SPEECH_CHARS {
        return Err(invalid(format!(
            "input must be at most {MAX_SPEECH_CHARS} characters"
        )));
    }
    let voice = match wire.voice {
        Some(Value::String(s)) if !s.is_empty() => Value::String(s),
        Some(Value::Object(o))
            if o.len() == 1
                && o.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|i| !i.is_empty()) =>
        {
            Value::Object(o)
        }
        Some(_) => {
            return Err(invalid(
                "voice must be a voice name or an object with an id",
            ))
        }
        None => return Err(invalid("voice is required")),
    };
    let response_format = match wire.response_format {
        Some(f) if !["mp3", "opus", "aac", "flac", "wav", "pcm"].contains(&f.as_str()) => {
            return Err(invalid(
                "response_format must be one of mp3, opus, aac, flac, wav, pcm",
            ))
        }
        f => f,
    };
    if let Some(s) = wire.speed {
        if !(0.25..=4.0).contains(&s) {
            return Err(invalid("speed must be between 0.25 and 4.0"));
        }
    }
    Ok(SpeechRequest {
        model,
        input,
        voice,
        response_format,
        speed: wire.speed,
        instructions: wire.instructions,
    })
}

pub fn build_speech(target: &Target, req: &SpeechRequest) -> Result<HttpRequest, TranslateError> {
    let url = url_of(target, "audio/speech")?;
    // The tts-1 models take no instructions: told before a request is sent.
    if req.instructions.is_some() && target.model.starts_with("tts-1") {
        return Err(invalid("instructions are not supported by tts-1 models"));
    }
    let mut body = Map::new();
    if target.kind == ProviderKind::OpenAi {
        body.insert("model".into(), json!(target.model));
    }
    body.insert("input".into(), json!(req.input));
    body.insert("voice".into(), req.voice.clone());
    if let Some(f) = &req.response_format {
        body.insert("response_format".into(), json!(f));
    }
    if let Some(s) = req.speed {
        body.insert("speed".into(), json!(s));
    }
    if let Some(i) = &req.instructions {
        body.insert("instructions".into(), json!(i));
    }
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    headers.extend(auth_headers(target));
    Ok(HttpRequest {
        method: "POST",
        url,
        headers,
        body: serde_json::to_vec(&Value::Object(body))
            .map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

/// The content type of audio in `format` (mp3 when none is named), for an
/// answer whose provider named none.
pub fn speech_content_type(format: Option<&str>) -> &'static str {
    match format {
        Some("opus") => "audio/opus",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("pcm") => "audio/pcm",
        _ => "audio/mpeg",
    }
}

/// The error of a failed speech call; a successful one is streamed, not
/// parsed.
pub fn parse_speech_error(kind: ProviderKind, status: u16, body: &[u8]) -> TranslateError {
    if !kind.supports_audio() {
        return TranslateError::Unsupported(NOT_SUPPORTED.into());
    }
    provider_error(status, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileInfo {
        FileInfo {
            name: "a.mp3".into(),
            content_type: "audio/mpeg".into(),
            len: 10,
        }
    }

    fn f(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn target(kind: ProviderKind, model: &str) -> Target {
        Target {
            kind,
            base_url: "https://api.example.com/v1/".into(),
            api_key: Some("sk-x".into()),
            model: model.into(),
            api_version: None,
        }
    }

    #[test]
    fn parses_every_documented_field() {
        let r = parse_transcription(
            Task::Transcription,
            &f(&[
                ("model", "whisper-1"),
                ("language", "en"),
                ("prompt", "hi"),
                ("response_format", "verbose_json"),
                ("temperature", "0.2"),
                ("timestamp_granularities[]", "word"),
                ("timestamp_granularities[]", "segment"),
                ("stream", "false"),
            ]),
            file(),
        )
        .unwrap();
        assert_eq!(r.model, "whisper-1");
        assert_eq!(r.response_format, TextFormat::VerboseJson);
        assert_eq!(r.temperature, Some(0.2));
        assert_eq!(r.timestamp_granularities, ["word", "segment"]);
    }

    #[test]
    fn refuses_what_it_does_not_carry() {
        for (name, value) in [
            ("stream", "true"),
            ("include[]", "logprobs"),
            ("chunking_strategy", "auto"),
            ("known_speaker_names[]", "a"),
            ("whatever", "1"),
        ] {
            let e = parse_transcription(
                Task::Transcription,
                &f(&[("model", "m"), (name, value)]),
                file(),
            )
            .unwrap_err();
            assert!(matches!(e, TranslateError::Unsupported(_)), "{name}");
        }
        // A translation takes no language.
        assert!(matches!(
            parse_transcription(
                Task::Translation,
                &f(&[("model", "m"), ("language", "en")]),
                file()
            ),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn checks_values() {
        let bad = |pairs: &[(&str, &str)]| {
            matches!(
                parse_transcription(Task::Transcription, &f(pairs), file()),
                Err(TranslateError::InvalidRequest(_))
            )
        };
        assert!(bad(&[]));
        assert!(bad(&[("model", "m"), ("response_format", "diarized_json")]));
        assert!(bad(&[("model", "m"), ("temperature", "1.5")]));
        assert!(bad(&[("model", "m"), ("temperature", "hot")]));
        assert!(bad(&[
            ("model", "m"),
            ("timestamp_granularities[]", "word")
        ]));
        assert!(bad(&[("model", "m"), ("model", "n")]));
        let empty = FileInfo { len: 0, ..file() };
        assert!(parse_transcription(Task::Transcription, &f(&[("model", "m")]), empty).is_err());
    }

    #[test]
    fn builds_the_upload_per_provider() {
        let req = parse_transcription(
            Task::Transcription,
            &f(&[
                ("model", "alias"),
                ("language", "de"),
                ("temperature", "0"),
                ("response_format", "verbose_json"),
                ("timestamp_granularities", "word"),
            ]),
            file(),
        )
        .unwrap();
        let u = build_upload(&target(ProviderKind::OpenAi, "whisper-1"), &req).unwrap();
        assert_eq!(u.url, "https://api.example.com/v1/audio/transcriptions");
        assert!(u
            .headers
            .contains(&("authorization".into(), "Bearer sk-x".into())));
        assert!(u.fields.contains(&("model".into(), "whisper-1".into())));
        assert!(u.fields.contains(&("language".into(), "de".into())));
        assert!(u
            .fields
            .contains(&("timestamp_granularities[]".into(), "word".into())));
        let a = build_upload(&target(ProviderKind::Azure, "my dep"), &req).unwrap();
        assert_eq!(
            a.url,
            format!(
                "https://api.example.com/v1/openai/deployments/my%20dep/audio/transcriptions?api-version={DEFAULT_AZURE_API_VERSION}"
            )
        );
        assert!(a.headers.contains(&("api-key".into(), "sk-x".into())));
        assert!(a.fields.iter().all(|(k, _)| k != "model"));
        for kind in [ProviderKind::Anthropic, ProviderKind::Gemini] {
            assert!(matches!(
                build_upload(&target(kind, "m"), &req),
                Err(TranslateError::Unsupported(_))
            ));
        }
        // gpt-4o models take json or text.
        assert!(build_upload(&target(ProviderKind::OpenAi, "gpt-4o-transcribe"), &req).is_err());
    }

    #[test]
    fn translation_goes_to_its_path() {
        let req = parse_transcription(Task::Translation, &f(&[("model", "m")]), file()).unwrap();
        let u = build_upload(&target(ProviderKind::OpenAi, "whisper-1"), &req).unwrap();
        assert!(u.url.ends_with("/audio/translations"));
    }

    #[test]
    fn json_answers_are_cut_and_expose_text_slots() {
        let body = json!({
            "task": "transcribe", "language": "en", "duration": 1.5, "text": "call me",
            "segments": [{"id": 0, "start": 0.0, "end": 1.0, "text": " call me", "tokens": [1]}],
            "words": [{"word": "call", "start": 0.0, "end": 0.5}],
            "usage": {"type": "tokens", "input_tokens": 14, "output_tokens": 3, "total_tokens": 17},
            "secret_extra": "x"
        });
        let mut a = parse_transcription_response(
            ProviderKind::OpenAi,
            TextFormat::VerboseJson,
            200,
            body.to_string().as_bytes(),
        )
        .unwrap();
        assert_eq!(a.usage, Some((14, 3)));
        assert_eq!(a.transcript.slots().len(), 3);
        for s in a.transcript.slots() {
            *s = "X".into();
        }
        a.transcript.drop_words();
        let v: Value = serde_json::from_slice(&a.transcript.render()).unwrap();
        assert_eq!(v["text"], "X");
        assert_eq!(v["segments"][0]["text"], "X");
        assert_eq!(v["segments"][0]["start"], 0.0);
        assert!(v.get("words").is_none() && v.get("secret_extra").is_none());
    }

    #[test]
    fn subtitles_expose_the_spoken_text_of_each_cue_whole() {
        let srt = "1\r\n00:00:00,000 --> 00:00:01,000\r\nCall 555\r\nsecond line\r\n\r\n2\r\n00:00:01,000 --> 00:00:02,000\r\nbye\r\n";
        let mut a = parse_transcription_response(
            ProviderKind::OpenAi,
            TextFormat::Srt,
            200,
            srt.as_bytes(),
        )
        .unwrap();
        assert_eq!(a.transcript.render(), srt.as_bytes());
        let slots = a.transcript.slots();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0], "Call 555\nsecond line");
        for s in slots {
            *s = s.replace("555", "[X]");
        }
        let out = String::from_utf8(a.transcript.render()).unwrap();
        assert!(
            out.contains("Call [X]\r\nsecond line\r\n")
                && out.contains("00:00:00,000 --> 00:00:01,000\r\n")
        );
        let vtt = "WEBVTT\n\nNOTE x\n\nid1\n00:00.000 --> 00:01.000\nhello\n";
        let mut a = parse_transcription_response(
            ProviderKind::OpenAi,
            TextFormat::Vtt,
            200,
            vtt.as_bytes(),
        )
        .unwrap();
        assert_eq!(a.transcript.slots().len(), 1);
        assert_eq!(a.transcript.render(), vtt.as_bytes());
    }

    #[test]
    fn a_phrase_over_a_line_break_is_seen_whole_and_a_rewrite_keeps_the_cue_valid() {
        let srt = "1\n00:00:00,000 --> 00:00:01,000\ncard 4111 1111\n1111 1111 ok\n\n2\n00:00:01,000 --> 00:00:02,000\nnext\n";
        let mut a = parse_transcription_response(
            ProviderKind::OpenAi,
            TextFormat::Srt,
            200,
            srt.as_bytes(),
        )
        .unwrap();
        // One match over the break, replaced by one line, and a replacement
        // that leaves an empty line is not written as a blank line.
        *a.transcript.slots()[0] = "card [CARD] ok".into();
        *a.transcript.slots()[1] = "\n".into();
        let out = String::from_utf8(a.transcript.render()).unwrap();
        assert_eq!(
            out,
            "1\n00:00:00,000 --> 00:00:01,000\ncard [CARD] ok\n\n2\n00:00:01,000 --> 00:00:02,000\n"
        );
    }

    #[test]
    fn a_lone_carriage_return_is_a_line_break() {
        let srt = "1\r00:00:00,000 --> 00:00:01,000\rmail a@b.co\r\r2\r00:00:01,000 --> 00:00:02,000\rbye\r";
        let mut a = parse_transcription_response(
            ProviderKind::OpenAi,
            TextFormat::Srt,
            200,
            srt.as_bytes(),
        )
        .unwrap();
        let slots = a.transcript.slots();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0], "mail a@b.co");
        *a.transcript.slots()[0] = "mail [EMAIL]".into();
        assert_eq!(
            String::from_utf8(a.transcript.render()).unwrap(),
            "1\r00:00:00,000 --> 00:00:01,000\rmail [EMAIL]\r\r2\r00:00:01,000 --> 00:00:02,000\rbye\r"
        );
    }

    #[test]
    fn answers_are_checked() {
        let p = |fmt, status, body: &str| {
            parse_transcription_response(ProviderKind::OpenAi, fmt, status, body.as_bytes())
        };
        assert!(matches!(
            p(TextFormat::Json, 200, "not json"),
            Err(TranslateError::Malformed(_))
        ));
        assert!(matches!(
            p(TextFormat::Json, 200, "{}"),
            Err(TranslateError::Malformed(_))
        ));
        assert!(matches!(
            p(TextFormat::Json, 500, "{}"),
            Err(TranslateError::Provider { .. })
        ));
        assert!(p(TextFormat::Text, 200, "hello").is_ok());
        assert!(matches!(
            parse_transcription_response(ProviderKind::Gemini, TextFormat::Json, 200, b"{}"),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn parses_speech() {
        let r = parse_speech(
            br#"{"model":"tts-1","input":"hi","voice":"alloy","response_format":"wav","speed":1.5,"stream_format":"audio"}"#,
        )
        .unwrap();
        assert_eq!(r.speed, Some(1.5));
        let custom = parse_speech(br#"{"model":"m","input":"hi","voice":{"id":"voice_1"}}"#);
        assert!(custom.is_ok());
        for body in [
            r#"{"input":"hi","voice":"a"}"#,
            r#"{"model":"m","voice":"a"}"#,
            r#"{"model":"m","input":"hi"}"#,
            r#"{"model":"m","input":"hi","voice":"a","speed":5}"#,
            r#"{"model":"m","input":"hi","voice":"a","response_format":"ogg"}"#,
            r#"{"model":"m","input":"hi","voice":{"id":"a","x":1}}"#,
            r#"{"model":"m","input":"hi","voice":7}"#,
        ] {
            assert!(
                matches!(
                    parse_speech(body.as_bytes()),
                    Err(TranslateError::InvalidRequest(_))
                ),
                "{body}"
            );
        }
        let long = format!(
            r#"{{"model":"m","input":"{}","voice":"a"}}"#,
            "a".repeat(MAX_SPEECH_CHARS + 1)
        );
        assert!(parse_speech(long.as_bytes()).is_err());
        for body in [
            r#"{"model":"m","input":"hi","voice":"a","stream_format":"sse"}"#,
            r#"{"model":"m","input":"hi","voice":"a","stream":true}"#,
        ] {
            assert!(matches!(
                parse_speech(body.as_bytes()),
                Err(TranslateError::Unsupported(_))
            ));
        }
    }

    #[test]
    fn builds_speech() {
        let r = parse_speech(
            br#"{"model":"alias","input":"hi","voice":"alloy","speed":2,"instructions":"calm"}"#,
        )
        .unwrap();
        let h = build_speech(&target(ProviderKind::OpenAi, "gpt-4o-mini-tts"), &r).unwrap();
        assert_eq!(h.url, "https://api.example.com/v1/audio/speech");
        let v: Value = serde_json::from_slice(&h.body).unwrap();
        assert_eq!(
            v,
            json!({"model":"gpt-4o-mini-tts","input":"hi","voice":"alloy","speed":2.0,"instructions":"calm"})
        );
        let a = build_speech(&target(ProviderKind::Azure, "tts"), &r).unwrap();
        assert!(a
            .url
            .contains("/openai/deployments/tts/audio/speech?api-version="));
        assert!(serde_json::from_slice::<Value>(&a.body)
            .unwrap()
            .get("model")
            .is_none());
        assert!(build_speech(&target(ProviderKind::OpenAi, "tts-1-hd"), &r).is_err());
        assert!(build_speech(&target(ProviderKind::Gemini, "m"), &r).is_err());
        assert_eq!(speech_content_type(Some("wav")), "audio/wav");
        assert_eq!(speech_content_type(None), "audio/mpeg");
    }
}
