//! GBOOST S9 core: agent listener (client enrol, webhook-gated sessions, rpc) and owner listener
//! (console token). Secrets are never logged: only ids and truncated hashes.
use axum::{
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CLIENT_FILE: &str = include_str!("../../client/gboost_client.py");
const REQ_LOG_MAX: u64 = 1_000_000;
const SEC_LOG_MAX: u64 = 10_000_000;
const BODY_MAX: usize = 16 * 1024;
const MAX_IP_ENTRIES: usize = 1000;
const STR_CAP: usize = 64;
const AUTHFAIL_LOG_GAP_MS: u64 = 10_000;

#[derive(Clone)]
pub struct Config {
    pub state_dir: PathBuf,
    pub session_ttl_s: u64,
    pub code_ttl_s: u64,
    pub session_budget: u32,
    pub max_starts_per_hour: usize,
    pub check_cooldown_s: u64,
    pub max_checks_per_hour_global: usize,
    pub webhook_post_timeout_ms: u64,
    pub max_pending_enrol: usize,
    pub max_clients: usize,
    pub pending_ttl_s: u64,
    pub revoked_purge_s: u64,
    pub enrol_per_ip: usize,
    pub enrol_ip_window_s: u64,
    pub webhook_timeout_s: u64,
    pub request_timeout_s: u64,
    pub noise_flush_s: u64,
}
impl Config {
    pub fn new(state_dir: PathBuf) -> Self {
        Config {
            state_dir,
            session_ttl_s: 4 * 3600,
            code_ttl_s: 300,
            session_budget: 500,
            max_starts_per_hour: 6,
            check_cooldown_s: 60,
            max_checks_per_hour_global: 12,
            webhook_post_timeout_ms: 8000,
            max_pending_enrol: 5,
            max_clients: 50,
            pending_ttl_s: 15 * 60,
            revoked_purge_s: 24 * 3600,
            enrol_per_ip: 3,
            enrol_ip_window_s: 600,
            webhook_timeout_s: 10,
            request_timeout_s: 15,
            noise_flush_s: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct ClientRec {
    id: String,
    secret_hash: String,
    fingerprint: String,
    state: String, // pending | approved | revoked
    created_ms: u64,
    #[serde(default)]
    revoked_ms: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct Session {
    id: String,
    client_id: String,
    created_ms: u64,
    expires_ms: u64,
    used: u32,
}
struct PendingCode {
    hash: String,
    expires_ms: u64,
}
#[derive(Default)]
struct Data {
    clients: HashMap<String, ClientRec>,
    sessions: HashMap<String, Session>, // key: sha256(session token)
    msessions: HashMap<String, Session>, // MCP connector: one server-side session per client id
    codes: HashMap<String, PendingCode>,
    starts: HashMap<String, Vec<u64>>,
    last_check: HashMap<String, u64>,
    global_starts: Vec<u64>,
    enrol_ips: HashMap<IpAddr, Vec<u64>>,
    authfail_logged: HashMap<String, u64>,
    version: u64,
}
struct Snap {
    ver: u64,
    json: String,
}
impl Data {
    /// Drop expired pending enrolments, old revoked records, expired sessions and codes. Returns true when
    /// the client list changed (the caller persists it after dropping the lock).
    fn sweep(&mut self, cfg: &Config, now: u64) -> bool {
        let before = self.clients.len();
        let pttl = cfg.pending_ttl_s.saturating_mul(1000);
        let rttl = cfg.revoked_purge_s.saturating_mul(1000);
        self.clients.retain(|_, c| match c.state.as_str() {
            "pending" => now < c.created_ms.saturating_add(pttl),
            "revoked" => now < c.revoked_ms.unwrap_or(c.created_ms).saturating_add(rttl),
            _ => true,
        });
        let clients = &self.clients;
        self.sessions.retain(|_, s| now < s.expires_ms && clients.contains_key(&s.client_id));
        self.msessions.retain(|k, s| now < s.expires_ms && clients.contains_key(k));
        self.codes.retain(|k, p| now < p.expires_ms && clients.contains_key(k));
        self.starts.retain(|k, v| {
            v.retain(|t| now.saturating_sub(*t) < 3_600_000);
            !v.is_empty() && clients.contains_key(k)
        });
        self.last_check.retain(|k, t| now.saturating_sub(*t) < 3_600_000 && clients.contains_key(k));
        self.global_starts.retain(|t| now.saturating_sub(*t) < 3_600_000);
        self.authfail_logged.retain(|k, _| clients.contains_key(k));
        let win = cfg.enrol_ip_window_s.saturating_mul(1000);
        self.enrol_ips.retain(|_, v| {
            v.retain(|t| now.saturating_sub(*t) < win);
            !v.is_empty()
        });
        self.clients.len() != before
    }
    fn snapshot(&mut self) -> Snap {
        self.version += 1;
        let mut list: Vec<&ClientRec> = self.clients.values().collect();
        list.sort_by_key(|c| c.created_ms);
        Snap { ver: self.version, json: serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into()) }
    }
}

/// A log file with size-based rotation: path, path.1 .. path.keep.
struct Rot {
    path: PathBuf,
    file: std::fs::File,
    size: u64,
    max: u64,
    keep: usize,
}
impl Rot {
    fn open(path: PathBuf, max: u64, keep: usize) -> std::io::Result<Rot> {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = open_private(&path, false)?;
        Ok(Rot { path, file, size, max, keep })
    }
    fn write(&mut self, line: &str) {
        if self.size + line.len() as u64 > self.max {
            let num = |i: usize| PathBuf::from(format!("{}.{i}", self.path.display()));
            for i in (1..self.keep).rev() {
                let _ = std::fs::rename(num(i), num(i + 1));
            }
            let _ = std::fs::rename(&self.path, num(1));
            if let Ok(f) = open_private(&self.path, true) {
                self.file = f;
            }
            self.size = 0;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size += line.len() as u64;
        }
    }
}
struct Noise {
    count: u64,
    last_ms: u64,
}
pub struct App {
    cfg: Config,
    data: Mutex<Data>,
    save_lock: Mutex<u64>,
    req_log: Mutex<Rot>,
    sec_log: Mutex<Rot>,
    noise: Mutex<Noise>,
    console_token: String,
    http: reqwest::Client,
    /// The state files read cleanly at start (an absent file is a fresh node). /healthz reports it.
    state_ok: bool,
}

fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default()
}
fn sha_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
fn rand_b64(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        d |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    d == 0
}
fn short(h: &str) -> String {
    h.chars().take(8).collect()
}
/// Cap an attacker-supplied string before it reaches a log.
fn cap(s: &str) -> String {
    s.chars().take(STR_CAP).collect()
}
/// Files are readable by the owner only. Unix: mode 0600. Windows: files under the user's profile inherit an
/// ACL that admits only that user, SYSTEM and Administrators; the state directory must live there.
#[cfg(unix)]
fn owner_only(o: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    o.mode(0o600);
}
#[cfg(not(unix))]
fn owner_only(_o: &mut std::fs::OpenOptions) {}
fn open_private(path: &std::path::Path, truncate: bool) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.create(true).write(true);
    if truncate { o.truncate(true); } else { o.append(true); }
    owner_only(&mut o);
    o.open(path)
}
fn write_private(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", rand_b64(6)));
    let res = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.create_new(true).write(true);
        owner_only(&mut o);
        let mut f = o.open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// The routine webhook must be https; plain http only to loopback (tests, local fakes). No credentials in the url.
pub fn webhook_url_ok(u: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(u) else { return false };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else { return false };
    match url.scheme() {
        "https" => true,
        "http" => {
            let h = host.trim_matches(|c| c == '[' || c == ']');
            match h.parse::<IpAddr>() {
                Ok(ip) => ip.is_loopback(),
                Err(_) => h.eq_ignore_ascii_case("localhost"),
            }
        }
        _ => false,
    }
}

type ErrT = (StatusCode, Json<Value>);
type Res = Result<Json<Value>, ErrT>;
fn err(code: u16, msg: &str) -> ErrT {
    (StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(json!({"error": msg})))
}

const SECURITY_EVENTS: &[&str] = &[
    "enrol", "approved", "revoked", "revoke_all", "session_created", "finish_mismatch", "webhook_fail",
    "auth_fail", "auth_revoked", "start_rate_limited", "save_failed", "client_created", "mcp_session_created",
];

impl App {
    pub fn routine_configured(&self) -> bool {
        self.routine().is_some()
    }
    fn routine(&self) -> Option<(String, String)> {
        let t = std::fs::read_to_string(self.cfg.state_dir.join("routine.toml")).ok()?;
        let v: toml::Value = t.parse().ok()?;
        let url = v.get("url")?.as_str()?.to_string();
        let key = v.get("key")?.as_str()?.to_string();
        if !webhook_url_ok(&url) {
            return None;
        }
        Some((url, key))
    }
    fn dat(&self) -> MutexGuard<'_, Data> {
        lk(&self.data)
    }
    fn event(&self, name: &str, extra: Value) {
        let mut v = json!({"time": now_ms(), "event": name});
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        let line = format!("{v}\n");
        let log = if SECURITY_EVENTS.contains(&name) { &self.sec_log } else { &self.req_log };
        lk(log).write(&line);
    }
    /// Count unauthenticated noise in memory; write one aggregate line at most once per window.
    fn noise(&self) {
        let now = now_ms();
        let flush = {
            let mut n = lk(&self.noise);
            n.count += 1;
            if now.saturating_sub(n.last_ms) >= self.cfg.noise_flush_s.saturating_mul(1000) {
                let c = n.count;
                let w = now.saturating_sub(n.last_ms);
                n.count = 0;
                n.last_ms = now;
                Some((c, w))
            } else {
                None
            }
        };
        if let Some((c, w)) = flush {
            self.event("noise_summary", json!({"requests": c, "window_ms": w}));
        }
    }
    /// MCP sessions survive a restart of this node (an update or a rollback; plan 6.7 9a).
    fn save_msessions(&self, d: &Data) {
        let list: Vec<&Session> = d.msessions.values().collect();
        if let Err(e) = write_private(&self.cfg.state_dir.join("msessions.json"), &serde_json::to_string(&list).unwrap_or_default()) {
            self.event("state_write_error", json!({"file": "msessions.json", "detail": e.kind().to_string()}));
        }
    }
    fn persist(&self, s: Snap) -> std::io::Result<()> {
        let mut last = lk(&self.save_lock);
        if s.ver <= *last {
            return Ok(()); // a newer snapshot was already written
        }
        write_private(&self.cfg.state_dir.join("clients.json"), &s.json)?;
        *last = s.ver;
        Ok(())
    }
    fn persist_or_log(&self, s: Snap) -> Result<(), ErrT> {
        self.persist(s).map_err(|e| {
            self.event("save_failed", json!({"kind": format!("{:?}", e.kind())}));
            err(500, "could not save the client list")
        })
    }
    fn housekeeping(&self) {
        let snap = {
            let mut d = self.dat();
            if d.sweep(&self.cfg, now_ms()) { Some(d.snapshot()) } else { None }
        };
        if let Some(s) = snap {
            let _ = self.persist_or_log(s);
        }
    }
    /// Verify `x-gboost-client: id:secret`. A wrong secret, an unknown id and a revoked client all get the
    /// same generic 401.
    fn auth_client(&self, h: &HeaderMap, need_approved: bool) -> Result<ClientRec, ErrT> {
        let raw = h.get("x-gboost-client").and_then(|v| v.to_str().ok()).ok_or_else(|| err(401, "client credentials required"))?;
        let (id, secret) = raw.split_once(':').ok_or_else(|| err(401, "client credentials required"))?;
        let now = now_ms();
        let mut d = self.dat();
        let rec = d.clients.get(id).cloned();
        // compare against a dummy when unknown, to keep timing similar
        let stored = rec.as_ref().map(|r| r.secret_hash.clone()).unwrap_or_else(|| "0".repeat(64));
        let ok = ct_eq(&stored, &sha_hex(secret)) && rec.is_some();
        let revoked = ok && rec.as_ref().map(|r| r.state == "revoked").unwrap_or(false);
        if !ok || revoked {
            // security log only for a known client id, at most once per gap per client
            let log = rec.is_some() && {
                let last = d.authfail_logged.get(id).copied().unwrap_or(0);
                if now.saturating_sub(last) >= AUTHFAIL_LOG_GAP_MS {
                    d.authfail_logged.insert(id.to_string(), now);
                    true
                } else {
                    false
                }
            };
            drop(d);
            if log {
                self.event(if revoked { "auth_revoked" } else { "auth_fail" }, json!({"client_id": short(id)}));
            }
            return Err(err(401, "bad client credentials"));
        }
        drop(d);
        let rec = rec.ok_or_else(|| err(401, "bad client credentials"))?;
        if need_approved && rec.state != "approved" {
            return Err(err(403, "client not approved yet; ask the owner to approve this fingerprint"));
        }
        Ok(rec)
    }
    fn auth_session(&self, h: &HeaderMap, client: &ClientRec, count: bool) -> Result<Session, ErrT> {
        let tok = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).ok_or_else(|| err(401, "session required"))?;
        let key = sha_hex(tok);
        let mut d = self.dat();
        let s = d.sessions.get_mut(&key).ok_or_else(|| err(401, "invalid session"))?;
        if s.client_id != client.id {
            return Err(err(401, "invalid session"));
        }
        if now_ms() >= s.expires_ms {
            d.sessions.remove(&key);
            return Err(err(401, "session expired"));
        }
        if count {
            if s.used >= self.cfg.session_budget {
                return Err(err(429, "session call budget exhausted"));
            }
            s.used += 1;
        }
        Ok(Session { id: s.id.clone(), client_id: s.client_id.clone(), created_ms: s.created_ms, expires_ms: s.expires_ms, used: s.used })
    }
}

pub fn build(cfg: Config) -> std::io::Result<(Router, Router, Arc<App>)> {
    std::fs::create_dir_all(&cfg.state_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cfg.state_dir, std::fs::Permissions::from_mode(0o700));
    }
    let tok_path = cfg.state_dir.join("console.token");
    let console_token = match std::fs::read_to_string(&tok_path) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            let t = rand_b64(24);
            write_private(&tok_path, &t)?;
            t
        }
    };
    let mut data = Data::default();
    let mut state_ok = true;
    if let Ok(t) = std::fs::read_to_string(cfg.state_dir.join("clients.json")) {
        let parsed = serde_json::from_str::<Vec<ClientRec>>(&t);
        state_ok &= parsed.is_ok();
        if let Ok(list) = parsed {
            for mut c in list {
                if c.state == "revoked" && c.revoked_ms.is_none() {
                    c.revoked_ms = Some(now_ms());
                }
                data.clients.insert(c.id.clone(), c);
            }
        }
    }
    if let Ok(t) = std::fs::read_to_string(cfg.state_dir.join("msessions.json")) {
        if let Ok(list) = serde_json::from_str::<Vec<Session>>(&t) {
            let now = now_ms();
            for s in list {
                let ok = data.clients.get(&s.client_id).map(|c| c.state == "approved").unwrap_or(false);
                if ok && now < s.expires_ms {
                    data.msessions.insert(s.client_id.clone(), s);
                }
            }
        }
    }
    let req_log = Rot::open(cfg.state_dir.join("requests.log"), REQ_LOG_MAX, 1)?;
    let sec_log = Rot::open(cfg.state_dir.join("security.log"), SEC_LOG_MAX, 3)?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.webhook_timeout_s))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let app = Arc::new(App {
        cfg,
        data: Mutex::new(data),
        save_lock: Mutex::new(0),
        req_log: Mutex::new(req_log),
        sec_log: Mutex::new(sec_log),
        noise: Mutex::new(Noise { count: 0, last_ms: now_ms() }),
        console_token,
        http,
        state_ok,
    });

    let agent = Router::new()
        .route("/client/gboost_client.py", get(client_file))
        .route("/enroll", post(enroll))
        .route("/session/start", post(session_start))
        .route("/session/finish", post(session_finish))
        .route("/rpc", post(rpc))
        .route("/mcp", post(mcp).get(|| async { StatusCode::METHOD_NOT_ALLOWED }))
        .layer(axum::extract::DefaultBodyLimit::max(BODY_MAX))
        .layer(middleware::from_fn_with_state(app.clone(), timeout_mw))
        .layer(middleware::from_fn_with_state(app.clone(), access_log))
        .with_state(app.clone());
    let owner = Router::new()
        .route("/healthz", get(healthz))
        .route("/local/clients", get(list_clients))
        .route("/local/clients/new", post(new_client))
        .route("/local/clients/{id}/approve", post(approve))
        .route("/local/clients/{id}/revoke", post(revoke))
        .route("/local/sessions/revoke-all", post(revoke_all))
        .layer(middleware::from_fn_with_state(app.clone(), console_auth))
        .layer(middleware::from_fn_with_state(app.clone(), access_log))
        .with_state(app.clone());
    Ok((agent, owner, app))
}

