//! HTTP server. One optional API key guards mutations, listings, and PDF
//! publication; draft HTML serving remains public.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures_util::{stream, Stream};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;

use crate::db::{self, AvailabilityError, NewUpload, UploadError};
use crate::notifications::{self, PushHub, VapidIdentity};
use crate::pdf::{render_version_pdf, PdfIdentity};
use crate::policy::{validate_html, PolicyOptions, DEFAULT_MAX_HTML_BYTES};
use crate::realtime::DashboardUpdates;
use crate::render::{
    render_dashboard, render_dashboard_detail, render_dashboard_rows, render_not_found,
};
use crate::storage::BlobStore;
use crate::types::{
    Availability, AvailabilityUpdate, DraftDetail, DraftSummary, PushSubscriptionInput,
    UploadMetadata, UploadResponse,
};

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

    /// Accept <link> tags pointing at Google Fonts, and widen the served CSP
    /// so those stylesheets and font files actually load
    #[arg(long, env = "KERYX_ALLOW_FONT_LINKS")]
    pub allow_font_links: bool,

    /// Accept inline on* handlers whose body is assignment-only, e.g. the
    /// async-CSS idiom onload="this.media='all'"
    #[arg(long, env = "KERYX_ALLOW_SAFE_HANDLERS")]
    pub allow_safe_handlers: bool,

    /// Serve drafts with script-src 'unsafe-inline' so inline scripts and
    /// permitted on* handlers actually run. Accepting a script at upload is
    /// not enough on its own: without this the CSP still blocks execution
    #[arg(long, env = "KERYX_ALLOW_INLINE_SCRIPTS")]
    pub allow_inline_scripts: bool,

    /// Contact push services may use about this server's Web Push traffic,
    /// e.g. mailto:ops@example.com (default: the HTTPS public base URL,
    /// otherwise mailto:keryx@localhost)
    #[arg(long, env = "KERYX_PUSH_CONTACT")]
    pub push_contact: Option<String>,
}

impl ServeArgs {
    fn policy(&self) -> PolicyOptions {
        PolicyOptions {
            max_html_bytes: self.max_html_bytes,
            allow_font_links: self.allow_font_links,
            allow_safe_handlers: self.allow_safe_handlers,
            allow_inline_scripts: self.allow_inline_scripts,
        }
    }
}

struct AppState {
    db: Arc<Mutex<Connection>>,
    store: BlobStore,
    public_base_url: Option<String>,
    api_key_hash: Option<String>,
    policy: PolicyOptions,
    csp: HeaderValue,
    push: Arc<PushHub>,
    dashboard_updates: DashboardUpdates,
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
    let public_base_url = args
        .public_base_url
        .as_deref()
        .map(|u| u.trim_end_matches('/').to_string());
    let vapid = VapidIdentity::load_or_create(&data_dir)?;
    let push_contact = args
        .push_contact
        .clone()
        .unwrap_or_else(|| notifications::default_contact(public_base_url.as_deref()));

    let state: SharedState = Arc::new(AppState {
        db: Arc::new(Mutex::new(conn)),
        store: BlobStore::new(data_dir.clone()),
        public_base_url,
        api_key_hash: args.api_key.as_deref().map(crate::sha256_hex),
        policy: args.policy(),
        csp: draft_csp(&args.policy()),
        push: Arc::new(PushHub::new(vapid, push_contact)),
        dashboard_updates: DashboardUpdates::new(),
    });
    let blob_root = state.store.root().join("drafts");
    let dispatcher_db = state.db.clone();
    let dispatcher_hub = state.push.clone();
    let dashboard_updates = state.dashboard_updates.clone();

