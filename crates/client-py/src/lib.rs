//! Python bindings for the Rust client.
//!
//! This module is the thin native layer (`ultrafast._native`): it takes plain
//! values, runs the Rust client on a shared tokio runtime, and gives plain
//! tuples back. The typed classes, argument checks and exceptions live in the
//! Python package. The key stays inside `Target`; its `repr` is redacted, and
//! errors come from the Rust client already scrubbed of it.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::{future_into_py, get_runtime};
use ultrafast_client::types::{ChatResponse, FinishReason, StreamEvent, ToolChoice, Usage};
use ultrafast_client::{
    ChatRequest, Client as RustClient, EmbeddingsRequest, EmbeddingsResponse, Error, ErrorKind,
    Target as RustTarget,
};
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::{parse_messages, parse_response_format, parse_tools};

/// How long a blocking call waits before it lets Python handle a signal.
const SIGNAL_SLICE: Duration = Duration::from_millis(100);

/// Waits for `fut` on the shared runtime with the GIL released, in short
/// slices; between slices Python's signal handlers run, so Ctrl-C raises
/// `KeyboardInterrupt` and the call (and its request) is dropped.
fn wait<T: Send>(py: Python<'_>, fut: impl Future<Output = T> + Send) -> PyResult<T> {
    let mut fut = Box::pin(fut);
    loop {
        let slice = py.detach(|| {
            get_runtime().block_on(async { tokio::time::timeout(SIGNAL_SLICE, fut.as_mut()).await })
        });
        match slice {
            Ok(out) => return Ok(out),
            Err(_) => py.check_signals()?,
        }
    }
}

type BoxStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;

type UsageOut = Option<(u32, u32)>;
type ToolCallOut = (String, String, String);
type ChatOut = (
    String,
    String,
    String,
    Option<&'static str>,
    UsageOut,
    Vec<ToolCallOut>,
);
/// `(kind, text, finish_reason, usage, index, id, name)`; a `tool_call_delta`
/// carries its argument text in `text`.
type EventOut = (
    &'static str,
    Option<String>,
    Option<&'static str>,
    UsageOut,
    Option<u32>,
    Option<String>,
    Option<String>,
);
type EmbedOut = (String, Vec<Vec<f32>>, u32);

fn usage_out(u: Option<Usage>) -> UsageOut {
    u.map(|u| (u.input_tokens, u.output_tokens))
}

fn finish_out(f: Option<FinishReason>) -> Option<&'static str> {
    f.map(FinishReason::as_openai)
}

fn chat_out(r: ChatResponse) -> ChatOut {
    (
        r.id,
        r.model,
        r.content,
        finish_out(r.finish_reason),
        usage_out(r.usage),
        r.tool_calls
            .into_iter()
            .map(|c| (c.id, c.name, c.arguments))
            .collect(),
    )
}

fn event_out(e: StreamEvent) -> EventOut {
    match e {
        StreamEvent::Delta { text } => ("delta", Some(text), None, None, None, None, None),
        StreamEvent::ToolCallStart { index, id, name } => (
            "tool_call_start",
            None,
            None,
            None,
            Some(index),
            Some(id),
            Some(name),
        ),
        StreamEvent::ToolCallDelta { index, arguments } => (
            "tool_call_delta",
            Some(arguments),
            None,
            None,
            Some(index),
            None,
            None,
        ),
        StreamEvent::Done {
            finish_reason,
            usage,
        } => (
            "done",
            None,
            finish_out(finish_reason),
            usage_out(usage),
            None,
            None,
            None,
        ),
    }
}

fn embed_out(r: EmbeddingsResponse) -> EmbedOut {
    (r.model, r.vectors, r.prompt_tokens)
}

/// The Python exception for a client error; the classes are in `ultrafast._errors`.
fn to_py(py: Python<'_>, e: &Error) -> PyErr {
    let build = || -> PyResult<PyErr> {
        let made = py.import("ultrafast._errors")?.getattr("make")?.call1((
            e.kind.as_str(),
            e.message.as_str(),
            e.status,
            e.retryable,
            e.retry_after.map(|d| d.as_secs()),
        ))?;
        Ok(PyErr::from_value(made))
    };
    build().unwrap_or_else(|err| err)
}

fn async_err(e: Error) -> PyErr {
    Python::attach(|py| to_py(py, &e))
}

type Tags = Option<BTreeMap<String, String>>;

