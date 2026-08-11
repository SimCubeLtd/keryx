//! HTTP server. One optional API key guards mutations and listings; draft
//! serving is public; HTML bytes come straight out of SQLite.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;

use crate::db::{self, NewUpload, UploadError};
use crate::policy::{validate_html, DEFAULT_MAX_HTML_BYTES};
use crate::render::{render_dashboard, render_not_found};
use crate::storage::BlobStore;
use crate::types::{DraftDetail, DraftSummary, UploadMetadata, UploadResponse};

#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, env = "KERYX_PORT", default_value_t = 7812)]
    pub port: u16,

    /// Address to bind
    #[arg(long, env = "KERYX_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// SQLite database path (default: ~/.keryx/keryx.db)
    #[arg(long, env = "KERYX_DB")]
    pub db: Option<PathBuf>,

    /// Directory for stored HTML files (default: ~/.keryx)
    #[arg(long, env = "KERYX_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Base URL used in returned links, e.g. http://myhost:7812
    /// (default: derived from each request's Host header)
    #[arg(long, env = "KERYX_PUBLIC_BASE_URL")]
    pub public_base_url: Option<String>,

    /// If set, uploads, listings, and deletes require this key as a Bearer token
    #[arg(long, env = "KERYX_API_KEY")]
    pub api_key: Option<String>,

    /// Maximum accepted HTML size in bytes
    #[arg(long, env = "KERYX_MAX_HTML_BYTES", default_value_t = DEFAULT_MAX_HTML_BYTES)]
    pub max_html_bytes: usize,
}

struct AppState {
    db: Mutex<Connection>,
    store: BlobStore,
    public_base_url: Option<String>,
    api_key_hash: Option<String>,
    max_html_bytes: usize,
}

type SharedState = Arc<AppState>;

fn default_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".keryx")
}

pub fn default_db_path() -> PathBuf {
    default_state_dir().join("keryx.db")
}

pub fn run(args: ServeArgs) -> Result<()> {
    let db_path = args.db.clone().unwrap_or_else(default_db_path);
    let data_dir = args.data_dir.clone().unwrap_or_else(default_state_dir);
    let conn = db::open(&db_path)?;

    let state: SharedState = Arc::new(AppState {
        db: Mutex::new(conn),
        store: BlobStore::new(data_dir.clone()),
        public_base_url: args
            .public_base_url
            .as_deref()
            .map(|u| u.trim_end_matches('/').to_string()),
        api_key_hash: args.api_key.as_deref().map(crate::sha256_hex),
        max_html_bytes: args.max_html_bytes,
    });
    let blob_root = state.store.root().join("drafts");

    let app = Router::new()
        .route("/", get(dashboard))
        .route("/healthz", get(healthz))
        .route("/api/me", get(me))
        .route("/api/uploads", post(upload))
        .route("/api/drafts", get(list_drafts))
        .route("/api/drafts/{draft_id}", get(draft_detail))
        .route("/api/drafts/{draft_id}", delete(delete_draft))
        .route("/api/drafts/{draft_id}/disable", post(disable_draft))
        .route("/api/purge", post(purge_deleted))
        .route("/d/{draft_id}", get(serve_current))
        .route("/d/{draft_id}/raw", get(serve_current))
        .route("/d/{draft_id}/v/{version}", get(serve_version))
        .route("/d/{draft_id}/v/{version}/raw", get(serve_version))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(args.max_html_bytes * 2 + 64 * 1024))
        .layer(axum::middleware::map_response(common_headers))
        .with_state(state);

    let addr = format!("{}:{}", args.host, args.port);
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        println!("keryx serving on http://{addr}");
        println!("database: {}", db_path.display());
        println!("blobs: {}", blob_root.display());
        println!(
            "auth: {}",
            if args.api_key.is_some() {
                "API key required for uploads/listings/deletes"
            } else {
                "open (set KERYX_API_KEY to require a key)"
            }
        );
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
        Ok(())
    })
}

async fn common_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// --- auth -------------------------------------------------------------------

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = &state.api_key_hash else {
        return true;
    };
    let Some(token) = bearer_token(headers) else {
        return false;
    };
    // Hash both sides so the comparison is constant-time in the token bytes.
    crate::sha256_hex(&token) == *expected
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let token = rest.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn unauthorized() -> Response {
    json_error(StatusCode::UNAUTHORIZED, "Missing or invalid API key.")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "ok": false, "error": message }))).into_response()
}