/// Owner listener (loopback only), no token: the updater's health check. Version only, no state.
async fn healthz(State(app): State<Arc<App>>) -> Json<Value> {
    let state_ok = app.state_ok && std::fs::metadata(&app.cfg.state_dir).map(|m| m.is_dir()).unwrap_or(false);
    Json(json!({"ok": state_ok, "version": std::env::var("GBOOST_VERSION").unwrap_or_else(|_| "dev".into())}))
}

async fn access_log(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    app.housekeeping();
    let (m, p) = (req.method().to_string(), cap(req.uri().path()));
    let resp = next.run(req).await;
    let st = resp.status().as_u16();
    // unauthenticated noise is counted, not logged per request
    if matches!(st, 400 | 401 | 404 | 405 | 408 | 413 | 415 | 422 | 429) || p == "/client/gboost_client.py" {
        app.noise();
    } else {
        app.event("request", json!({"method": m, "path": p, "status": st}));
    }
    resp
}
async fn timeout_mw(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    match tokio::time::timeout(Duration::from_secs(app.cfg.request_timeout_s), next.run(req)).await {
        Ok(r) => r,
        Err(_) => err(408, "request timed out").into_response(),
    }
}
async fn console_auth(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    // The health check (version only) is the one route without the token: the updater calls it.
    if req.method() == axum::http::Method::GET && req.uri().path() == "/healthz" {
        return next.run(req).await;
    }
    let ok = req.headers().get("x-console-token").and_then(|v| v.to_str().ok()).map(|t| ct_eq(t, &app.console_token)).unwrap_or(false);
    if !ok {
        return err(401, "console token required").into_response();
    }
    next.run(req).await
}

