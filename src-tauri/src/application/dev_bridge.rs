//! 开发桥接：让普通浏览器里运行的前端调用真实的 Rust 命令（仅 `dev-bridge` feature）。
//!
//! 做法：本机 HTTP 服务把浏览器的 invoke 请求构造成 Tauri 的 [`InvokeRequest`]，交给主窗口的
//! [`tauri::WebviewWindow::on_message`]——这是 Tauri 留给“自定义 invoke 系统”的公开入口，
//! 走的就是 `generate_handler!` 里注册的全部命令（含 State 注入、权限检查与插件命令），
//! 所以新增命令不需要在这里做任何事。事件方向反过来：浏览器订阅某个事件名时，服务端
//! 用 `listen_any` 监听，再经 SSE 推给浏览器。
//!
//! 安全：只绑定 127.0.0.1；每个请求必须带启动时随机生成的令牌（头 `x-dev-bridge-token`）。
//! 端口与令牌写入项目根目录的 `dev-bridge.local.json`（gitignore），由 Vite 开发服务器读取并代理
//! 浏览器的请求，令牌不会出现在浏览器里。绝不能进入 release 构建。

use std::{
    collections::{hash_map::RandomState, HashSet},
    hash::{BuildHasher, Hasher},
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use tauri::{
    ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody},
    webview::InvokeRequest,
    AppHandle, Listener, Manager,
};
use tokio::sync::{broadcast, oneshot};

const TOKEN_HEADER: &str = "x-dev-bridge-token";
/// 单个命令等待响应的上限
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

struct Bridge {
    app: AppHandle,
    token: String,
    events: broadcast::Sender<String>,
    subscribed: Mutex<HashSet<String>>,
}

/// 启动桥接服务（在 setup 里调用）。失败只记日志，不影响应用。
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run(app).await {
            log::error!("[dev-bridge] failed to start: {error}");
        }
    });
}

/// 随机令牌：RandomState 由系统熵播种，两次哈希拼成 128 位
fn random_token() -> String {
    let part = || RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", part(), part())
}

/// 项目根目录（编译期固定；dev-bridge 只用于源码树里的开发）
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

async fn run(app: AppHandle) -> Result<(), String> {
    let token = random_token();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let addr: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
    let (events, _) = broadcast::channel(256);
    let bridge = Arc::new(Bridge {
        app,
        token: token.clone(),
        events,
        subscribed: Mutex::new(HashSet::new()),
    });
    let router = Router::new()
        .route("/ping", get(ping))
        .route("/ipc/{cmd}", post(ipc))
        .route("/subscribe/{event}", post(subscribe))
        .route("/events", get(events_stream))
        .route("/logs", get(logs))
        .layer(middleware::from_fn_with_state(bridge.clone(), auth))
        .with_state(bridge);

    let info = json!({ "port": addr.port(), "token": token, "pid": std::process::id() });
    let info_path = project_root().join("dev-bridge.local.json");
    std::fs::write(&info_path, info.to_string()).map_err(|e| e.to_string())?;
    log::warn!("[dev-bridge] listening on http://{addr} (info: {})", info_path.display());
    axum::serve(listener, router).await.map_err(|e| e.to_string())
}

async fn auth(State(bridge): State<Arc<Bridge>>, request: Request, next: Next) -> Response {
    let ok = request
        .headers()
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == bridge.token);
    if ok {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "bad or missing dev bridge token").into_response()
    }
}

async fn ping() -> impl IntoResponse {
    axum::Json(json!({ "ok": true, "pid": std::process::id() }))
}

fn error_response(status: StatusCode, value: Value) -> Response {
    let mut response = (status, value.to_string()).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response
        .headers_mut()
        .insert("x-ipc-error", "1".parse().unwrap());
    response
}