// --- URL helpers ------------------------------------------------------------

fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(configured) = &state.public_base_url {
        return configured.clone();
    }
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("http")
        .to_string();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("{proto}://{host}")
}

fn fill_urls(draft: &mut DraftSummary, base: &str) {
    draft.public_url = format!("{base}/d/{}", draft.draft_id);
    draft.raw_url = format!("{base}/d/{}/raw", draft.draft_id);
}

// --- handlers ---------------------------------------------------------------

async fn dashboard(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    let base = base_url(&state, &headers);
    let drafts = {
        let conn = state.db.lock().unwrap();
        db::list_drafts(&conn)
    };
    match drafts {
        Ok(mut drafts) => {
            for draft in &mut drafts {
                fill_urls(draft, &base);
            }
            Html(render_dashboard(&drafts, &base)).into_response()
        }
        Err(error) => internal_error(error),
    }
}

async fn healthz(State(state): State<SharedState>) -> Response {
    let result = {
        let conn = state.db.lock().unwrap();
        conn.query_row("SELECT 1", [], |_| Ok(()))
            .map_err(anyhow::Error::from)
    };
    match result {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn me(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    Json(json!({
        "ok": true,
        "authRequired": state.api_key_hash.is_some()
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadBody {
    html: Option<String>,
    filename: Option<String>,
    draft_id: Option<String>,
    description: Option<String>,
    #[serde(default)]
    metadata: UploadMetadata,
}

async fn upload(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<UploadBody>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }

    let html = body.html.unwrap_or_default();
    let validation = validate_html(&html, state.max_html_bytes);
    if !validation.ok() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "ok": false,
                "errors": validation.errors,
                "warnings": validation.warnings
            })),
        )
            .into_response();
    }

    let source_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let upload = NewUpload {
        html: &html,
        filename: clean_text(body.filename.as_deref(), 255),
        draft_id: clean_text(body.draft_id.as_deref(), 255),
        description: clean_text(body.description.as_deref(), 1000),
        title_from_html: validation.title.clone(),
        metadata: &body.metadata,
        source_ip: Some(source_ip),
        user_agent,
        has_inline_script: validation.has_inline_script,
        external_image_hosts: &validation.external_image_hosts,
    };

    let outcome = {
        let mut conn = state.db.lock().unwrap();
        db::record_upload(&mut conn, &state.store, upload)
    };

    match outcome {
        Ok(outcome) => {
            let base = base_url(&state, &headers);
            let response = UploadResponse {
                public_url: format!("{base}/d/{}", outcome.draft_id),
                raw_url: format!("{base}/d/{}/raw", outcome.draft_id),
                draft_id: outcome.draft_id,
                version_id: outcome.version_id,
                version_number: outcome.version_number,
                title: outcome.title,
                warnings: validation.warnings,
            };
            let status = if outcome.created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            let mut body = serde_json::to_value(&response).unwrap_or_default();
            body["ok"] = json!(true);
            (status, Json(body)).into_response()
        }
        Err(UploadError::DraftNotFound) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
        Err(UploadError::Other(error)) => internal_error(error),
    }
}

async fn list_drafts(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let base = base_url(&state, &headers);
    let drafts = {
        let conn = state.db.lock().unwrap();
        db::list_drafts(&conn)
    };
    match drafts {
        Ok(mut drafts) => {
            for draft in &mut drafts {
                fill_urls(draft, &base);
            }
            Json(json!({ "ok": true, "drafts": drafts })).into_response()
        }
        Err(error) => internal_error(error),
    }
}