fn invalid(e: TranslateError) -> Error {
    match e {
        TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
            Error::new(ErrorKind::InvalidRequest, m)
        }
        other => Error::new(ErrorKind::InvalidRequest, other.to_string()),
    }
}

fn json_list(what: &str, text: &str) -> Result<Vec<serde_json::Value>, Error> {
    serde_json::from_str(text)
        .map_err(|e| Error::new(ErrorKind::InvalidRequest, format!("{what}: {e}")))
}

/// What the Python layer hands over: messages and tools as JSON text in
/// OpenAI's shape, parsed by the same code the gateway uses.
struct ToolArgs {
    tools: Option<String>,
    tool_choice: Option<String>,
    parallel_tool_calls: Option<bool>,
    response_format: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn chat_request(
    py: Python<'_>,
    model: String,
    messages: String,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    stop: Option<Vec<String>>,
    tags: Tags,
    tools: ToolArgs,
) -> PyResult<ChatRequest> {
    let build = || -> Result<ChatRequest, Error> {
        let mut req = ChatRequest::new(model);
        req.inner.messages = parse_messages(json_list("messages", &messages)?).map_err(invalid)?;
        if let Some(t) = &tools.tools {
            req.inner.tools = parse_tools(json_list("tools", t)?).map_err(invalid)?;
        }
        req.inner.tool_choice = tools.tool_choice.map(|c| match c.as_str() {
            "auto" => ToolChoice::Auto,
            "none" => ToolChoice::None,
            "required" => ToolChoice::Required,
            _ => ToolChoice::Tool(c),
        });
        req.inner.parallel_tool_calls = tools.parallel_tool_calls;
        if let Some(f) = &tools.response_format {
            let v: serde_json::Value = serde_json::from_str(f).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest, format!("response_format: {e}"))
            })?;
            req.inner.response_format = Some(parse_response_format(&v).map_err(invalid)?);
        }
        if let Some(v) = max_tokens {
            req = req.max_tokens(v);
        }
        if let Some(v) = temperature {
            req = req.temperature(v);
        }
        if let Some(v) = top_p {
            req = req.top_p(v);
        }
        if let Some(v) = stop {
            req = req.stop(v);
        }
        for (k, v) in tags.unwrap_or_default() {
            req = req.tag(k, v);
        }
        Ok(req)
    };
    build().map_err(|e| to_py(py, &e))
}

fn embed_request(
    model: String,
    input: Vec<String>,
    dimensions: Option<u32>,
    tags: Tags,
) -> EmbeddingsRequest {
    let mut req = EmbeddingsRequest::new(model, input);
    if let Some(d) = dimensions {
        req = req.dimensions(d);
    }
    for (k, v) in tags.unwrap_or_default() {
        req = req.tag(k, v);
    }
    req
}

/// Where calls go. Holds the key; never shows it.
#[pyclass(frozen, module = "ultrafast")]
struct Target(RustTarget);

#[pymethods]
impl Target {
    fn __repr__(&self) -> String {
        format!("{:?}", self.0)
    }
}

#[pyfunction]
fn gateway(base_url: String, key: String) -> Target {
    Target(RustTarget::gateway(base_url, key))
}

fn with_base(t: RustTarget, base_url: Option<String>) -> Target {
    Target(match base_url {
        Some(u) => t.with_base_url(u),
        None => t,
    })
}

#[pyfunction]
#[pyo3(signature = (key, base_url=None))]
fn openai(key: String, base_url: Option<String>) -> Target {
    with_base(RustTarget::openai(key), base_url)
}

#[pyfunction]
#[pyo3(signature = (key, base_url=None))]
fn anthropic(key: String, base_url: Option<String>) -> Target {
    with_base(RustTarget::anthropic(key), base_url)
}

#[pyfunction]
#[pyo3(signature = (key, base_url=None))]
fn gemini(key: String, base_url: Option<String>) -> Target {
    with_base(RustTarget::gemini(key), base_url)
}

#[pyfunction]
#[pyo3(signature = (endpoint, key, api_version=None))]
fn azure(endpoint: String, key: String, api_version: Option<String>) -> Target {
    let t = RustTarget::azure(endpoint, key);
    Target(match api_version {
        Some(v) => t.with_api_version(v),
        None => t,
    })
}

#[pyfunction]
fn openai_compatible(base_url: String, key: String) -> Target {
    Target(RustTarget::openai_compatible(base_url, key))
}