    let app = build_router(state, args.max_html_bytes);

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
            "policy: max {} bytes{}{}",
            args.max_html_bytes,
            if args.allow_font_links {
                " · Google Font <link> allowed"
            } else {
                ""
            },
            if args.allow_safe_handlers {
                " · assignment-only on* handlers allowed"
            } else {
                ""
            }
        );
        println!(
            "scripts: {}",
            if args.allow_inline_scripts {
                "inline scripts execute (script-src 'unsafe-inline')"
            } else {
                "inline scripts stored but never executed (script-src 'none')"
            }
        );
        println!(
            "auth: {}",
            if args.api_key.is_some() {
                "API key required for uploads/listings/deletes/PDFs"
            } else {
                "open (set KERYX_API_KEY to require a key)"
            }
        );
        println!(
            "push: VAPID identity {} · contact {}",
            data_dir.join("vapid.json").display(),
            dispatcher_hub.contact()
        );
        tokio::spawn(notifications::run_dispatcher(
            dispatcher_db,
            dispatcher_hub,
            dashboard_updates,
        ));
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

fn build_router(state: SharedState, max_html_bytes: usize) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/api/dashboard/events", get(dashboard_events))
        .route("/api/dashboard/snapshot", get(dashboard_snapshot))
        .route("/healthz", get(healthz))
        .route("/api/me", get(me))
        .route("/api/uploads", post(upload))
        .route("/api/drafts", get(list_drafts))
        .route("/api/drafts/{draft_id}", get(draft_detail))
        .route("/api/drafts/{draft_id}", delete(delete_draft))
        .route("/api/drafts/{draft_id}/pdf", get(publish_pdf))
        .route("/api/drafts/{draft_id}/availability", put(set_availability))
        .route("/api/drafts/{draft_id}/disable", post(disable_draft))
        .route("/api/purge", post(purge_deleted))
        .route("/api/push/vapid", get(push_vapid))
        .route(
            "/api/push/subscriptions",
            put(push_subscribe).delete(push_unsubscribe),
        )
        .route("/manifest.webmanifest", get(manifest))
        .route("/sw.js", get(service_worker))
        .route("/pwa-icon-192.png", get(icon_192))
        .route("/pwa-icon-512.png", get(icon_512))
        .route("/d/{draft_id}", get(serve_current))
        .route("/d/{draft_id}/raw", get(serve_current))
        .route("/d/{draft_id}/v/{version}", get(serve_version))
        .route("/d/{draft_id}/v/{version}/raw", get(serve_version))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(max_html_bytes * 2 + 64 * 1024))
        .layer(axum::middleware::map_response(common_headers))
        .with_state(state)
}

/// CSP for served drafts. Built once at startup: the bytes are never altered,
/// so the only question is what the page may do once a browser has it. Both
/// knobs have to track the upload policy — accepting a font `<link>` or an
/// inline script and then blocking it here would store content that can never
/// work. `connect-src` stays `'none'` either way: a draft is a document, not
/// a client for something else.
fn draft_csp(policy: &PolicyOptions) -> HeaderValue {
    // 'unsafe-inline' covers inline <script>, on* handlers, and javascript:
    // URLs; upload validation is what keeps the last two in check.
    let script_src = if policy.allow_inline_scripts {
        "'unsafe-inline'"
    } else {
        "'none'"
    };
    let (style_src, font_src) = if policy.allow_font_links {
        (
            "'unsafe-inline' https://fonts.googleapis.com",
            " font-src https://fonts.gstatic.com;",
        )
    } else {
        ("'unsafe-inline'", "")
    };
    let csp = format!(
        "default-src 'none'; script-src {script_src}; style-src {style_src};{font_src} \
         img-src https: data:; connect-src 'none'; base-uri 'none'; form-action 'none'"
    );
    HeaderValue::from_str(&csp).expect("CSP is built from ASCII fragments")
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
    let drafts = dashboard_drafts(&state, &base);
    match drafts {
        Ok(drafts) => Html(render_dashboard(
            &drafts,
            &base,
            state.api_key_hash.is_none(),
        ))
        .into_response(),
        Err(error) => internal_error(error),
    }
}

fn dashboard_drafts(state: &AppState, base: &str) -> Result<Vec<DraftSummary>> {
    let mut drafts = {
        let conn = state.db.lock().unwrap();
        db::list_drafts(&conn)?
    };
    for draft in &mut drafts {
        fill_urls(draft, base);
    }
    Ok(drafts)
}

#[derive(Deserialize, Default)]
struct DashboardSnapshotQuery {
    selected: Option<String>,
}

