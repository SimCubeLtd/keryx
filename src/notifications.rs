//! Web Push delivery for draft activity. The database is the outbox: an
//! event and its per-subscription deliveries are written in the same
//! transaction as the draft change, and the dispatcher here drains them,
//! retries temporary failures, drops expired subscriptions, and turns due
//! snoozes into wake events. Payload encryption (RFC 8291) and VAPID
//! signing (RFC 8292) come from web-push-native, never from Keryx itself.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use axum::http::{Request, Uri};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Notify;
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::p256::PublicKey;
use web_push_native::{Auth, WebPushBuilder};

use crate::db::{self, PendingDelivery};

const VAPID_FILE: &str = "vapid.json";
/// Temporary failures are retried with doubling delays; after this many
/// attempts the delivery is dropped.
const MAX_ATTEMPTS: i64 = 6;
const PUSH_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const IDLE_SLEEP: Duration = Duration::from_secs(60 * 60);
const BATCH: usize = 50;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredVapid {
    private_key: String,
    public_key: String,
    created_at: String,
}

/// The server's stable VAPID key pair. Browsers bind their subscription to
/// the public key, so it is persisted under the data directory and reused
/// across restarts.
pub struct VapidIdentity {
    key_pair: ES256KeyPair,
    public_key: String,
}

impl VapidIdentity {
    pub fn generate() -> Self {
        Self::from_key_pair(ES256KeyPair::generate())
    }

    fn from_key_pair(key_pair: ES256KeyPair) -> Self {
        let public_key =
            URL_SAFE_NO_PAD.encode(key_pair.public_key().public_key().to_bytes_uncompressed());
        Self {
            key_pair,
            public_key,
        }
    }

    /// Load `<data_dir>/vapid.json`, or create it (owner-readable only).
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(VAPID_FILE);
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let stored: StoredVapid = serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?;
            let bytes = URL_SAFE_NO_PAD
                .decode(stored.private_key.trim_end_matches('='))
                .with_context(|| format!("decoding the VAPID key in {}", path.display()))?;
            let key_pair = ES256KeyPair::from_bytes(&bytes)
                .map_err(|error| anyhow!("invalid VAPID key in {}: {error}", path.display()))?;
            return Ok(Self::from_key_pair(key_pair));
        }

        let identity = Self::generate();
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("creating {}", data_dir.display()))?;
        let stored = StoredVapid {
            private_key: URL_SAFE_NO_PAD.encode(identity.key_pair.to_bytes()),
            public_key: identity.public_key.clone(),
            created_at: db::now(),
        };
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&stored)?),
        )
        .with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(identity)
    }

    /// Base64url, uncompressed P-256 point: what `pushManager.subscribe`
    /// takes as `applicationServerKey`.
    pub fn public_key(&self) -> &str {
        &self.public_key
    }
}

/// Shared between the HTTP handlers and the dispatcher task.
pub struct PushHub {
    vapid: VapidIdentity,
    contact: String,
    wake: Notify,
}

impl PushHub {
    pub fn new(vapid: VapidIdentity, contact: String) -> Self {
        Self {
            vapid,
            contact,
            wake: Notify::new(),
        }
    }

    pub fn public_key(&self) -> &str {
        self.vapid.public_key()
    }

    pub fn contact(&self) -> &str {
        &self.contact
    }

    /// Prod the dispatcher: a new event was recorded or a snooze changed the
    /// nearest wake time. A prod that arrives mid-run is kept, not lost.
    pub fn wake(&self) {
        self.wake.notify_one();
    }
}