async fn draft_detail(
    State(state): State<SharedState>,
    Path(draft_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let base = base_url(&state, &headers);
    let result = {
        let conn = state.db.lock().unwrap();
        db::get_draft_summary(&conn, &draft_id)
            .and_then(|draft| Ok((draft, db::list_versions(&conn, &draft_id)?)))
    };
    match result {
        Ok((Some(mut draft), versions)) => {
            fill_urls(&mut draft, &base);
            Json(json!({ "ok": true, "draft": DraftDetail { draft, versions } })).into_response()
        }
        Ok((None, _)) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
        Err(error) => internal_error(error),
    }
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    purge: Option<bool>,
}

async fn delete_draft(
    State(state): State<SharedState>,
    Path(draft_id): Path<String>,
    Query(query): Query<DeleteQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }

    if query.purge.unwrap_or(false) {
        let result = {
            let mut conn = state.db.lock().unwrap();
            db::purge_draft(&mut conn, &draft_id)
        };
        return match result {
            Ok(Some(keys)) => {
                remove_blobs(&state, &keys);
                Json(json!({ "ok": true, "purged": true })).into_response()
            }
            Ok(None) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
            Err(error) => internal_error(error),
        };
    }

    let result = {
        let conn = state.db.lock().unwrap();
        db::soft_delete_draft(&conn, &draft_id)
    };
    match result {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
        Err(error) => internal_error(error),
    }
}

/// Housekeeping: hard-delete every soft-deleted draft and its files.
async fn purge_deleted(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let result = {
        let mut conn = state.db.lock().unwrap();
        db::purge_deleted_drafts(&mut conn)
    };
    match result {
        Ok((count, keys)) => {
            remove_blobs(&state, &keys);
            Json(json!({ "ok": true, "purgedDrafts": count })).into_response()
        }
        Err(error) => internal_error(error),
    }
}

/// The rows are already gone when this runs, so a failed file removal only
/// leaves an orphan blob — log it rather than failing the request.
fn remove_blobs(state: &AppState, keys: &[String]) {
    for key in keys {
        if let Err(error) = state.store.remove(key) {
            eprintln!("purge: failed to remove blob {key}: {error:#}");
        }
    }
}

#[derive(Deserialize, Default)]
struct DisableBody {
    reason: Option<String>,
}

async fn disable_draft(
    State(state): State<SharedState>,
    Path(draft_id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<DisableBody>>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let reason = body
        .and_then(|Json(b)| clean_text(b.reason.as_deref(), 255))
        .unwrap_or_else(|| "Disabled by owner.".to_string());
    let result = {
        let conn = state.db.lock().unwrap();
        db::disable_draft(&conn, &draft_id, &reason)
    };
    match result {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
        Err(error) => internal_error(error),
    }
}

async fn serve_current(State(state): State<SharedState>, Path(draft_id): Path<String>) -> Response {
    serve_draft(&state, &draft_id, None)
}

async fn serve_version(
    State(state): State<SharedState>,
    Path((draft_id, version)): Path<(String, String)>,
) -> Response {
    let Ok(version_number) = version.parse::<i64>() else {
        return not_found().await;
    };
    if version_number < 1 {
        return not_found().await;
    }
    serve_draft(&state, &draft_id, Some(version_number))
}

/// Serve the exact uploaded HTML, byte for byte, to every client — browsers,
/// curl, and agent fetchers alike. No browser detection, no wrapper page. The
/// CSP never changes the bytes a client reads; it only constrains what the
/// page may do if a human opens it in a browser.
fn serve_draft(state: &AppState, draft_id: &str, version: Option<i64>) -> Response {
    let found = {
        let conn = state.db.lock().unwrap();
        db::find_public_version(&conn, draft_id, version)
    };
    match found {
        Ok(Some(served)) => {
            let html = match state.store.get(&served.object_key) {
                Ok(html) => html,
                Err(error) => return internal_error(error),
            };
            let mut response = Html(html).into_response();
            let headers = response.headers_mut();
            headers.insert(
                "content-security-policy",
                HeaderValue::from_static(
                    "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; \
                     img-src https: data:; connect-src 'none'; base-uri 'none'; form-action 'none'",
                ),
            );
            if let Ok(value) = HeaderValue::from_str(&served.draft_id) {
                headers.insert("x-keryx-draft-id", value);
            }
            if let Ok(value) = HeaderValue::from_str(&served.version_number.to_string()) {
                headers.insert("x-keryx-draft-version", value);
            }
            response
        }
        Ok(None) => (StatusCode::NOT_FOUND, Html(render_not_found())).into_response(),
        Err(error) => internal_error(error),
    }
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Html(render_not_found())).into_response()
}

fn internal_error(error: anyhow::Error) -> Response {
    eprintln!("internal error: {error:#}");
    json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error.")
}

fn clean_text(value: Option<&str>, max_length: usize) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_length).collect())
}