/// 转发一次 invoke：响应成功时按 Tauri 的原样返回 JSON 或二进制，失败时带 x-ipc-error 头
async fn ipc(
    State(bridge): State<Arc<Bridge>>,
    Path(cmd): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(window) = bridge.app.get_webview_window("main") else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, json!("main window not found"));
    };
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    let invoke_body = if is_json {
        match serde_json::from_slice(&body) {
            Ok(value) => InvokeBody::Json(value),
            Err(error) => {
                return error_response(StatusCode::BAD_REQUEST, json!(error.to_string()));
            }
        }
    } else {
        InvokeBody::Raw(body.to_vec())
    };
    let url = window
        .url()
        .unwrap_or_else(|_| "http://localhost:1420/".parse().unwrap());
    let request = InvokeRequest {
        cmd,
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url,
        body: invoke_body,
        headers,
        invoke_key: bridge.app.invoke_key().to_string(),
    };
    let (tx, rx) = oneshot::channel();
    window.on_message(
        request,
        Box::new(move |_webview, _cmd, response, _callback, _error| {
            let _ = tx.send(response);
        }),
    );
    match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
        Ok(Ok(InvokeResponse::Ok(InvokeResponseBody::Json(text)))) => {
            ([(header::CONTENT_TYPE, "application/json")], text).into_response()
        }
        Ok(Ok(InvokeResponse::Ok(InvokeResponseBody::Raw(bytes)))) => {
            ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
        }
        Ok(Ok(InvokeResponse::Err(error))) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, error.0)
        }
        Ok(Err(_)) => error_response(StatusCode::INTERNAL_SERVER_ERROR, json!("responder dropped")),
        Err(_) => error_response(StatusCode::GATEWAY_TIMEOUT, json!("command timed out")),
    }
}

/// 订阅一个 Rust 事件（幂等）。事件名字符集与 Tauri 的 listen_any 要求一致，否则它会 panic
async fn subscribe(State(bridge): State<Arc<Bridge>>, Path(event): Path<String>) -> Response {
    let valid = !event.is_empty()
        && event
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '_'));
    if !valid {
        return error_response(StatusCode::BAD_REQUEST, json!("invalid event name"));
    }
    let first = bridge.subscribed.lock().unwrap().insert(event.clone());
    if first {
        let tx = bridge.events.clone();
        let name = event.clone();
        bridge.app.listen_any(event, move |e| {
            let payload: Value = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
            let _ = tx.send(json!({ "event": name, "payload": payload }).to_string());
        });
    }
    StatusCode::NO_CONTENT.into_response()
}

/// SSE：把已订阅事件推给浏览器；每 15 秒一条注释保活
async fn events_stream(State(bridge): State<Arc<Bridge>>) -> Response {
    let rx = bridge.events.subscribe();
    let body = stream::unfold(rx, |mut rx| async move {
        loop {
            match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(message)) => {
                    return Some((
                        Ok::<_, std::convert::Infallible>(Bytes::from(format!("data: {message}\n\n"))),
                        rx,
                    ));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Err(_) => {
                    return Some((Ok(Bytes::from_static(b": ping\n\n")), rx));
                }
            }
        }
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .unwrap()
}

/// 应用日志文件的尾部（含前端经 plugin:log 转发进来的 console 输出）。
/// 参数：lines（默认 200，最多 2000）、grep（子串过滤，大小写不敏感）、trace=1（包含 TRACE 级别）
async fn logs(
    State(bridge): State<Arc<Bridge>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let lines = query
        .get("lines")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(200)
        .min(2000);
    let grep = query.get("grep").map(|v| v.to_lowercase());
    // 默认不含 TRACE：dev 构建的日志级别很细（窗口系统、文件监听的底层事件会刷屏），传 trace=1 才包含
    let with_trace = query.get("trace").is_some_and(|v| v == "1");
    let Ok(dir) = bridge.app.path().app_log_dir() else {
        return (StatusCode::NOT_FOUND, "no log dir").into_response();
    };
    let newest = std::fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
    });
    let Some(file) = newest else {
        return (StatusCode::NOT_FOUND, "no log file").into_response();
    };
    // 只读文件尾部 512KB，避免大日志整个读入
    let text = (|| -> std::io::Result<String> {
        let mut f = std::fs::File::open(file.path())?;
        let len = f.metadata()?.len();
        let start = len.saturating_sub(512 * 1024);
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    })()
    .unwrap_or_default();
    let mut selected: Vec<&str> = text
        .lines()
        .filter(|l| with_trace || !l.contains("][TRACE]"))
        .filter(|l| grep.as_ref().is_none_or(|g| l.to_lowercase().contains(g)))
        .collect();
    let skip = selected.len().saturating_sub(lines);
    selected.drain(..skip);
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], selected.join("\n")).into_response()
}
