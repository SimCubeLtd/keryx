//! HTTP server. One optional API key guards mutations, listings, and PDF
//! publication; draft HTML serving remains public.

mod notifications;
mod realtime;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
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
use serde::Deserialize;
use serde_json::json;

use crate::notifications::{PushHub, VapidIdentity};
use crate::realtime::DashboardUpdates;
use keryx_core::ids::new_internal_id;
use keryx_core::types::{
    Availability, AvailabilityUpdate, DraftDetail, DraftSummary, PushSubscriptionInput,
    UploadMetadata, UploadResponse,
};
use keryx_db::{
    AvailabilityError, DatabaseConfig, DraftStore, NewUpload, SeaOrmStore, UploadError,
};
use keryx_policy::{validate_html, PolicyOptions, DEFAULT_MAX_HTML_BYTES};
use keryx_render::pdf::{render_version_pdf, PdfIdentity};
use keryx_render::{
    render_dashboard, render_dashboard_detail, render_dashboard_rows, render_not_found,
};
use keryx_store::{create_backend, object_key, BackendConfig, BlobBackend, DiskConfig, S3Config};

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageKind {
    Disk,
    S3,
}

/// The database flags, shared by `serve` and the offline `storage` commands.
#[derive(clap::Args, Debug, Clone)]
pub struct DatabaseArgs {
    /// SQLite database path (default: ~/.keryx/keryx.db). Ignored when
    /// --database-url is set
    #[arg(long, env = "KERYX_DB")]
    pub db: Option<PathBuf>,

    /// A postgres:// URL selects Postgres instead of SQLite. TLS is set in
    /// the URL with sslmode and sslrootcert
    #[arg(long, env = "KERYX_DATABASE_URL", hide_env_values = true)]
    pub database_url: Option<String>,

    /// Database connections in the pool (default: 1 on SQLite, 4 on Postgres)
    #[arg(long, env = "KERYX_DB_POOL_SIZE")]
    pub db_pool_size: Option<u32>,

    /// Skip the snapshot Keryx takes before it first adopts a SQLite database
    /// written by an older version
    #[arg(long, env = "KERYX_NO_BACKUP")]
    pub no_backup: bool,
}

impl DatabaseArgs {
    /// Postgres when a URL is given; otherwise SQLite at --db, which keeps
    /// every existing deployment on its current path with no new flags.
    pub fn config(&self) -> DatabaseConfig {
        match self
            .database_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
        {
            Some(url) => DatabaseConfig::Postgres {
                url: url.trim().to_string(),
                pool_size: self.db_pool_size,
            },
            None => DatabaseConfig::Sqlite {
                path: self.db.clone().unwrap_or_else(default_db_path),
                backup: !self.no_backup,
                pool_size: self.db_pool_size,
            },
        }
    }
}

/// The S3 flags, shared by `serve` and the offline `storage` commands so one
/// set of environment variables configures all of them.
#[derive(clap::Args, Debug, Clone)]
pub struct S3Args {
    /// S3 bucket; required when --storage is s3. Credentials never come from
    /// Keryx flags: they resolve through the standard AWS chain
    #[arg(long, env = "KERYX_S3_BUCKET")]
    pub s3_bucket: Option<String>,

    /// S3 region, or the placeholder most S3-compatible endpoints accept
    #[arg(long, env = "KERYX_S3_REGION", default_value = "us-east-1")]
    pub s3_region: String,

    /// Custom S3 endpoint for RustFS, MinIO, Ceph RGW, R2 or B2
    /// (default: AWS_ENDPOINT_URL_S3, then AWS)
    #[arg(long, env = "KERYX_S3_ENDPOINT")]
    pub s3_endpoint: Option<String>,

    /// Key prefix inside the bucket
    #[arg(long, env = "KERYX_S3_PREFIX", default_value = "")]
    pub s3_prefix: String,

    /// Named AWS profile for credential lookup
    #[arg(long, env = "KERYX_S3_PROFILE")]
    pub s3_profile: Option<String>,
}