async fn client_file() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/x-python; charset=utf-8")], CLIENT_FILE)
}

#[derive(Deserialize)]
struct EnrolReq {
    client_id: String,
    secret: String,
}
async fn enroll(State(app): State<Arc<App>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Json(r): Json<EnrolReq>) -> Res {
    let id_ok = r.client_id.starts_with("gbc_") && r.client_id.len() >= 12 && r.client_id.len() <= 64
        && r.client_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !id_ok || r.secret.len() < 32 || r.secret.len() > 256 {
        return Err(err(400, "bad client_id or secret (secret needs at least 32 characters)"));
    }
    let h = sha_hex(&r.secret);
    let fp: String = h.chars().take(16).collect();
    let now = now_ms();
    let snap = {
        let mut d = app.dat();
        d.sweep(&app.cfg, now);
        if d.clients.contains_key(&r.client_id) {
            return Err(err(409, "client id already enrolled"));
        }
        let ip = peer.ip();
        if !d.enrol_ips.contains_key(&ip) && d.enrol_ips.len() >= MAX_IP_ENTRIES {
            return Err(err(429, "too many enrolment attempts; try again later"));
        }
        if d.enrol_ips.get(&ip).map(|v| v.len()).unwrap_or(0) >= app.cfg.enrol_per_ip {
            return Err(err(429, "too many enrolments from this address; try again later"));
        }
        if d.clients.values().filter(|c| c.state == "pending").count() >= app.cfg.max_pending_enrol {
            return Err(err(429, "too many pending enrolments; the owner must approve or revoke some"));
        }
        if d.clients.len() >= app.cfg.max_clients {
            return Err(err(429, "too many clients; the owner must revoke some"));
        }
        d.enrol_ips.entry(ip).or_default().push(now);
        d.clients.insert(r.client_id.clone(), ClientRec { id: r.client_id.clone(), secret_hash: h, fingerprint: fp.clone(), state: "pending".into(), created_ms: now, revoked_ms: None });
        d.snapshot()
    };
    app.persist_or_log(snap)?;
    app.event("enrol", json!({"client_id": r.client_id, "fingerprint": fp}));
    Ok(Json(json!({"state": "pending", "client_id": r.client_id, "fingerprint": fp})))
}