/// Return the server-rendered mutable parts of the dashboard. This route is
/// public like `/`, but protected deployments still redact management data.
async fn dashboard_snapshot(
    State(state): State<SharedState>,
    Query(query): Query<DashboardSnapshotQuery>,
    headers: HeaderMap,
) -> Response {
    let base = base_url(&state, &headers);
    let drafts = match dashboard_drafts(&state, &base) {
        Ok(drafts) => drafts,
        Err(error) => return internal_error(error),
    };
    let selected = query
        .selected
        .as_deref()
        .and_then(|selected| drafts.iter().find(|draft| draft.draft_id == selected))
        .or_else(|| {
            drafts
                .iter()
                .find(|draft| draft.availability() == Availability::Active)
        })
        .or_else(|| drafts.first());
    let management_enabled = state.api_key_hash.is_none();

    Json(json!({
        "ok": true,
        "rows": render_dashboard_rows(
            &drafts,
            selected.map(|draft| draft.draft_id.as_str()),
            management_enabled,
        ),
        "detail": render_dashboard_detail(selected, management_enabled),
    }))
    .into_response()
}

/// Stream coalesced invalidations. Each connection immediately receives the
/// current revision, so EventSource reconnects always trigger a fresh snapshot.
async fn dashboard_events(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(
        (state.dashboard_updates.subscribe(), true),
        |(mut receiver, initial)| async move {
            if !initial && receiver.changed().await.is_err() {
                return None;
            }
            let revision = *receiver.borrow_and_update();
            let event = Event::default()
                .event("dashboard")
                .id(revision.to_string())
                .data("refresh");
            Some((Ok(event), (receiver, false)))
        },
    );
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
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
        "authRequired": state.api_key_hash.is_some(),
        "policy": state.policy
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
    let validation = validate_html(&html, &state.policy);
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
            state.push.wake();
            state.dashboard_updates.changed();
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
struct PdfQuery {
    version: Option<i64>,
}

/// Render one immutable stored version. This route intentionally accepts no
/// HTML body, so Fulgur cannot be exposed as a general conversion service.
async fn publish_pdf(
    State(state): State<SharedState>,
    Path(draft_id): Path<String>,
    Query(query): Query<PdfQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    if query.version.is_some_and(|version| version < 1) {
        return json_error(StatusCode::BAD_REQUEST, "Version must be at least 1.");
    }

    let served = {
        let conn = state.db.lock().unwrap();
        db::find_public_version(&conn, &draft_id, query.version)
    };
    let served = match served {
        Ok(Some(served)) => served,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "Draft version not found."),
        Err(error) => return internal_error(error),
    };
    let html = match state.store.get(&served.object_key) {
        Ok(html) => html,
        Err(error) => return internal_error(error),
    };

    let render_draft_id = served.draft_id.clone();
    let version_number = served.version_number;
    let version_created_at = served.created_at;
    let rendered = tokio::task::spawn_blocking(move || {
        render_version_pdf(
            &html,
            PdfIdentity {
                draft_id: &render_draft_id,
                version_number,
                version_created_at: &version_created_at,
            },
        )
    })
    .await;
    let rendered = match rendered {
        Ok(Ok(rendered)) => rendered,
        Ok(Err(error)) => {
            eprintln!("PDF export rejected for {draft_id} v{version_number}: {error:#}");
            return json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                &format!("PDF export failed: {error:#}"),
            );
        }
        Err(error) => return internal_error(error.into()),
    };

    let base = base_url(&state, &headers);
    let public_url = format!("{base}/d/{draft_id}/v/{version_number}");
    let raw_url = format!("{public_url}/raw");
    let mut response = rendered.bytes.into_response();
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    let disposition = format!("attachment; filename=\"keryx-{draft_id}-v{version_number}.pdf\"");
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        response_headers.insert(header::CONTENT_DISPOSITION, value);
    }
    for (name, value) in [
        ("x-keryx-draft-id", draft_id),
        ("x-keryx-draft-version", version_number.to_string()),
        ("x-keryx-public-url", public_url),
        ("x-keryx-raw-url", raw_url),
        ("x-keryx-pdf-pages", rendered.page_count.to_string()),
        ("x-keryx-pdf-images", rendered.image_count.to_string()),
        ("x-keryx-pdf-svgs", rendered.svg_count.to_string()),
    ] {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name),
            HeaderValue::from_str(&value),
        ) {
            response_headers.insert(name, value);
        }
    }
    response
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
                state.dashboard_updates.changed();
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
        Ok(true) => {
            state.dashboard_updates.changed();
            Json(json!({ "ok": true })).into_response()
        }
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
            if count > 0 {
                state.dashboard_updates.changed();
            }
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

