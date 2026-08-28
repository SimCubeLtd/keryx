//! Web Push delivery for draft activity. The database is the outbox: an
//! event and its per-subscription deliveries are written in the same
//! transaction as the draft change, and the dispatcher here drains them,
//! retries temporary failures, drops expired subscriptions, and turns due
//! snoozes into wake events. Payload encryption (RFC 8291) and VAPID
//! signing (RFC 8292) come from web-push-native, never from Keryx itself.

use std::net::{IpAddr, SocketAddr};
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
/// Deliveries in flight at once; one slow push service must not hold up
/// the rest of a batch.
const CONCURRENCY: usize = 8;

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

/// Push endpoints are public, vendor-run services. Refusing anything else
/// stops a subscription from turning the dispatcher into a probe of the
/// server's own network. Names are checked again at connection time by
/// [`PublicResolver`], after DNS.
pub fn check_endpoint(endpoint: &str) -> Result<()> {
    let url = url::Url::parse(endpoint).context("endpoint is not a valid URL")?;
    if url.scheme() != "https" {
        bail!("endpoint must use https");
    }
    match url.host() {
        None => bail!("endpoint has no host"),
        Some(url::Host::Domain(domain)) => {
            let name = domain.trim_end_matches('.').to_ascii_lowercase();
            let internal = name == "localhost"
                || !name.contains('.')
                || [".localhost", ".local", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix));
            if internal {
                bail!("endpoint host {domain:?} is not a public name");
            }
        }
        Some(url::Host::Ipv4(ip)) if !is_public(IpAddr::V4(ip)) => {
            bail!("endpoint address {ip} is not public")
        }
        Some(url::Host::Ipv6(ip)) if !is_public(IpAddr::V6(ip)) => {
            bail!("endpoint address {ip} is not public")
        }
        Some(_) => {}
    }
    Ok(())
}

/// Globally routable unicast only: no loopback, private, link-local,
/// shared (CGNAT), documentation, multicast, or reserved space.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || (s[0] == 0x2001 && s[1] == 0x0db8))
        }
    }
}

/// System DNS with every non-public address dropped, so a name that points
/// (or is later rebound) inside the network never gets a connection.
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let public: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|address| is_public(address.ip()))
                .collect();
            if public.is_empty() {
                return Err(format!("{host} resolves to no public address").into());
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

fn decode_key(value: &str, what: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .with_context(|| format!("subscription {what} is not base64url"))
}

/// Encrypt the event payload for one subscription and sign it with VAPID.
/// The payload carries only display text and a same-origin path.
pub fn build_request(hub: &PushHub, delivery: &PendingDelivery) -> Result<Request<Vec<u8>>> {
    check_endpoint(&delivery.endpoint)?;
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
                let next =
                    Utc::now() + chrono::Duration::seconds(backoff_seconds(delivery.attempts));
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
    // No redirects: an approved endpoint must not be able to forward the
    // request somewhere the endpoint policy would have refused.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("keryx/{}", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::new(PublicResolver))
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
        let mut pending = due.into_iter();
        let mut in_flight = tokio::task::JoinSet::new();
        loop {
            while in_flight.len() < CONCURRENCY {
                let Some(delivery) = pending.next() else {
                    break;
                };
                let client = client.clone();
                let hub = hub.clone();
                in_flight.spawn(async move {
                    let outcome = deliver(&client, &hub, &delivery).await;
                    (delivery, outcome)
                });
            }
            match in_flight.join_next().await {
                Some(Ok((delivery, outcome))) => {
                    let conn = db.lock().unwrap();
                    if let Err(error) = apply_outcome(&conn, &delivery, outcome) {
                        eprintln!("notifications: updating delivery failed: {error:#}");
                    }
                }
                Some(Err(error)) => eprintln!("notifications: delivery task failed: {error}"),
                None => break,
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
        assert_eq!(backoff_seconds(0), 30);
        assert_eq!(backoff_seconds(1), 60);
        assert_eq!(backoff_seconds(4), 480);
    }

    #[test]
    fn endpoint_policy_accepts_public_push_services_only() {
        for endpoint in [
            "https://fcm.googleapis.com/fcm/send/abc",
            "https://updates.push.services.mozilla.com/wpush/v2/abc",
            "https://web.push.apple.com/abc",
            "https://[2606:4700::1]/push",
            "https://93.184.216.34/push",
        ] {
            assert!(
                check_endpoint(endpoint).is_ok(),
                "{endpoint} should be accepted"
            );
        }
        for endpoint in [
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://localhost/push",
            "https://keryx/push",
            "https://printer.local/push",
            "https://vault.internal/push",
            "https://127.0.0.1:1/push",
            "https://10.0.0.5/push",
            "https://192.168.1.10/push",
            "https://172.16.0.1/push",
            "https://169.254.169.254/latest/meta-data",
            "https://100.100.1.1/push",
            "https://0.0.0.0/push",
            "https://[::1]/push",
            "https://[fd00::1]/push",
            "https://[fe80::1]/push",
            "https://[::ffff:127.0.0.1]/push",
            "not a url",
        ] {
            assert!(
                check_endpoint(endpoint).is_err(),
                "{endpoint} should be rejected"
            );
        }
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
        // The first retry waits the documented 30 seconds.
        let next = DateTime::parse_from_rfc3339(&db::next_delivery_at(&conn).unwrap().unwrap())
            .unwrap()
            .with_timezone(&Utc);
        let wait = (next - Utc::now()).num_seconds();
        assert!((25..=30).contains(&wait), "first retry waits {wait}s");

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