/// The VAPID `sub` claim a push service may use to reach the operator.
pub fn default_contact(public_base_url: Option<&str>) -> String {
    match public_base_url {
        Some(url) if url.starts_with("https://") => url.to_string(),
        _ => "mailto:keryx@localhost".to_string(),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Delivered,
    /// The push service no longer knows the subscription; forget it.
    Expired,
    /// Temporary: rate limit, server error, or no response.
    Retry,
    /// Our request was wrong; retrying would not help.
    Rejected(String),
}

/// RFC 8030 status handling. 404 and 410 mean the subscription is gone,
/// 429 and 5xx are temporary, and any other client error is ours to fix.
pub fn classify(status: u16) -> DeliveryOutcome {
    match status {
        200..=299 => DeliveryOutcome::Delivered,
        404 | 410 => DeliveryOutcome::Expired,
        408 | 429 | 500..=599 => DeliveryOutcome::Retry,
        other => DeliveryOutcome::Rejected(format!("push service answered HTTP {other}")),
    }
}

/// Delay before attempt `attempts + 1`: 30s, 1m, 2m, 4m, 8m.
pub fn backoff_seconds(attempts: i64) -> i64 {
    30 * 2_i64.pow(attempts.clamp(0, 10) as u32)
}

fn decode_key(value: &str, what: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .with_context(|| format!("subscription {what} is not base64url"))
}

/// Encrypt the event payload for one subscription and sign it with VAPID.
/// The payload carries only display text and a same-origin path.
pub fn build_request(hub: &PushHub, delivery: &PendingDelivery) -> Result<Request<Vec<u8>>> {
    let endpoint: Uri = delivery
        .endpoint
        .parse()
        .context("subscription endpoint is not a valid URI")?;
    let p256dh = PublicKey::from_sec1_bytes(&decode_key(&delivery.p256dh, "p256dh key")?)
        .map_err(|_| anyhow!("subscription p256dh key is not a P-256 point"))?;
    let auth = decode_key(&delivery.auth, "auth secret")?;
    if auth.len() != 16 {
        bail!("subscription auth secret must be 16 bytes");
    }
    let payload = serde_json::to_vec(&json!({
        "title": delivery.event.title,
        "body": delivery.event.body,
        "tag": delivery.event.key,
        "target": delivery.event.target,
    }))?;
    WebPushBuilder::new(endpoint, p256dh, Auth::clone_from_slice(&auth))
        .with_valid_duration(PUSH_TTL)
        .with_vapid(&hub.vapid.key_pair, &hub.contact)
        .build(payload)
        .map_err(|error| anyhow!("building push request: {error}"))
}

async fn deliver(
    client: &reqwest::Client,
    hub: &PushHub,
    delivery: &PendingDelivery,
) -> DeliveryOutcome {
    let request = build_request(hub, delivery)
        .and_then(|request| reqwest::Request::try_from(request).context("converting push request"));
    let request = match request {
        Ok(request) => request,
        Err(error) => return DeliveryOutcome::Rejected(format!("{error:#}")),
    };
    match client.execute(request).await {
        Ok(response) => classify(response.status().as_u16()),
        Err(error) => {
            eprintln!("push: {} unreachable: {error}", delivery.endpoint);
            DeliveryOutcome::Retry
        }
    }
}

/// Record what happened to one delivery: done, retry later, or forget the
/// subscription.
pub fn apply_outcome(
    conn: &Connection,
    delivery: &PendingDelivery,
    outcome: DeliveryOutcome,
) -> Result<()> {
    let key = &delivery.event.key;
    let subscription = &delivery.subscription_id;
    match outcome {
        DeliveryOutcome::Delivered => db::delivery_done(conn, key, subscription)?,
        DeliveryOutcome::Expired => {
            db::remove_push_subscription_by_id(conn, subscription)?;
            eprintln!("push: removed expired subscription {subscription}");
        }
        DeliveryOutcome::Retry => {
            let attempts = delivery.attempts + 1;
            if attempts >= MAX_ATTEMPTS {
                db::delivery_done(conn, key, subscription)?;
                eprintln!("push: giving up on {key} for {subscription} after {attempts} attempts");
            } else {
                let next = Utc::now() + chrono::Duration::seconds(backoff_seconds(attempts));
                db::delivery_retry(
                    conn,
                    key,
                    subscription,
                    attempts,
                    &db::format_timestamp(next),
                )?;
            }
        }
        DeliveryOutcome::Rejected(reason) => {
            db::delivery_done(conn, key, subscription)?;
            eprintln!("push: dropped {key} for {subscription}: {reason}");
        }
    }
    Ok(())
}

/// Sleep until the earliest of the next wake and the next delivery, bounded
/// so a clock jump or a far-future snooze never parks the loop forever.
fn sleep_until(next: Option<String>) -> Duration {
    next.and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
        .map(|at| {
            (at.with_timezone(&Utc) - Utc::now())
                .to_std()
                .unwrap_or(Duration::ZERO)
        })
        .unwrap_or(IDLE_SLEEP)
        .clamp(Duration::from_millis(250), IDLE_SLEEP)
}

/// Runs for the life of the server. Each pass records wake events for due
/// snoozes, sends every due delivery, then sleeps until something is due or
/// a handler calls [`PushHub::wake`]. The first pass after a restart picks up
/// anything that came due while the server was down.
pub async fn run_dispatcher(db: Arc<Mutex<Connection>>, hub: Arc<PushHub>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("keryx/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client");

    loop {
        let now = db::now();
        let due = {
            let mut conn = db.lock().unwrap();
            match db::record_due_wakes(&mut conn, &now) {
                Ok(woke) => {
                    for event in woke {
                        println!("notification: {} · {}", event.title, event.draft_id);
                    }
                }
                Err(error) => eprintln!("notifications: recording wakes failed: {error:#}"),
            }
            db::due_deliveries(&conn, &now, BATCH).unwrap_or_else(|error| {
                eprintln!("notifications: reading deliveries failed: {error:#}");
                Vec::new()
            })
        };
        let drained = due.len() < BATCH;
        for delivery in due {
            let outcome = deliver(&client, &hub, &delivery).await;
            let conn = db.lock().unwrap();
            if let Err(error) = apply_outcome(&conn, &delivery, outcome) {
                eprintln!("notifications: updating delivery failed: {error:#}");
            }
        }
        if !drained {
            continue;
        }

        let next = {
            let conn = db.lock().unwrap();
            [
                db::next_wake_at(&conn, &db::now()).ok().flatten(),
                db::next_delivery_at(&conn).ok().flatten(),
            ]
            .into_iter()
            .flatten()
            .min()
        };
        tokio::select! {
            _ = hub.wake.notified() => {}
            _ = tokio::time::sleep(sleep_until(next)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NotificationEvent, NotificationKind, PushKeys, PushSubscriptionInput};

    #[test]
    fn classification_follows_rfc8030() {
        assert_eq!(classify(201), DeliveryOutcome::Delivered);
        assert_eq!(classify(404), DeliveryOutcome::Expired);
        assert_eq!(classify(410), DeliveryOutcome::Expired);
        assert_eq!(classify(429), DeliveryOutcome::Retry);
        assert_eq!(classify(503), DeliveryOutcome::Retry);
        assert!(matches!(classify(400), DeliveryOutcome::Rejected(_)));
        assert!(matches!(classify(413), DeliveryOutcome::Rejected(_)));
        assert_eq!(backoff_seconds(1), 60);
        assert_eq!(backoff_seconds(4), 480);
    }

    #[test]
    fn vapid_identity_is_created_once_and_reloaded() {
        let directory = tempfile::tempdir().unwrap();
        let first = VapidIdentity::load_or_create(directory.path()).unwrap();
        let second = VapidIdentity::load_or_create(directory.path()).unwrap();
        assert_eq!(first.public_key(), second.public_key());
        assert_eq!(
            URL_SAFE_NO_PAD.decode(first.public_key()).unwrap().len(),
            65
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.path().join(VAPID_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    fn fake_subscription() -> PushSubscriptionInput {
        let browser_key = ES256KeyPair::generate();
        PushSubscriptionInput {
            endpoint: "https://push.example.test/send/abc".into(),
            keys: PushKeys {
                p256dh: URL_SAFE_NO_PAD.encode(
                    browser_key
                        .public_key()
                        .public_key()
                        .to_bytes_uncompressed(),
                ),
                auth: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 16]>()),
            },
            events: None,
        }
    }

    #[test]
    fn push_requests_are_encrypted_and_signed_with_a_same_origin_target() {
        let hub = PushHub::new(VapidIdentity::generate(), default_contact(None));
        let conn = db::test_connection();
        db::upsert_push_subscription(&conn, &fake_subscription()).unwrap();
        let event = NotificationEvent::woke(
            "abc123def456",
            "Release checklist",
            "2026-08-28T08:00:00.000Z",
            "2026-08-28T08:00:00.100Z",
        );
        assert!(db::record_event(&conn, &event).unwrap());
        let delivery = db::due_deliveries(&conn, "2026-08-28T08:00:00.100Z", 10)
            .unwrap()
            .remove(0);

        let request = build_request(&hub, &delivery).unwrap();
        assert_eq!(request.method(), "POST");
        assert_eq!(request.uri(), "https://push.example.test/send/abc");
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        assert_eq!(request.headers()["ttl"], "86400");
        let authorization = request.headers()["authorization"].to_str().unwrap();
        assert!(authorization.starts_with("vapid t="));
        assert!(authorization.contains(&format!("k={}", hub.public_key())));
        let body = String::from_utf8_lossy(request.body());
        assert!(
            !body.contains("Release checklist"),
            "payload must be encrypted"
        );
        assert!(request.body().len() > 86);
        assert_eq!(event.target, "/d/abc123def456");
    }

    #[test]
    fn outcomes_retry_with_backoff_give_up_and_drop_expired_subscriptions() {
        let conn = db::test_connection();
        let subscription = db::upsert_push_subscription(&conn, &fake_subscription()).unwrap();
        assert_eq!(subscription.events, NotificationKind::ALL.to_vec());
        let event =
            NotificationEvent::published("abc123def456", "Plan", "V1", "2026-08-28T08:00:00.000Z");
        db::record_event(&conn, &event).unwrap();
        let far_future = "2099-01-01T00:00:00.000Z";

        let delivery = db::due_deliveries(&conn, far_future, 10).unwrap().remove(0);
        apply_outcome(&conn, &delivery, DeliveryOutcome::Retry).unwrap();
        assert!(db::due_deliveries(&conn, &db::now(), 10)
            .unwrap()
            .is_empty());
        let retried = db::due_deliveries(&conn, far_future, 10).unwrap().remove(0);
        assert_eq!(retried.attempts, 1);
        assert!(db::next_delivery_at(&conn).unwrap().unwrap() > db::now());

        let exhausted = PendingDelivery {
            attempts: MAX_ATTEMPTS - 1,
            ..retried
        };
        apply_outcome(&conn, &exhausted, DeliveryOutcome::Retry).unwrap();
        assert!(db::due_deliveries(&conn, far_future, 10)
            .unwrap()
            .is_empty());
        assert_eq!(db::next_delivery_at(&conn).unwrap(), None);

        let second =
            NotificationEvent::revised("abc123def456", "Plan", "V2", 2, "2026-08-28T09:00:00.000Z");
        db::record_event(&conn, &second).unwrap();
        let delivery = db::due_deliveries(&conn, far_future, 10).unwrap().remove(0);
        apply_outcome(&conn, &delivery, DeliveryOutcome::Expired).unwrap();
        assert!(db::due_deliveries(&conn, far_future, 10)
            .unwrap()
            .is_empty());
        assert!(
            !db::remove_push_subscription(&conn, "https://push.example.test/send/abc").unwrap()
        );
    }
}