enum Check {
    Sent(String),
    AlreadyPending,
}
const COOLDOWN_MSG: &str = "a session check was sent recently; wait";
const MCP_COOLDOWN_MSG: &str = "A session check was sent recently; wait.";

/// Removes the pending code of one start on any exit (failure, timeout, cancellation) unless disarmed,
/// and only while its hash still matches (a newer start's code stays).
struct CodeGuard<'a> {
    app: &'a App,
    id: String,
    hash: String,
    armed: bool,
}
impl Drop for CodeGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let mut d = lk(&self.app.data);
            if d.codes.get(&self.id).map(|p| p.hash == self.hash).unwrap_or(false) {
                d.codes.remove(&self.id);
            }
        }
    }
}

/// Create a code, post it to the routine webhook. With `skip_if_pending`, an unexpired pending code
/// means nothing is sent (decided under the same lock that inserts the new code). Starts are limited by a
/// per-client cooldown, a per-client hourly count and a global hourly cap; attempts count even when the
/// webhook fails, because the webhook may have received them.
async fn start_check(app: &App, c: &ClientRec, skip_if_pending: bool) -> Result<Check, ErrT> {
    let (url, key) = app.routine().ok_or_else(|| err(503, "routine webhook not configured: state/routine.toml needs an https url (http only to loopback) and key"))?;
    let code = rand_b64(16);
    let check_id = rand_b64(6);
    let hash = sha_hex(&code);
    let mut pending = false;
    let limited: Option<&'static str> = {
        let mut d = app.dat();
        let now = now_ms();
        if skip_if_pending && d.codes.get(&c.id).map(|p| now < p.expires_ms).unwrap_or(false) {
            pending = true;
            None
        } else {
            d.global_starts.retain(|t| now.saturating_sub(*t) < 3_600_000);
            let recent = d.last_check.get(&c.id).map(|t| now.saturating_sub(*t) < app.cfg.check_cooldown_s.saturating_mul(1000)).unwrap_or(false);
            let global_full = d.global_starts.len() >= app.cfg.max_checks_per_hour_global;
            let v = d.starts.entry(c.id.clone()).or_default();
            v.retain(|t| now.saturating_sub(*t) < 3_600_000);
            if recent {
                Some(COOLDOWN_MSG)
            } else if v.len() >= app.cfg.max_starts_per_hour || global_full {
                Some("too many session starts this hour")
            } else {
                v.push(now);
                d.global_starts.push(now);
                d.last_check.insert(c.id.clone(), now);
                d.codes.insert(c.id.clone(), PendingCode { hash: hash.clone(), expires_ms: now + app.cfg.code_ttl_s * 1000 });
                None
            }
        }
    };
    if pending {
        return Ok(Check::AlreadyPending);
    }
    if let Some(m) = limited {
        app.event("start_rate_limited", json!({"client_id": c.id, "reason": m}));
        return Err(err(429, m));
    }
    let mut guard = CodeGuard { app, id: c.id.clone(), hash: hash.clone(), armed: true };
    let body = json!({"source": "gboost", "kind": "session_check", "code": code, "check_id": check_id, "client_fingerprint": c.fingerprint});
    let sent = tokio::time::timeout(Duration::from_millis(app.cfg.webhook_post_timeout_ms), app.http.post(&url).bearer_auth(&key).json(&body).send()).await;
    let ok = matches!(&sent, Ok(Ok(r)) if r.status().is_success());
    if !ok {
        let st = match &sent {
            Ok(Ok(r)) => r.status().as_u16().to_string(),
            Ok(Err(_)) => "transport error".into(),
            Err(_) => "timeout".into(),
        };
        drop(guard); // removes this start's code
        app.event("webhook_fail", json!({"client_id": c.id, "detail": st}));
        return Err(err(502, "routine webhook did not accept the session check"));
    }
    guard.armed = false;
    let http_status = match &sent { Ok(Ok(r)) => r.status().as_u16(), _ => 0 };
    app.event("session_check_sent", json!({"client_id": c.id, "code_hash": short(&hash), "http_status": http_status}));
    Ok(Check::Sent(check_id))
}