impl S3Args {
    /// The blob backend a storage kind selects. `data_dir` roots the disk
    /// backend.
    pub fn backend_config(
        &self,
        kind: StorageKind,
        data_dir: &std::path::Path,
    ) -> Result<BackendConfig> {
        Ok(match kind {
            StorageKind::Disk => BackendConfig::Disk(DiskConfig {
                data_dir: data_dir.to_path_buf(),
            }),
            StorageKind::S3 => BackendConfig::S3(S3Config {
                bucket: self
                    .s3_bucket
                    .clone()
                    .filter(|bucket| !bucket.trim().is_empty())
                    .context("s3 storage needs --s3-bucket (or KERYX_S3_BUCKET)")?,
                region: self.s3_region.clone(),
                endpoint: self.s3_endpoint.clone(),
                prefix: self.s3_prefix.clone(),
                profile: self.s3_profile.clone(),
            }),
        })
    }
}

#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, env = "KERYX_PORT", default_value_t = 7812)]
    pub port: u16,

    /// Address to bind
    #[arg(long, env = "KERYX_HOST", default_value = "127.0.0.1")]
    pub host: String,

    #[command(flatten)]
    pub database: DatabaseArgs,

    /// Directory for local state: the push identity, the blob staging area,
    /// and the stored HTML files when --storage is disk (default: ~/.keryx)
    #[arg(long, env = "KERYX_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Where draft HTML is stored. The --s3-* flags are ignored unless this
    /// is s3
    #[arg(long, env = "KERYX_STORAGE", value_enum, default_value_t = StorageKind::Disk)]
    pub storage: StorageKind,

    #[command(flatten)]
    pub s3: S3Args,

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
    db: Arc<dyn DraftStore>,
    store: Arc<dyn BlobBackend>,
    public_base_url: Option<String>,
    api_key_hash: Option<String>,
    policy: PolicyOptions,
    csp: HeaderValue,
    push: Arc<PushHub>,
    dashboard_updates: DashboardUpdates,
}

type SharedState = Arc<AppState>;

pub fn default_state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".keryx")
}

pub fn default_db_path() -> PathBuf {
    default_state_dir().join("keryx.db")
}

pub fn run(args: ServeArgs) -> Result<()> {
    let database = args.database.config();
    let data_dir = args.data_dir.clone().unwrap_or_else(default_state_dir);
    let public_base_url = args
        .public_base_url
        .as_deref()
        .map(|u| u.trim_end_matches('/').to_string());
    let vapid = VapidIdentity::load_or_create(&data_dir)?;
    let push_contact = args
        .push_contact
        .clone()
        .unwrap_or_else(|| notifications::default_contact(public_base_url.as_deref()));

    let backend_config = args.s3.backend_config(args.storage, &data_dir)?;
    let api_key_hash = args.api_key.as_deref().map(keryx_core::sha256_hex);
    let policy = args.policy();

    let addr = format!("{}:{}", args.host, args.port);
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        // Fail fast: a misconfigured object store must stop the boot, not
        // surface as a 500 on the first upload of the day.
        let store = create_backend(&backend_config).await?;
        let probe_started = std::time::Instant::now();
        store
            .probe()
            .await
            .with_context(|| format!("blob store startup probe failed for {}", store.describe()))?;
        let probe_ms = probe_started.elapsed().as_millis();
        let blob_description = store.describe().to_string();

        // Opening adopts a legacy database in place, after a backup.
        let (store_db, adoption) = SeaOrmStore::open(&database).await?;
        let db_status = adoption.to_string();

        let state: SharedState = Arc::new(AppState {
            db: Arc::new(store_db),
            store,
            public_base_url,
            api_key_hash,
            csp: draft_csp(&policy),
            policy,
            push: Arc::new(PushHub::new(vapid, push_contact)),
            dashboard_updates: DashboardUpdates::new(),
        });
        let dispatcher_db = state.db.clone();
        let dispatcher_hub = state.push.clone();
        let dashboard_updates = state.dashboard_updates.clone();
        let app = build_router(state, args.max_html_bytes);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("binding {addr}"))?;
        println!("keryx serving on http://{addr}");
        println!("database: {} ({db_status})", database.describe());
        println!("blobs: {blob_description} (probe ok, {probe_ms} ms)");
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
    keryx_core::sha256_hex(&token) == *expected
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
    let drafts = dashboard_drafts(&state, &base).await;
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