/// `PUT /api/drafts/:id/availability`: the single transition endpoint for
/// active, snoozed, and disabled. Responds with the updated draft summary.
async fn set_availability(
    State(state): State<SharedState>,
    Path(draft_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<AvailabilityUpdate>, JsonRejection>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let update = match body {
        Ok(Json(update)) => update,
        Err(rejection) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("Invalid availability update: {}", rejection.body_text()),
            )
        }
    };
    apply_availability(&state, &headers, &draft_id, update)
}

/// Compatibility adapter for the original disable route; it routes through
/// the same mutation as the availability endpoint.
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
    let reason = body.and_then(|Json(b)| b.reason);
    apply_availability(
        &state,
        &headers,
        &draft_id,
        AvailabilityUpdate::Disabled { reason },
    )
}

fn apply_availability(
    state: &AppState,
    headers: &HeaderMap,
    draft_id: &str,
    update: AvailabilityUpdate,
) -> Response {
    let update = match update {
        AvailabilityUpdate::Disabled { reason } => AvailabilityUpdate::Disabled {
            reason: clean_text(reason.as_deref(), 255),
        },
        other => other,
    };
    let result = {
        let mut conn = state.db.lock().unwrap();
        db::set_availability(&mut conn, draft_id, &update)
    };
    match result {
        Ok(mut draft) => {
            state.push.wake();
            state.dashboard_updates.changed();
            fill_urls(&mut draft, &base_url(state, headers));
            Json(json!({ "ok": true, "draft": draft })).into_response()
        }
        Err(AvailabilityError::DraftNotFound) => {
            json_error(StatusCode::NOT_FOUND, "Draft not found.")
        }
        Err(AvailabilityError::InvalidWakeTime(message)) => {
            json_error(StatusCode::BAD_REQUEST, &message)
        }
        Err(AvailabilityError::Other(error)) => internal_error(error),
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
            headers.insert("content-security-policy", state.csp.clone());
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

// --- push subscriptions ------------------------------------------------------
// Same authentication rule as every other mutation: with an API key set the
// dashboard is read-only, so a protected deployment has no browser path to
// subscribe until Keryx has browser authentication.

async fn push_vapid(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    Json(json!({ "ok": true, "publicKey": state.push.public_key() })).into_response()
}

async fn push_subscribe(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Result<Json<PushSubscriptionInput>, JsonRejection>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let input = match body {
        Ok(Json(input)) => input,
        Err(rejection) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("Invalid subscription: {}", rejection.body_text()),
            )
        }
    };
    if let Err(error) = notifications::check_endpoint(&input.endpoint) {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("Subscription endpoint rejected: {error}."),
        );
    }
    let result = {
        let conn = state.db.lock().unwrap();
        db::upsert_push_subscription(&conn, &input)
    };
    match result {
        Ok(subscription) => {
            Json(json!({ "ok": true, "subscription": subscription })).into_response()
        }
        Err(error) => internal_error(error),
    }
}

#[derive(Deserialize)]
struct UnsubscribeBody {
    endpoint: String,
}

async fn push_unsubscribe(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Result<Json<UnsubscribeBody>, JsonRejection>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let Ok(Json(body)) = body else {
        return json_error(StatusCode::BAD_REQUEST, "Endpoint is required.");
    };
    let result = {
        let conn = state.db.lock().unwrap();
        db::remove_push_subscription(&conn, &body.endpoint)
    };
    match result {
        Ok(removed) => Json(json!({ "ok": true, "removed": removed })).into_response(),
        Err(error) => internal_error(error),
    }
}

// --- installable app assets --------------------------------------------------
// Served on every deployment; the browser's origin decides whether it will
// register the worker or offer installation.