async fn session_start(State(app): State<Arc<App>>, h: HeaderMap) -> Res {
    let c = app.auth_client(&h, true)?;
    match start_check(&app, &c, false).await? {
        Check::Sent(check_id) => Ok(Json(json!({"pending": true, "expires_in": app.cfg.code_ttl_s, "check_id": check_id}))),
        Check::AlreadyPending => Err(err(500, "unexpected")),
    }
}

/// Verify and consume the pending code for a client (single use, expiry, hash compare) under the
/// caller's lock. On failure returns the error and an optional event to log after the lock is dropped.
fn take_code(d: &mut Data, c: &ClientRec, code: &str) -> Result<(), (ErrT, Option<&'static str>)> {
    if d.clients.get(&c.id).map(|x| x.state != "approved").unwrap_or(true) {
        return Err((err(401, "bad client credentials"), None));
    }
    let (expires, hash) = match d.codes.get(&c.id) {
        Some(p) => (p.expires_ms, p.hash.clone()),
        None => return Err((err(401, "no pending session check"), None)),
    };
    if now_ms() >= expires {
        d.codes.remove(&c.id);
        return Err((err(401, "code expired"), Some("finish_expired")));
    }
    if !ct_eq(&hash, &sha_hex(code.trim())) {
        return Err((err(401, "code does not match"), Some("finish_mismatch")));
    }
    d.codes.remove(&c.id); // single use
    Ok(())
}