async fn dashboard_drafts(state: &AppState, base: &str) -> Result<Vec<DraftSummary>> {
    let mut drafts = state.db.list_drafts().await?;
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
    let drafts = match dashboard_drafts(&state, &base).await {
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
    match state.db.ping().await {
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

    // 1. Cheap read: resolve or mint the draft id.
    let target = state
        .db
        .resolve_upload_target(clean_text(body.draft_id.as_deref(), 255))
        .await;
    let (draft_id, created) = match target {
        Ok(target) => target,
        Err(UploadError::DraftNotFound) => {
            return json_error(StatusCode::NOT_FOUND, "Draft not found.")
        }
        Err(UploadError::Other(error)) => return internal_error(error),
    };
    let version_id = new_internal_id();
    let object_key = object_key(&draft_id, &version_id);

    // 2. The blob lands before the metadata commits, with no transaction open.
    if let Err(error) = state.store.put(&object_key, &html).await {
        return internal_error(error);
    }

    // 3. Metadata only, with the ids and key handed in.
    let upload = NewUpload {
        html: &html,
        filename: clean_text(body.filename.as_deref(), 255),
        draft_id,
        created,
        version_id,
        object_key: object_key.clone(),
        description: clean_text(body.description.as_deref(), 1000),
        title_from_html: validation.title.clone(),
        metadata: &body.metadata,
        source_ip: Some(source_ip),
        user_agent,
        has_inline_script: validation.has_inline_script,
        external_image_hosts: &validation.external_image_hosts,
    };
    let outcome = state.db.record_upload(upload).await;
    if outcome.is_err() {
        // The draft went away mid-upload, or the record step failed: the blob
        // has no row. Best effort; anything that slips through is an orphan.
        remove_blobs(&state, std::slice::from_ref(&object_key)).await;
    }

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
    let drafts = state.db.list_drafts().await;
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
    let result = match state.db.get_draft_summary(&draft_id).await {
        Ok(draft) => state
            .db
            .list_versions(&draft_id)
            .await
            .map(|versions| (draft, versions)),
        Err(error) => Err(error),
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

    let served = state.db.find_public_version(&draft_id, query.version).await;
    let served = match served {
        Ok(Some(served)) => served,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "Draft version not found."),
        Err(error) => return internal_error(error),
    };
    let html = match state.store.get(&served.object_key).await {
        Ok(Some(html)) => html,
        // A row whose blob is gone, e.g. a database rewound past a purge.
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "Draft version not found."),
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
        let result = state.db.purge_draft(&draft_id).await;
        return match result {
            Ok(Some(keys)) => {
                state.dashboard_updates.changed();
                remove_blobs(&state, &keys).await;
                Json(json!({ "ok": true, "purged": true })).into_response()
            }
            Ok(None) => json_error(StatusCode::NOT_FOUND, "Draft not found."),
            Err(error) => internal_error(error),
        };
    }

    let result = state.db.soft_delete_draft(&draft_id).await;
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
    let result = state.db.purge_deleted_drafts().await;
    match result {
        Ok((count, keys)) => {
            if count > 0 {
                state.dashboard_updates.changed();
            }
            remove_blobs(&state, &keys).await;
            Json(json!({ "ok": true, "purgedDrafts": count })).into_response()
        }
        Err(error) => internal_error(error),
    }
}

/// The rows are already gone when this runs, so a failed removal only leaves
/// orphan blobs for `keryx storage gc`: log it rather than failing the
/// request. One `remove_many` call, because a purge can report thousands of
/// keys and S3 deletes them in batches.
async fn remove_blobs(state: &AppState, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    if let Err(error) = state.store.remove_many(keys).await {
        eprintln!("failed to remove {} blobs: {error:#}", keys.len());
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
    apply_availability(&state, &headers, &draft_id, update).await
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
    .await
}

async fn apply_availability(
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
    let result = state.db.set_availability(draft_id, &update).await;
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
    serve_draft(&state, &draft_id, None).await
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
    serve_draft(&state, &draft_id, Some(version_number)).await
}

/// Serve the exact uploaded HTML, byte for byte, to every client — browsers,
/// curl, and agent fetchers alike. No browser detection, no wrapper page. The
/// CSP never changes the bytes a client reads; it only constrains what the
/// page may do if a human opens it in a browser.
async fn serve_draft(state: &AppState, draft_id: &str, version: Option<i64>) -> Response {
    let found = state.db.find_public_version(draft_id, version).await;
    match found {
        Ok(Some(served)) => {
            let html = match state.store.get(&served.object_key).await {
                Ok(Some(html)) => html,
                // A row whose blob is gone, e.g. a database rewound past a purge.
                Ok(None) => return not_found().await,
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
    let result = state.db.upsert_push_subscription(&input).await;
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
    let result = state.db.remove_push_subscription(&body.endpoint).await;
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
    use std::sync::Mutex;

    /// Store the blob and record its metadata, as the upload handler does.
    async fn record(
        state: &AppState,
        html: &str,
        draft_id: Option<String>,
        metadata: &UploadMetadata,
    ) -> keryx_db::UploadOutcome {
        let (draft_id, created) = state.db.resolve_upload_target(draft_id).await.unwrap();
        let version_id = new_internal_id();
        let object_key = object_key(&draft_id, &version_id);
        state.store.put(&object_key, html).await.unwrap();
        state
            .db
            .record_upload(NewUpload {
                html,
                filename: Some("report.html".into()),
                draft_id,
                created,
                version_id,
                object_key,
                description: None,
                title_from_html: Some("PDF endpoint test".into()),
                metadata,
                source_ip: None,
                user_agent: None,
                has_inline_script: false,
                external_image_hosts: &[],
            })
            .await
            .unwrap()
    }

    /// A protected server (API key "secret") on an in-memory store.
    async fn test_state() -> SharedState {
        test_state_with(keryx_store::memory_backend()).await
    }

    async fn test_state_with(store: Arc<dyn BlobBackend>) -> SharedState {
        test_state_and_db(store).await.0
    }

    /// The state plus the concrete store behind it, for tests that look at
    /// rows no store method exposes.
    async fn test_state_and_db(store: Arc<dyn BlobBackend>) -> (SharedState, Arc<SeaOrmStore>) {
        let db = Arc::new(SeaOrmStore::open_test().await);
        let state = Arc::new(AppState {
            db: db.clone(),
            store,
            public_base_url: Some("https://keryx.test".into()),
            api_key_hash: Some(keryx_core::sha256_hex("secret")),
            policy: PolicyOptions::default(),
            csp: draft_csp(&PolicyOptions::default()),
            push: Arc::new(PushHub::new(
                VapidIdentity::generate(),
                "mailto:test@keryx.test".into(),
            )),
            dashboard_updates: DashboardUpdates::new(),
        });
        (state, db)
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

    #[tokio::test]
    async fn pdf_endpoint_is_authenticated_versioned_and_ephemeral() {
        let (state, db) = test_state_and_db(keryx_store::memory_backend()).await;
        let metadata = UploadMetadata::default();
        let draft_id = {
            let first = record(
                &state,
                "<!doctype html><title>v1</title><h1>First</h1>",
                None,
                &metadata,
            )
            .await;
            record(
                &state,
                "<!doctype html><title>v2</title><h1>Latest</h1>",
                Some(first.draft_id.clone()),
                &metadata,
            )
            .await;
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
        let counts = || async {
            (
                db.peek::<i64>("SELECT COUNT(*) FROM draft_versions")
                    .await
                    .unwrap(),
                db.peek::<i64>("SELECT COUNT(*) FROM notification_events")
                    .await
                    .unwrap(),
            )
        };
        let before = counts().await;
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

        assert_eq!(counts().await, before);
        let stored = state.store.list("").await.unwrap();
        assert!(stored.iter().all(|entry| !entry.key.ends_with(".pdf")));
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
        let state = test_state().await;
        let metadata = UploadMetadata {
            repo_org: Some("SimCubeLtd".into()),
            repo_name: Some("keryx".into()),
            git_branch: Some("feat/realtime-dashboard".into()),
            ..UploadMetadata::default()
        };
        {
            record(
                &state,
                "<!doctype html><title>Realtime</title><h1>Realtime</h1>",
                None,
                &metadata,
            )
            .await;
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
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn push_subscription_routes_are_authenticated_and_validate_endpoints() {
        let state = test_state().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        let input = |endpoint: &str| {
            Ok(Json(PushSubscriptionInput {
                endpoint: endpoint.into(),
                keys: keryx_core::types::PushKeys {
                    p256dh: "BPUBLIC".into(),
                    auth: "AUTH".into(),
                },
                events: Some(vec![keryx_core::types::NotificationKind::Woke]),
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
    }

    #[tokio::test]
    async fn availability_route_owns_every_transition() {
        let (state, db) = test_state_and_db(keryx_store::memory_backend()).await;
        let mut dashboard_updates = state.dashboard_updates.subscribe();
        let metadata = UploadMetadata::default();
        let draft_id = {
            record(
                &state,
                "<!doctype html><title>v1</title><h1>First</h1>",
                None,
                &metadata,
            )
            .await
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
            serve_draft(&state, &draft_id, None).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            serve_draft(&state, &draft_id, Some(1)).await.status(),
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
            serve_draft(&state, &draft_id, None).await.status(),
            StatusCode::NOT_FOUND
        );
        let reason: String = db
            .peek(&format!(
                "SELECT disabled_reason FROM drafts WHERE id = '{draft_id}'"
            ))
            .await
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
            serve_draft(&state, &draft_id, None).await.status(),
            StatusCode::OK
        );
    }

    /// A backend whose failures and side effects a test scripts, over a real
    /// in-memory store.
    #[derive(Default)]
    struct ScriptedBackend {
        inner: Option<Arc<dyn BlobBackend>>,
        fail_put: bool,
        fail_remove: bool,
        /// Runs once, after a successful put: the window in which a draft can
        /// vanish between resolve and record.
        after_put: Mutex<Option<futures_util::future::BoxFuture<'static, ()>>>,
        removed: Mutex<Vec<Vec<String>>>,
    }

    impl ScriptedBackend {
        fn new() -> Self {
            Self {
                inner: Some(keryx_store::memory_backend()),
                ..Self::default()
            }
        }

        fn inner(&self) -> &Arc<dyn BlobBackend> {
            self.inner.as_ref().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl BlobBackend for ScriptedBackend {
        async fn put(&self, key: &str, html: &str) -> Result<()> {
            if self.fail_put {
                anyhow::bail!("scripted put failure");
            }
            self.inner().put(key, html).await?;
            let hook = self.after_put.lock().unwrap().take();
            if let Some(hook) = hook {
                hook.await;
            }
            Ok(())
        }
        async fn get(&self, key: &str) -> Result<Option<String>> {
            self.inner().get(key).await
        }
        async fn remove_many(&self, keys: &[String]) -> Result<()> {
            self.removed.lock().unwrap().push(keys.to_vec());
            if self.fail_remove {
                anyhow::bail!("scripted remove failure");
            }
            self.inner().remove_many(keys).await
        }
        async fn list(&self, prefix: &str) -> Result<Vec<keryx_store::BlobEntry>> {
            self.inner().list(prefix).await
        }
        async fn probe(&self) -> Result<()> {
            Ok(())
        }
        fn describe(&self) -> &str {
            "scripted://"
        }
    }

    fn bearer() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        headers
    }

    async fn post_upload(state: &SharedState, draft_id: Option<&str>) -> Response {
        let body: UploadBody = serde_json::from_value(json!({
            "html": "<!doctype html><title>Upload</title><h1>Upload</h1>",
            "draftId": draft_id,
        }))
        .unwrap();
        upload(
            State(state.clone()),
            ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4000))),
            bearer(),
            Json(body),
        )
        .await
    }

    async fn row_count(db: &SeaOrmStore, table: &str) -> i64 {
        db.peek(&format!("SELECT COUNT(*) FROM {table}"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_failed_blob_put_leaves_no_draft_or_version_row() {
        let (state, db) = test_state_and_db(Arc::new(ScriptedBackend {
            fail_put: true,
            ..ScriptedBackend::new()
        }))
        .await;

        let response = post_upload(&state, None).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(row_count(&db, "drafts").await, 0);
        assert_eq!(row_count(&db, "draft_versions").await, 0);
    }

    #[tokio::test]
    async fn an_upload_to_a_draft_purged_mid_flight_fails_cleanly_and_removes_its_blob() {
        let backend = Arc::new(ScriptedBackend::new());
        let (state, db) = test_state_and_db(backend.clone()).await;
        let draft_id = record(&state, "<p>v1</p>", None, &UploadMetadata::default())
            .await
            .draft_id;
        // Arm a purge to run between the blob put and the record step.
        let (purging, id) = (state.db.clone(), draft_id.clone());
        *backend.after_put.lock().unwrap() = Some(Box::pin(async move {
            purging.purge_draft(&id).await.unwrap().unwrap();
        }));

        let response = post_upload(&state, Some(&draft_id)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(row_count(&db, "draft_versions").await, 0);

        // The handler removed exactly the blob it had just written.
        let removed = backend.removed.lock().unwrap().clone();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].len(), 1);
        assert!(removed[0][0].starts_with(&format!("drafts/{draft_id}/")));
        assert_eq!(backend.get(&removed[0][0]).await.unwrap(), None);
    }

    #[tokio::test]
    async fn purge_removes_exactly_the_reported_keys_and_survives_a_removal_failure() {
        let backend = Arc::new(ScriptedBackend {
            fail_remove: true,
            ..ScriptedBackend::new()
        });
        let state = test_state_with(backend.clone()).await;
        let metadata = UploadMetadata::default();
        let first = record(&state, "<p>v1</p>", None, &metadata).await;
        record(&state, "<p>v2</p>", Some(first.draft_id.clone()), &metadata).await;
        let mut expected: Vec<String> = state
            .db
            .blob_records()
            .await
            .unwrap()
            .into_iter()
            .map(|record| record.object_key)
            .collect();
        expected.sort();

        let response = delete_draft(
            State(state.clone()),
            Path(first.draft_id.clone()),
            Query(serde_json::from_value(json!({ "purge": true })).unwrap()),
            bearer(),
        )
        .await;
        // The rows are gone, so the failed removal is logged, not fatal.
        assert_eq!(response.status(), StatusCode::OK);

        let mut removed = backend.removed.lock().unwrap().clone();
        assert_eq!(removed.len(), 1, "one remove_many call, not one per key");
        removed[0].sort();
        assert_eq!(removed[0], expected);
    }

    #[tokio::test]
    async fn a_version_whose_blob_is_missing_serves_not_found() {
        let state = test_state().await;
        let outcome = record(&state, "<p>v1</p>", None, &UploadMetadata::default()).await;
        let served = state
            .db
            .find_public_version(&outcome.draft_id, None)
            .await
            .unwrap()
            .unwrap();
        state.store.remove_many(&[served.object_key]).await.unwrap();

        let response = serve_draft(&state, &outcome.draft_id, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = publish_pdf(
            State(state.clone()),
            Path(outcome.draft_id.clone()),
            Query(serde_json::from_value(json!({})).unwrap()),
            bearer(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