const MANIFEST: &str = include_str!("../assets/manifest.webmanifest");
const SERVICE_WORKER: &str = include_str!("../assets/service-worker.js");
const ICON_192: &[u8] = include_bytes!("../assets/pwa-icon-192.png");
const ICON_512: &[u8] = include_bytes!("../assets/pwa-icon-512.png");

fn static_asset(content_type: &'static str, body: impl Into<axum::body::Body>) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body.into()).into_response()
}

async fn manifest() -> Response {
    static_asset("application/manifest+json", MANIFEST)
}

async fn service_worker() -> Response {
    static_asset("text/javascript; charset=utf-8", SERVICE_WORKER)
}

async fn icon_192() -> Response {
    static_asset("image/png", ICON_192)
}

async fn icon_512() -> Response {
    static_asset("image/png", ICON_512)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    fn upload<'a>(
        html: &'a str,
        draft_id: Option<String>,
        metadata: &'a UploadMetadata,
    ) -> NewUpload<'a> {
        NewUpload {
            html,
            filename: Some("report.html".into()),
            draft_id,
            description: None,
            title_from_html: Some("PDF endpoint test".into()),
            metadata,
            source_ip: None,
            user_agent: None,
            has_inline_script: false,
            external_image_hosts: &[],
        }
    }

    /// A protected server (API key "secret") on a throwaway store.
    fn test_state() -> SharedState {
        let store = crate::storage::test_store();
        let conn = db::open(&store.root().join("test.db")).unwrap();
        Arc::new(AppState {
            db: Arc::new(Mutex::new(conn)),
            store,
            public_base_url: Some("https://keryx.test".into()),
            api_key_hash: Some(crate::sha256_hex("secret")),
            policy: PolicyOptions::default(),
            csp: draft_csp(&PolicyOptions::default()),
            push: Arc::new(PushHub::new(
                VapidIdentity::generate(),
                "mailto:test@keryx.test".into(),
            )),
            dashboard_updates: DashboardUpdates::new(),
        })
    }

    #[test]
    fn csp_tracks_the_upload_policy() {
        let strict = draft_csp(&PolicyOptions::default());
        let strict = strict.to_str().unwrap();
        assert!(strict.contains("script-src 'none'"));
        assert!(!strict.contains("fonts.googleapis.com"));
        assert!(!strict.contains("font-src"));

        let open = draft_csp(&PolicyOptions {
            allow_font_links: true,
            allow_inline_scripts: true,
            ..PolicyOptions::default()
        });
        let open = open.to_str().unwrap();
        assert!(open.contains("script-src 'unsafe-inline'"));
        assert!(open.contains("style-src 'unsafe-inline' https://fonts.googleapis.com"));
        assert!(open.contains("font-src https://fonts.gstatic.com"));
        // A draft is a document, never a client for something else.
        assert!(open.contains("connect-src 'none'"));
    }

    fn contains_pdf(path: &std::path::Path) -> bool {
        let Ok(entries) = std::fs::read_dir(path) else {
            return false;
        };
        entries.filter_map(std::result::Result::ok).any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                contains_pdf(&path)
            } else {
                path.extension().is_some_and(|extension| extension == "pdf")
            }
        })
    }

    #[tokio::test]
    async fn pdf_endpoint_is_authenticated_versioned_and_ephemeral() {
        let state = test_state();
        let metadata = UploadMetadata::default();
        let draft_id = {
            let mut conn = state.db.lock().unwrap();
            let first = db::record_upload(
                &mut conn,
                &state.store,
                upload(
                    "<!doctype html><title>v1</title><h1>First</h1>",
                    None,
                    &metadata,
                ),
            )
            .unwrap();
            db::record_upload(
                &mut conn,
                &state.store,
                upload(
                    "<!doctype html><title>v2</title><h1>Latest</h1>",
                    Some(first.draft_id.clone()),
                    &metadata,
                ),
            )
            .unwrap();
            first.draft_id
        };

        let unauthorized = publish_pdf(
            State(state.clone()),
            Path(draft_id.clone()),
            Query(PdfQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        // Publishing a PDF creates neither a version nor a notification.
        let counts = |state: &AppState| -> (i64, i64) {
            state
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM draft_versions), (SELECT COUNT(*) FROM notification_events)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap()
        };
        let before = counts(&state);
        assert_eq!(before.1, 2);
        let response = publish_pdf(
            State(state.clone()),
            Path(draft_id.clone()),
            Query(PdfQuery::default()),
            headers.clone(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            HeaderValue::from_static("application/pdf")
        );
        assert_eq!(response.headers()["x-keryx-draft-version"], "2");
        assert_eq!(
            response.headers()["x-keryx-public-url"],
            format!("https://keryx.test/d/{draft_id}/v/2")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.starts_with(b"%PDF-"));

        let explicit = publish_pdf(
            State(state.clone()),
            Path(draft_id.clone()),
            Query(PdfQuery { version: Some(1) }),
            headers,
        )
        .await;
        assert_eq!(explicit.status(), StatusCode::OK);
        assert_eq!(explicit.headers()["x-keryx-draft-version"], "1");

        assert_eq!(counts(&state), before);
        assert!(!contains_pdf(state.store.root()));

        std::fs::remove_dir_all(state.store.root()).ok();
    }

    #[test]
    fn pwa_assets_are_installable_and_the_worker_never_intercepts_requests() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["name"], "Keryx");
        assert_eq!(manifest["start_url"], "/");
        assert_eq!(manifest["scope"], "/");
        assert_eq!(manifest["display"], "standalone");
        let sizes: Vec<&str> = manifest["icons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|icon| icon["sizes"].as_str().unwrap())
            .collect();
        assert!(sizes.contains(&"192x192"));
        assert!(sizes.contains(&"512x512"));
        assert!(ICON_192.starts_with(b"\x89PNG"));
        assert!(ICON_512.starts_with(b"\x89PNG"));

        assert!(SERVICE_WORKER.contains("addEventListener(\"push\""));
        assert!(SERVICE_WORKER.contains("addEventListener(\"notificationclick\""));
        assert!(!SERVICE_WORKER.contains("fetch"));
        assert!(!SERVICE_WORKER.contains("caches"));
    }

    #[tokio::test]
    async fn realtime_routes_stream_invalidations_and_keep_protected_snapshots_redacted() {
        let state = test_state();
        let metadata = UploadMetadata {
            repo_org: Some("SimCubeLtd".into()),
            repo_name: Some("keryx".into()),
            git_branch: Some("feat/realtime-dashboard".into()),
            ..UploadMetadata::default()
        };
        {
            let mut conn = state.db.lock().unwrap();
            db::record_upload(
                &mut conn,
                &state.store,
                upload(
                    "<!doctype html><title>Realtime</title><h1>Realtime</h1>",
                    None,
                    &metadata,
                ),
            )
            .unwrap();
        }

        let snapshot = dashboard_snapshot(
            State(state.clone()),
            Query(DashboardSnapshotQuery::default()),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(snapshot.status(), StatusCode::OK);
        let body = json_body(snapshot).await;
        assert!(body["rows"].as_str().unwrap().contains("Protected"));
        assert!(!body["rows"].as_str().unwrap().contains("SimCubeLtd"));
        assert!(!body["detail"]
            .as_str()
            .unwrap()
            .contains("feat/realtime-dashboard"));

        let events = dashboard_events(State(state.clone())).await.into_response();
        assert_eq!(events.status(), StatusCode::OK);
        assert_eq!(
            events.headers()[header::CONTENT_TYPE],
            HeaderValue::from_static("text/event-stream")
        );

        std::fs::remove_dir_all(state.store.root()).ok();
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn push_subscription_routes_are_authenticated_and_validate_endpoints() {
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let input = |endpoint: &str| {
            Ok(Json(PushSubscriptionInput {
                endpoint: endpoint.into(),
                keys: crate::types::PushKeys {
                    p256dh: "BPUBLIC".into(),
                    auth: "AUTH".into(),
                },
                events: Some(vec![crate::types::NotificationKind::Woke]),
            }))
        };

        let unauthorized = push_vapid(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let vapid = json_body(push_vapid(State(state.clone()), headers.clone()).await).await;
        assert_eq!(vapid["publicKey"], state.push.public_key());

        let plain = push_subscribe(
            State(state.clone()),
            headers.clone(),
            input("http://push.example.test/x"),
        )
        .await;
        assert_eq!(plain.status(), StatusCode::BAD_REQUEST);

        let stored = push_subscribe(
            State(state.clone()),
            headers.clone(),
            input("https://push.example.test/x"),
        )
        .await;
        assert_eq!(stored.status(), StatusCode::OK);
        let body = json_body(stored).await;
        assert_eq!(body["subscription"]["events"], json!(["woke"]));

        let removed = push_unsubscribe(
            State(state.clone()),
            headers,
            Ok(Json(UnsubscribeBody {
                endpoint: "https://push.example.test/x".into(),
            })),
        )
        .await;
        assert_eq!(json_body(removed).await["removed"], true);

        std::fs::remove_dir_all(state.store.root()).ok();
    }

    #[tokio::test]
    async fn availability_route_owns_every_transition() {
        let state = test_state();
        let mut dashboard_updates = state.dashboard_updates.subscribe();
        let metadata = UploadMetadata::default();
        let draft_id = {
            let mut conn = state.db.lock().unwrap();
            db::record_upload(
                &mut conn,
                &state.store,
                upload(
                    "<!doctype html><title>v1</title><h1>First</h1>",
                    None,
                    &metadata,
                ),
            )
            .unwrap()
            .draft_id
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let snooze = || {
            Ok(Json(AvailabilityUpdate::Snoozed {
                until: "2099-01-01T08:00:00Z".into(),
            }))
        };

        let unauthorized = set_availability(
            State(state.clone()),
            Path(draft_id.clone()),
            HeaderMap::new(),
            snooze(),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let missing = set_availability(
            State(state.clone()),
            Path("missing".into()),
            headers.clone(),
            snooze(),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let past = set_availability(
            State(state.clone()),
            Path(draft_id.clone()),
            headers.clone(),
            Ok(Json(AvailabilityUpdate::Snoozed {
                until: "2000-01-01T08:00:00Z".into(),
            })),
        )
        .await;
        assert_eq!(past.status(), StatusCode::BAD_REQUEST);
        assert!(!dashboard_updates.has_changed().unwrap());

        let snoozed = set_availability(
            State(state.clone()),
            Path(draft_id.clone()),
            headers.clone(),
            snooze(),
        )
        .await;
        assert_eq!(snoozed.status(), StatusCode::OK);
        assert!(dashboard_updates.has_changed().unwrap());
        dashboard_updates.borrow_and_update();
        let body = json_body(snoozed).await;
        assert_eq!(body["draft"]["snoozedUntil"], "2099-01-01T08:00:00.000Z");
        assert_eq!(body["draft"]["disabled"], false);
        assert_eq!(
            body["draft"]["publicUrl"],
            format!("https://keryx.test/d/{draft_id}")
        );
        assert_eq!(
            serve_draft(&state, &draft_id, None).status(),
            StatusCode::OK
        );
        assert_eq!(
            serve_draft(&state, &draft_id, Some(1)).status(),
            StatusCode::OK
        );

        let disabled = disable_draft(
            State(state.clone()),
            Path(draft_id.clone()),
            headers.clone(),
            Some(Json(DisableBody {
                reason: Some("  Superseded  ".into()),
            })),
        )
        .await;
        assert_eq!(disabled.status(), StatusCode::OK);
        let body = json_body(disabled).await;
        assert_eq!(body["draft"]["disabled"], true);
        assert!(body["draft"]["snoozedUntil"].is_null());
        assert_eq!(
            serve_draft(&state, &draft_id, None).status(),
            StatusCode::NOT_FOUND
        );
        let reason: String = state
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT disabled_reason FROM drafts WHERE id = ?1",
                [&draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reason, "Superseded");

        let enabled = set_availability(
            State(state.clone()),
            Path(draft_id.clone()),
            headers,
            Ok(Json(AvailabilityUpdate::Active)),
        )
        .await;
        assert_eq!(enabled.status(), StatusCode::OK);
        assert_eq!(
            serve_draft(&state, &draft_id, None).status(),
            StatusCode::OK
        );

        std::fs::remove_dir_all(state.store.root()).ok();
    }
}