#[derive(Deserialize)]
struct FinishReq {
    code: String,
}
async fn session_finish(State(app): State<Arc<App>>, h: HeaderMap, Json(r): Json<FinishReq>) -> Res {
    let c = app.auth_client(&h, true)?;
    let mut d = app.dat();
    if let Err((e, ev)) = take_code(&mut d, &c, &r.code) {
        drop(d);
        if let Some(ev) = ev {
            app.event(ev, json!({"client_id": c.id}));
        }
        return Err(e);
    }
    d.sessions.retain(|_, s| s.client_id != c.id); // a new session replaces the older ones
    let token = format!("gbs_{}", rand_b64(24));
    let now = now_ms();
    let id = short(&sha_hex(&token));
    d.sessions.insert(sha_hex(&token), Session { id: id.clone(), client_id: c.id.clone(), created_ms: now, expires_ms: now + app.cfg.session_ttl_s * 1000, used: 0 });
    drop(d);
    app.event("session_created", json!({"client_id": c.id, "session": id}));
    Ok(Json(json!({"session": token, "expires_in": app.cfg.session_ttl_s, "budget": app.cfg.session_budget})))
}

#[derive(Deserialize)]
struct RpcReq {
    tool: String,
    #[serde(default)]
    args: Value,
}
async fn rpc(State(app): State<Arc<App>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, Json(r): Json<RpcReq>) -> Res {
    let c = app.auth_client(&h, true)?;
    let s = app.auth_session(&h, &c, true)?;
    app.event("rpc", json!({"client_id": c.id, "session": s.id, "tool": cap(&r.tool)}));
    let text = match r.tool.as_str() {
        "hello" => {
            let n = r.args.get("name").and_then(|v| v.as_str()).unwrap_or("world");
            let n: String = n.chars().take(100).collect();
            format!("Hello, {n}!")
        }
        "whoami" => json!({
            "client_id": c.id, "fingerprint": c.fingerprint, "session": s.id,
            "created_ms": s.created_ms, "expires_ms": s.expires_ms,
            "calls_used": s.used, "calls_budget": app.cfg.session_budget,
            "peer": peer.to_string(),
        }).to_string(),
        "pending_request" => json!({
            "request_id": "req_demo_0001", "state": "pending", "expires_ms": now_ms() + 600_000,
            "text": "Demo request waiting for the owner. Open the GBOOST approver on the phone to decide."
        }).to_string(),
        _ => return Err(err(404, "unknown tool")),
    };
    Ok(Json(json!({"text": text})))
}

async fn list_clients(State(app): State<Arc<App>>) -> Json<Value> {
    let d = app.dat();
    let mut v: Vec<Value> = d.clients.values().map(|c| json!({"id": c.id, "fingerprint": c.fingerprint, "state": c.state, "created_ms": c.created_ms})).collect();
    v.sort_by_key(|x| x["created_ms"].as_u64());
    Json(json!({"clients": v}))
}
/// Check and set the state under one lock, so approve cannot overtake a revoke.
fn set_state(app: &App, id: &str, state: &str) -> Res {
    let snap = {
        let mut d = app.dat();
        let cur = d.clients.get(id).map(|c| c.state.clone()).ok_or_else(|| err(404, "unknown client"))?;
        if state == "approved" && cur == "revoked" {
            return Err(err(409, "client is revoked"));
        }
        if let Some(c) = d.clients.get_mut(id) {
            c.state = state.into();
            if state == "revoked" && cur != "revoked" {
                c.revoked_ms = Some(now_ms());
            }
        }
        if state == "revoked" {
            d.sessions.retain(|_, s| s.client_id != id);
            d.msessions.remove(id);
            d.codes.remove(id);
            app.save_msessions(&d);
        }
        d.snapshot()
    };
    app.persist_or_log(snap)?;
    app.event(state, json!({"client_id": id}));
    Ok(Json(json!({"id": id, "state": state})))
}
async fn approve(State(app): State<Arc<App>>, Path(id): Path<String>) -> Res {
    set_state(&app, &id, "approved")
}
async fn revoke(State(app): State<Arc<App>>, Path(id): Path<String>) -> Res {
    set_state(&app, &id, "revoked")
}
async fn revoke_all(State(app): State<Arc<App>>) -> Json<Value> {
    let mut d = app.dat();
    let n = d.sessions.len();
    d.sessions.clear();
    d.msessions.clear();
    d.codes.clear();
    app.save_msessions(&d);
    drop(d);
    app.event("revoke_all", json!({"sessions": n}));
    Json(json!({"revoked": n}))
}