fn make_client(
    target: &Target,
    timeout: Option<f64>,
    max_response_bytes: Option<usize>,
) -> PyResult<RustClient> {
    let mut c = RustClient::new(target.0.clone());
    if let Some(t) = timeout {
        let d = Duration::try_from_secs_f64(t)
            .map_err(|_| PyValueError::new_err("timeout is a positive number of seconds"))?;
        c = c.with_timeout(d);
    }
    if let Some(m) = max_response_bytes {
        c = c.with_max_response_bytes(m);
    }
    Ok(c)
}

/// The stream behind a Python iterator; shared so `close` can reach it.
struct Shared {
    stream: tokio::sync::Mutex<Option<BoxStream>>,
    closed: AtomicBool,
}

impl Shared {
    fn new(stream: BoxStream) -> Arc<Shared> {
        Arc::new(Shared {
            stream: tokio::sync::Mutex::new(Some(stream)),
            closed: AtomicBool::new(false),
        })
    }

    /// The next event, `None` at the end. After an error the stream is dropped,
    /// so the error is the last thing it gives.
    async fn next(&self) -> Result<Option<EventOut>, Error> {
        let mut guard = self.stream.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            *guard = None;
        }
        let Some(stream) = guard.as_mut() else {
            return Ok(None);
        };
        match stream.next().await {
            Some(Ok(e)) => Ok(Some(event_out(e))),
            Some(Err(e)) => {
                *guard = None;
                Err(e)
            }
            None => {
                *guard = None;
                Ok(None)
            }
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.stream.try_lock() {
            *g = None;
        }
    }
}

async fn open_stream(client: RustClient, req: ChatRequest) -> Result<Arc<Shared>, Error> {
    let s = client.chat_stream(req).await?;
    Ok(Shared::new(Box::pin(s)))
}

#[pyclass(module = "ultrafast._native")]
struct Client {
    inner: RustClient,
}

#[pymethods]
impl Client {
    #[new]
    #[pyo3(signature = (target, timeout=None, max_response_bytes=None))]
    fn new(
        target: PyRef<'_, Target>,
        timeout: Option<f64>,
        max_response_bytes: Option<usize>,
    ) -> PyResult<Self> {
        Ok(Client {
            inner: make_client(&target, timeout, max_response_bytes)?,
        })
    }

    fn target_repr(&self) -> String {
        format!("{:?}", self.inner)
    }

    #[allow(clippy::too_many_arguments)]
    fn chat(
        &self,
        py: Python<'_>,
        model: String,
        messages: String,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        top_p: Option<f32>,
        stop: Option<Vec<String>>,
        tags: Tags,
        tools: Option<String>,
        tool_choice: Option<String>,
        parallel_tool_calls: Option<bool>,
        response_format: Option<String>,
    ) -> PyResult<ChatOut> {
        let req = chat_request(
            py,
            model,
            messages,
            max_tokens,
            temperature,
            top_p,
            stop,
            tags,
            ToolArgs {
                tools,
                tool_choice,
                parallel_tool_calls,
                response_format,
            },
        )?;
        let client = self.inner.clone();
        // The GIL is released while waiting on the network; signals are checked between slices.
        let r = wait(py, client.chat(req))?;
        r.map(chat_out).map_err(|e| to_py(py, &e))
    }

    #[allow(clippy::too_many_arguments)]
    fn chat_stream(
        &self,
        py: Python<'_>,
        model: String,
        messages: String,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        top_p: Option<f32>,
        stop: Option<Vec<String>>,
        tags: Tags,
        tools: Option<String>,
        tool_choice: Option<String>,
        parallel_tool_calls: Option<bool>,
        response_format: Option<String>,
    ) -> PyResult<SyncStream> {
        let req = chat_request(
            py,
            model,
            messages,
            max_tokens,
            temperature,
            top_p,
            stop,
            tags,
            ToolArgs {
                tools,
                tool_choice,
                parallel_tool_calls,
                response_format,
            },
        )?;
        let client = self.inner.clone();
        let r = wait(py, open_stream(client, req))?;
        r.map(|shared| SyncStream { shared })
            .map_err(|e| to_py(py, &e))
    }