impl App {
    pub fn console_token(&self) -> &str {
        &self.console_token
    }
}

#[derive(Deserialize)]
struct NewClientQ {
    #[serde(default)]
    to_file: Option<String>,
}
/// Owner: make a client that is already approved. The secret appears once, in the response or in
/// state/new-client.txt (0600) with `?to_file=1`.
async fn new_client(State(app): State<Arc<App>>, Query(q): Query<NewClientQ>) -> Res {
    let id = format!("gbc_{}", rand_b64(9));
    let secret = rand_b64(32);
    let h = sha_hex(&secret);
    let fp: String = h.chars().take(16).collect();
    let now = now_ms();
    let to_file = matches!(q.to_file.as_deref(), Some("1") | Some("true"));
    let path = app.cfg.state_dir.join("new-client.txt");
    if to_file && path.exists() {
        return Err(err(409, "state/new-client.txt already exists; read and delete it first"));
    }
    let snap = {
        let mut d = app.dat();
        d.sweep(&app.cfg, now);
        if d.clients.len() >= app.cfg.max_clients {
            return Err(err(429, "too many clients; revoke some"));
        }
        d.clients.insert(id.clone(), ClientRec { id: id.clone(), secret_hash: h, fingerprint: fp.clone(), state: "approved".into(), created_ms: now, revoked_ms: None });
        d.snapshot()
    };
    if let Err(e) = app.persist_or_log(snap) {
        app.dat().clients.remove(&id); // roll back: the client was never saved
        return Err(e);
    }
    app.event("client_created", json!({"client_id": id, "fingerprint": fp}));
    let value = format!("{id}:{secret}");
    if to_file {
        write_private(&path, &format!("{value}\n")).map_err(|_| err(500, "could not write the key file"))?;
        return Ok(Json(json!({"client_id": id, "fingerprint": fp, "file": path.display().to_string()})));
    }
    Ok(Json(json!({"client_id": id, "header_name": "x-gboost-client", "header_value": value, "fingerprint": fp})))
}

// ---- MCP Streamable HTTP (plain JSON responses), Remote connector ----

const MCP_FAIL: &str = "Session check failed.";
const MCP_PENDING_MSG: &str = "GBOOST sent a session check to your routine webhook. When the routine has called gboost_deliver_code, call this tool again.";

fn mcp_tools() -> Value {
    let none = json!({"type": "object", "properties": {}});
    json!([
        {"name": "gboost_hello", "description": "Greets. Needs an open GBOOST session.", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}}},
        {"name": "gboost_whoami", "description": "Shows the calling client, its session and what the server sees. Needs an open GBOOST session.", "inputSchema": none},
        {"name": "gboost_pending_request", "description": "Shows a demo pending approval. Needs an open GBOOST session.", "inputSchema": none},
        {"name": "gboost_deliver_code", "description": "Only for the routine run woken by GBOOST with kind session_check: pass the `code` from the payload unchanged. Do not call it otherwise.",
         "inputSchema": {"type": "object", "properties": {"code": {"type": "string"}, "check_id": {"type": "string"}}, "required": ["code"]}},
        {"name": "gboost_status", "description": "Says whether a GBOOST session is open, when it expires and whether a session check is pending.", "inputSchema": none}
    ])
}
fn tool_text(t: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": t}], "isError": is_error})
}
fn rpc_err(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}
fn hdr_cap(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(cap)
}

impl App {
    /// An open MCP session for the client; counts one call against its budget.
    fn mcp_use(&self, c: &ClientRec) -> Result<Session, &'static str> {
        let mut d = self.dat();
        let now = now_ms();
        if d.clients.get(&c.id).map(|x| x.state != "approved").unwrap_or(true) {
            return Err(MCP_FAIL);
        }
        let Some(s) = d.msessions.get_mut(&c.id) else { return Err("") };
        if now >= s.expires_ms {
            d.msessions.remove(&c.id);
            self.save_msessions(&d);
            return Err("");
        }
        if s.used >= self.cfg.session_budget {
            return Err("GBOOST session call budget exhausted.");
        }
        s.used += 1;
        let out = Session { id: s.id.clone(), client_id: s.client_id.clone(), created_ms: s.created_ms, expires_ms: s.expires_ms, used: s.used };
        // Saved on create, revoke and expiry, and every 10 calls: a crash can give back at most 9 calls of budget.
        if out.used % 10 == 0 {
            self.save_msessions(&d);
        }
        Ok(out)
    }
    fn mcp_state(&self, c: &ClientRec) -> (Option<Session>, bool) {
        let d = self.dat();
        let now = now_ms();
        let s = d.msessions.get(&c.id).filter(|s| now < s.expires_ms).map(|s| Session { id: s.id.clone(), client_id: s.client_id.clone(), created_ms: s.created_ms, expires_ms: s.expires_ms, used: s.used });
        let pending = d.codes.get(&c.id).map(|p| now < p.expires_ms).unwrap_or(false);
        (s, pending)
    }
}

async fn mcp(State(app): State<Arc<App>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, h: HeaderMap, body: axum::body::Bytes) -> Response {
    // Generic 401 for every failure; no WWW-Authenticate, so the host does not start OAuth discovery.
    let c = match app.auth_client(&h, true) {
        Ok(c) => c,
        Err(_) => return (StatusCode::UNAUTHORIZED, Json(rpc_err(Value::Null, -32001, "unauthorized"))).into_response(),
    };
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return Json(rpc_err(Value::Null, -32700, "parse error")).into_response();
    };
    let method = msg["method"].as_str().unwrap_or("").to_string();
    let Some(id) = msg.get("id").cloned() else { return StatusCode::ACCEPTED.into_response() }; // notification
    let mut new_sid = None;
    let result = match method.as_str() {
        "initialize" => {
            let want = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let v = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"].into_iter().find(|x| *x == want).unwrap_or("2025-06-18");
            new_sid = Some(format!("gbm_{}", rand_b64(12)));
            json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": v, "capabilities": {"tools": {}}, "serverInfo": {"name": "gboost-spike", "version": "0.0.0"},
                "instructions": "GBOOST. Tools need an open session; the first call triggers a session check through the owner's routine."}})
        }
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": mcp_tools()}}),
        "tools/call" => {
            let args = &msg["params"]["arguments"];
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let r = match name {
                "gboost_deliver_code" => {
                    let code = args["code"].as_str().unwrap_or("");
                    let mut d = app.dat();
                    match take_code(&mut d, &c, code) {
                        Ok(()) => {
                            let now = now_ms();
                            if d.msessions.get(&c.id).map(|s| now < s.expires_ms).unwrap_or(false) {
                                // keep the open session: no reset of its budget or expiry
                                drop(d);
                                tool_text("Session already open", false)
                            } else {
                                let sid = format!("gbs_{}", rand_b64(6));
                                d.msessions.insert(c.id.clone(), Session { id: sid.clone(), client_id: c.id.clone(), created_ms: now, expires_ms: now + app.cfg.session_ttl_s * 1000, used: 0 });
                                app.save_msessions(&d);
                                drop(d);
                                app.event("mcp_session_created", json!({"client_id": c.id, "session": sid}));
                                tool_text("Session open", false)
                            }
                        }
                        Err((_, ev)) => {
                            drop(d);
                            if let Some(ev) = ev {
                                app.event(ev, json!({"client_id": c.id}));
                            }
                            tool_text(MCP_FAIL, true)
                        }
                    }
                }
                "gboost_status" => {
                    let (s, pending) = app.mcp_state(&c);
                    tool_text(&json!({"session_active": s.is_some(), "expires_ms": s.map(|s| s.expires_ms), "check_pending": pending}).to_string(), false)
                }
                "gboost_hello" | "gboost_whoami" | "gboost_pending_request" => match app.mcp_use(&c) {
                    Ok(s) => {
                        app.event("mcp_call", json!({"client_id": c.id, "session": s.id, "tool": name}));
                        match name {
                            "gboost_hello" => {
                                let n: String = args["name"].as_str().unwrap_or("world").chars().take(100).collect();
                                tool_text(&format!("Hello, {n}!"), false)
                            }
                            "gboost_whoami" => tool_text(&json!({
                                "client_id": c.id, "fingerprint": c.fingerprint, "session": s.id,
                                "created_ms": s.created_ms, "expires_ms": s.expires_ms,
                                "calls_used": s.used, "calls_budget": app.cfg.session_budget,
                                "claimed_by_caller": {"mcp_session_id": hdr_cap(&h, "mcp-session-id"), "x_forwarded_for": hdr_cap(&h, "x-forwarded-for")},
                                "seen": {"peer": peer.to_string()},
                                "note": "claimed_by_caller values are sent by the caller and prove nothing; seen is what the server observed. Neither is an identity.",
                            }).to_string(), false),
                            _ => tool_text(&json!({
                                "request_id": "req_demo_0001", "state": "pending", "expires_ms": now_ms() + 600_000,
                                "text": "Demo request waiting for the owner. Open the GBOOST approver on the phone to decide."
                            }).to_string(), false),
                        }
                    }
                    Err("") => match start_check(&app, &c, true).await {
                        Ok(Check::Sent(_)) | Ok(Check::AlreadyPending) => tool_text(MCP_PENDING_MSG, true),
                        Err((st, j)) if st == StatusCode::TOO_MANY_REQUESTS && j.0["error"] == COOLDOWN_MSG => tool_text(MCP_COOLDOWN_MSG, true),
                        Err((st, _)) if st == StatusCode::TOO_MANY_REQUESTS => tool_text("Too many session checks this hour; try later.", true),
                        Err((st, _)) if st == StatusCode::SERVICE_UNAVAILABLE => tool_text("GBOOST has no routine webhook configured.", true),
                        Err(_) => tool_text("GBOOST could not reach your routine webhook.", true),
                    },
                    Err(m) => tool_text(m, true),
                },
                other => return Json(rpc_err(id, -32602, &format!("unknown tool {}", cap(other)))).into_response(),
            };
            json!({"jsonrpc": "2.0", "id": id, "result": r})
        }
        _ => rpc_err(id, -32601, "method not found"),
    };
    let mut resp = Json(result).into_response();
    if let Some(sid) = new_sid {
        if let Ok(v) = header::HeaderValue::from_str(&sid) {
            resp.headers_mut().insert("mcp-session-id", v);
        }
    }
    resp
}