    fn embed(
        &self,
        py: Python<'_>,
        model: String,
        input: Vec<String>,
        dimensions: Option<u32>,
        tags: Tags,
    ) -> PyResult<EmbedOut> {
        let req = embed_request(model, input, dimensions, tags);
        let client = self.inner.clone();
        let r = wait(py, client.embed(req))?;
        r.map(embed_out).map_err(|e| to_py(py, &e))
    }
}

#[pyclass(module = "ultrafast._native")]
struct SyncStream {
    shared: Arc<Shared>,
}

#[pymethods]
impl SyncStream {
    /// The next event as a tuple, or `None` at the end; raises the stream's error.
    fn next(&self, py: Python<'_>) -> PyResult<Option<EventOut>> {
        let shared = self.shared.clone();
        let r = wait(py, async move { shared.next().await })?;
        r.map_err(|e| to_py(py, &e))
    }

    fn close(&self) {
        self.shared.close();
    }
}

#[pyclass(module = "ultrafast._native")]
struct AsyncClient {
    inner: RustClient,
}

#[pymethods]
impl AsyncClient {
    #[new]
    #[pyo3(signature = (target, timeout=None, max_response_bytes=None))]
    fn new(
        target: PyRef<'_, Target>,
        timeout: Option<f64>,
        max_response_bytes: Option<usize>,
    ) -> PyResult<Self> {
        Ok(AsyncClient {
            inner: make_client(&target, timeout, max_response_bytes)?,
        })
    }

    fn target_repr(&self) -> String {
        format!("{:?}", self.inner)
    }

    #[allow(clippy::too_many_arguments)]
    fn chat<'py>(
        &self,
        py: Python<'py>,
        model: String,
        messages: String,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        top_p: Option<f32>,
        stop: Option<Vec<String>>,
        tags: Tags,
        tools: Option<String>,
        tool_choice: Option<String>,
        parallel_tool_calls: Option<bool>,
        response_format: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let req = chat_request(
            py,
            model,
            messages,
            max_tokens,
            temperature,
            top_p,
            stop,
            tags,
            ToolArgs {
                tools,
                tool_choice,
                parallel_tool_calls,
                response_format,
            },
        )?;
        let client = self.inner.clone();
        future_into_py(py, async move {
            client.chat(req).await.map(chat_out).map_err(async_err)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn chat_stream<'py>(
        &self,
        py: Python<'py>,
        model: String,
        messages: String,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        top_p: Option<f32>,
        stop: Option<Vec<String>>,
        tags: Tags,
        tools: Option<String>,
        tool_choice: Option<String>,
        parallel_tool_calls: Option<bool>,
        response_format: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let req = chat_request(
            py,
            model,
            messages,
            max_tokens,
            temperature,
            top_p,
            stop,
            tags,
            ToolArgs {
                tools,
                tool_choice,
                parallel_tool_calls,
                response_format,
            },
        )?;
        let client = self.inner.clone();
        future_into_py(py, async move {
            open_stream(client, req)
                .await
                .map(|shared| AsyncStream { shared })
                .map_err(async_err)
        })
    }

    fn embed<'py>(
        &self,
        py: Python<'py>,
        model: String,
        input: Vec<String>,
        dimensions: Option<u32>,
        tags: Tags,
    ) -> PyResult<Bound<'py, PyAny>> {
        let req = embed_request(model, input, dimensions, tags);
        let client = self.inner.clone();
        future_into_py(py, async move {
            client.embed(req).await.map(embed_out).map_err(async_err)
        })
    }
}

#[pyclass(module = "ultrafast._native")]
struct AsyncStream {
    shared: Arc<Shared>,
}

#[pymethods]
impl AsyncStream {
    /// An awaitable of the next event tuple, or `None` at the end.
    fn next<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let shared = self.shared.clone();
        future_into_py(py, async move { shared.next().await.map_err(async_err) })
    }

    fn close(&self) {
        self.shared.close();
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Target>()?;
    m.add_class::<Client>()?;
    m.add_class::<AsyncClient>()?;
    m.add_class::<SyncStream>()?;
    m.add_class::<AsyncStream>()?;
    m.add_function(wrap_pyfunction!(gateway, m)?)?;
    m.add_function(wrap_pyfunction!(openai, m)?)?;
    m.add_function(wrap_pyfunction!(anthropic, m)?)?;
    m.add_function(wrap_pyfunction!(gemini, m)?)?;
    m.add_function(wrap_pyfunction!(azure, m)?)?;
    m.add_function(wrap_pyfunction!(openai_compatible, m)?)?;
    Ok(())
}
