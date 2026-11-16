//! Spike S4: a hello-world Bastion core as a Remote HTTPS MCP connector with OAuth 2.1.
//! Hand-rolled JSON-RPC over axum (rmcp not used: we need exact control of what is logged).
use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Form, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Config {
    pub public_url: String,
    pub state_dir: PathBuf,
    pub access_ttl_ms: u64,
    /// Dynamic client registration; off unless BASTION_ALLOW_DCR=1.
    pub allow_dcr: bool,
    /// Hard cap on each in-memory map.
    pub max_entries: usize,
    /// Rotate requests.log at this size, keeping one old file.
    pub log_max_bytes: u64,
    /// When set, the fixed client is a public PKCE client (as Grok's connector backend behaves): it may
    /// omit its secret at /token, and its redirect must equal this URI exactly.
    pub public_redirect: Option<String>,
}
impl Config {
    pub fn new(public_url: String, state_dir: PathBuf, access_ttl_ms: u64) -> Config {
        Config { public_url, state_dir, access_ttl_ms, allow_dcr: false, max_entries: 2000, log_max_bytes: 10 * 1024 * 1024, public_redirect: None }
    }
}
const LOG_VALUE_MAX: usize = 512;
const BODY_MAX: usize = 64 * 1024;
const PIN_MAX_FAILS: u32 = 5;
const WAKE_MAX_SECS: u64 = 600;
const WAKE_MIN_GAP_MS: u64 = 60_000;

struct Client {
    secret_hash: Option<String>,
    redirect_uris: Vec<String>,
    fixed: bool,
    created: u64, // u64::MAX for the fixed client so it is evicted last
}
struct AuthReq {
    client_id: String,
    redirect_uri: String,
    state: Option<String>,
    challenge: String,
    scope: String,
    created: u64,
}
struct CodeRec {
    client_id: String,
    redirect_uri: String,
    challenge: String,
    scope: String,
    expires: u64,
}
#[derive(Clone)]
struct Tok {
    id: String,
    client_id: String,
    expires: u64,
    created: u64,
}
struct RefRec {
    tid: String,
    cid: String,
    created: u64,
}
struct Sess {
    count: u64,
    created: u64,
    tid: String,
}
#[derive(Default)]
struct Data {
    clients: HashMap<String, Client>,
    pending: HashMap<String, AuthReq>,
    codes: HashMap<String, CodeRec>,
    tokens: HashMap<String, Tok>,
    refresh: HashMap<String, RefRec>,
    used_codes: HashMap<String, (String, u64)>, // code hash -> (token id, time)
    sessions: HashMap<String, Sess>,
    token_counts: HashMap<String, (u64, u64)>, // token id -> (count, created)
    pin_fails: u32,
    wake_pending: bool,
    last_wake_ms: u64,
}
fn make_room<V>(m: &mut HashMap<String, V>, max: usize, created: impl Fn(&V) -> u64) {
    while !m.is_empty() && m.len() >= max {
        let k = m.iter().min_by_key(|(_, v)| created(v)).map(|(k, _)| k.clone()).unwrap();
        m.remove(&k);
    }
}
impl Data {
    /// Drop expired entries, then leave room for one insert in each map (oldest evicted).
    fn sweep(&mut self, now: u64, max: usize) {
        self.pending.retain(|_, p| now.saturating_sub(p.created) < 600_000);
        self.codes.retain(|_, c| c.expires > now);
        self.tokens.retain(|_, t| t.expires > now);
        self.refresh.retain(|_, r| now.saturating_sub(r.created) < 30 * 86_400_000);
        self.used_codes.retain(|_, u| now.saturating_sub(u.1) < 86_400_000);
        make_room(&mut self.clients, max, |c| c.created);
        make_room(&mut self.pending, max, |p| p.created);
        make_room(&mut self.codes, max, |c| c.expires);
        make_room(&mut self.tokens, max, |t| t.created);
        make_room(&mut self.refresh, max, |r| r.created);
        make_room(&mut self.used_codes, max, |u| u.1);
        make_room(&mut self.sessions, max, |s| s.created);
        make_room(&mut self.token_counts, max, |c| c.1);
    }
}
struct LogFile {
    file: std::fs::File,
    size: u64,
}
fn open_private(path: &std::path::Path, truncate: bool) -> std::fs::File {
    let mut o = std::fs::OpenOptions::new();
    o.create(true).write(true);
    if truncate { o.truncate(true); } else { o.append(true); }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).unwrap()
}
fn trunc(s: &str) -> String {
    if s.len() <= LOG_VALUE_MAX {
        return s.to_string();
    }
    let mut i = LOG_VALUE_MAX;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!("{}...[truncated]", &s[..i])
}
/// Constant-time comparison.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        d |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    d == 0
}
pub struct Inner {
    cfg: Config,
    data: Mutex<Data>,
    log: Mutex<LogFile>,
    pin: String,
}
pub type App = Arc<Inner>;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}
fn rand_b64(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    B64.encode(b)
}
fn hash(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
fn rfc3339(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

impl Inner {
    /// The owner PIN (printed on the console at start). Exposed for tests.
    pub fn pin(&self) -> &str {
        &self.pin
    }
    pub fn pending_len(&self) -> usize {
        self.data.lock().unwrap().pending.len()
    }
    fn log_line(&self, v: Value) {
        let line = format!("{v}\n");
        let mut f = self.log.lock().unwrap();
        if f.size + line.len() as u64 > self.cfg.log_max_bytes {
            let path = self.cfg.state_dir.join("requests.log");
            let _ = std::fs::rename(&path, self.cfg.state_dir.join("requests.log.1"));
            f.file = open_private(&path, true);
            f.size = 0;
        }
        if f.file.write_all(line.as_bytes()).is_ok() {
            f.size += line.len() as u64;
        }
    }
    fn event(&self, name: &str, extra: Value) {
        let mut v = json!({"time": rfc3339(now_ms()), "event": name});
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        self.log_line(v);
    }
    /// Valid, unexpired access token for the Authorization header.
    fn authenticate(&self, headers: &HeaderMap) -> Option<Tok> {
        let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let tok = raw.strip_prefix("Bearer ")?;
        let d = self.data.lock().unwrap();
        let t = d.tokens.get(&hash(tok))?;
        if now_ms() < t.expires {
            Some(t.clone())
        } else {
            None
        }
    }
    /// Token id for logging only, even when expired.
    fn token_id_any(&self, headers: &HeaderMap) -> Option<String> {
        let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let tok = raw.strip_prefix("Bearer ")?;
        self.data.lock().unwrap().tokens.get(&hash(tok)).map(|t| t.id.clone())
    }
}

/// Create the app. First start generates the client id and secret; the secret is returned once.
pub fn build(cfg: Config) -> (Router, App, Option<(String, String)>) {
    create_state_dir(&cfg.state_dir);
    let log_path = cfg.state_dir.join("requests.log");
    let log = open_private(&log_path, false);
    let log_size = log.metadata().map(|m| m.len()).unwrap_or(0);
    let pin = format!("{:08}", rand::thread_rng().next_u32() % 100_000_000);
    let mut data = Data::default();
    let cpath = cfg.state_dir.join("client.json");
    let mut fresh = None;
    if let Ok(s) = std::fs::read_to_string(&cpath) {
        let v: Value = serde_json::from_str(&s).unwrap();
        data.clients.insert(
            v["client_id"].as_str().unwrap().into(),
            Client { secret_hash: Some(v["secret_hash"].as_str().unwrap().into()), redirect_uris: vec![], fixed: true, created: u64::MAX },
        );
    } else {
        let id = format!("bastion-{}", rand_b64(9));
        let secret = format!("bcs_{}", rand_b64(32));
        use std::io::Write as _;
        open_private(&cpath, true).write_all(json!({"client_id": id, "secret_hash": hash(&secret)}).to_string().as_bytes()).unwrap();
        data.clients.insert(id.clone(), Client { secret_hash: Some(hash(&secret)), redirect_uris: vec![], fixed: true, created: u64::MAX });
        fresh = Some((id, secret));
    }
    let app = Arc::new(Inner { cfg, data: Mutex::new(data), log: Mutex::new(LogFile { file: log, size: log_size }), pin });
    let router = Router::new()
        .route("/.well-known/oauth-protected-resource", get(prm))
        .route("/.well-known/oauth-protected-resource/mcp", get(prm))
        .route("/.well-known/oauth-authorization-server", get(asm))
        .route("/.well-known/openid-configuration", get(asm))
        .route("/authorize", get(authorize))
        .route("/authorize/decision", post(decision))
        .route("/token", post(token))
        .route("/pending/{id}", get(pending_page))
        .route("/mcp", post(mcp).get(mcp_get).delete(mcp_delete));
    let router = if app.cfg.allow_dcr { router.route("/register", post(register)) } else { router };
    let router = router
        .layer(middleware::from_fn_with_state(app.clone(), observe))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, std::time::Duration::from_secs(30)))
        .with_state(app.clone());
    (router, app, fresh)
}

#[cfg(unix)]
fn create_state_dir(p: &PathBuf) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p);
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn create_state_dir(p: &PathBuf) {
    let _ = std::fs::create_dir_all(p);
}

// ---------- observation log ----------

fn redacted(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "authorization" || n == "cookie" || n == "set-cookie" || n.contains("secret") || n.contains("api-key") || n.contains("token")
}

async fn observe(State(app): State<App>, req: Request<Body>, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = to_bytes(body, BODY_MAX).await else { return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response() };
    let remote = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.to_string());
    let mut headers = serde_json::Map::new();
    for (k, v) in parts.headers.iter() {
        let val = if redacted(k.as_str()) { "[redacted]".to_string() } else { trunc(&String::from_utf8_lossy(v.as_bytes())) };
        headers.insert(trunc(k.as_str()), Value::String(val));
    }
    let (mut mcp_method, mut tool) = (None, None);
    if parts.uri.path() == "/mcp" {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            mcp_method = v["method"].as_str().map(trunc);
            if mcp_method.as_deref() == Some("tools/call") {
                tool = v["params"]["name"].as_str().map(trunc);
            }
        }
    }
    let ua = parts.headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(trunc);
    let session = parts.headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).map(trunc);
    let tid = app.token_id_any(&parts.headers);
    let (method, path) = (trunc(parts.method.as_str()), trunc(&parts.uri.to_string()));
    let is_authz = parts.uri.path().starts_with("/authorize");
    let mut resp = next.run(Request::from_parts(parts, Body::from(bytes))).await;
    if is_authz {
        let h = resp.headers_mut();
        h.insert("x-frame-options", HeaderValue::from_static("DENY"));
        h.insert("content-security-policy", HeaderValue::from_static("frame-ancestors 'none'"));
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    let resp_session = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
    app.log_line(json!({
        "time": rfc3339(now_ms()), "method": method, "path": path, "status": resp.status().as_u16(),
        "mcp_method": mcp_method, "tool": tool, "session_id": session, "response_session_id": resp_session,
        "token_id": tid, "remote": remote.as_deref().map(trunc), "user_agent": ua, "headers": headers,
    }));
    resp
}

// ---------- OAuth ----------

fn base(app: &App) -> &str {
    app.cfg.public_url.trim_end_matches('/')
}
async fn prm(State(app): State<App>) -> Json<Value> {
    let b = base(&app);
    Json(json!({"resource": format!("{b}/mcp"), "authorization_servers": [b], "bearer_methods_supported": ["header"], "scopes_supported": ["bastion"]}))
}
async fn asm(State(app): State<App>) -> Json<Value> {
    let b = base(&app);
    let mut m = json!({
        "issuer": b,
        "authorization_endpoint": format!("{b}/authorize"),
        "token_endpoint": format!("{b}/token"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
        "scopes_supported": ["bastion"],
    });
    if app.cfg.allow_dcr {
        m["registration_endpoint"] = json!(format!("{b}/register"));
    }
    Json(m)
}

fn json_err(status: StatusCode, code: &str, desc: &str) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(json!({"error": code, "error_description": desc}))).into_response()
}

async fn register(State(app): State<App>, Json(body): Json<Value>) -> Response {
    let uris: Vec<String> = body["redirect_uris"].as_array().map(|a| a.iter().take(5).filter_map(|x| x.as_str().filter(|u| u.len() <= 512).map(String::from)).collect()).unwrap_or_default();
    if uris.is_empty() {
        return json_err(StatusCode::BAD_REQUEST, "invalid_redirect_uri", "redirect_uris required");
    }
    let public = body["token_endpoint_auth_method"].as_str() == Some("none");
    let id = format!("dyn-{}", rand_b64(9));
    let secret = (!public).then(|| format!("bcs_{}", rand_b64(32)));
    {
        let mut d = app.data.lock().unwrap();
        d.sweep(now_ms(), app.cfg.max_entries);
        d.clients.insert(id.clone(), Client { secret_hash: secret.as_ref().map(|s| hash(s)), redirect_uris: uris.clone(), fixed: false, created: now_ms() });
    }
    app.event("register", json!({"client_id": id})); // the body is deliberately not logged
    let mut out = json!({"client_id": id, "redirect_uris": uris, "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"],
        "token_endpoint_auth_method": if public {"none"} else {"client_secret_post"}, "client_name": body["client_name"]});
    if let Some(s) = secret {
        out["client_secret"] = json!(s);
    }
    (StatusCode::CREATED, [(header::CACHE_CONTROL, "no-store")], Json(out)).into_response()
}

#[derive(Deserialize)]
struct AuthQ {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    scope: Option<String>,
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn redirect_ok(c: &Client, uri: &str, public_redirect: Option<&str>) -> bool {
    if c.fixed && public_redirect.is_some() {
        Some(uri) == public_redirect
    } else if c.fixed {
        // Fallback when no public redirect is configured: the confidential fixed client (secret required).
        // The fixed client's callback is unknown until Grok shows it; accept https or loopback, and log it.
        uri.starts_with("https://") || uri.starts_with("http://127.0.0.1") || uri.starts_with("http://localhost")
    } else {
        c.redirect_uris.iter().any(|u| u == uri)
    }
}

async fn authorize(State(app): State<App>, Query(q): Query<AuthQ>) -> Response {
    let bad = |m: &str| (StatusCode::BAD_REQUEST, Html(format!("<h1>Bad request</h1><p>{}</p>", esc(m)))).into_response();
    let (Some(cid), Some(ru)) = (q.client_id.clone(), q.redirect_uri.clone()) else { return bad("client_id and redirect_uri required") };
    let mut d = app.data.lock().unwrap();
    match d.clients.get(&cid) {
        Some(c) if redirect_ok(c, &ru, app.cfg.public_redirect.as_deref()) => {}
        _ => return bad("unknown client or redirect_uri"),
    }
    // From here errors go back to the client.
    let back = |code: &str| {
        let mut u = url::Url::parse(&ru).unwrap();
        u.query_pairs_mut().append_pair("error", code);
        if let Some(s) = &q.state {
            u.query_pairs_mut().append_pair("state", s);
        }
        (StatusCode::FOUND, [(header::LOCATION, u.to_string())]).into_response()
    };
    if url::Url::parse(&ru).is_err() {
        return bad("bad redirect_uri");
    }
    if q.response_type.as_deref() != Some("code") {
        return back("unsupported_response_type");
    }
    let Some(ch) = q.code_challenge.clone().filter(|c| c.len() >= 43) else { return back("invalid_request") };
    if q.code_challenge_method.as_deref() != Some("S256") {
        return back("invalid_request");
    }
    let rid = rand_b64(18);
    let scope = q.scope.clone().unwrap_or_else(|| "bastion".into());
    d.sweep(now_ms(), app.cfg.max_entries);
    d.pending.insert(rid.clone(), AuthReq { client_id: cid.clone(), redirect_uri: ru.clone(), state: q.state.clone(), challenge: ch, scope: scope.clone(), created: now_ms() });
    drop(d);
    app.event("authorize_shown", json!({"client_id": cid, "redirect_uri": ru, "scope": scope}));
    Html(format!(
        "<!doctype html><meta name=viewport content='width=device-width'><h1>Bastion spike: connect a client?</h1>\
<p>Client: <b>{}</b><br>Redirect: <b>{}</b><br>Scope: <b>{}</b></p>\
<p>In the product this is a T4 approval on the phone. Here it only records the button.</p>\
<form method=post action=/authorize/decision><input type=hidden name=req value='{}'>\
<p>Owner PIN (printed on the server console): <input name=pin inputmode=numeric autocomplete=off required></p>\
<button name=decision value=approve>Approve</button> <button name=decision value=deny>Deny</button></form>",
        esc(&cid), esc(&ru), esc(&scope), esc(&rid)
    ))
    .into_response()
}

#[derive(Deserialize)]
struct Decision {
    req: String,
    decision: String,
    pin: String,
}
async fn decision(State(app): State<App>, Form(f): Form<Decision>) -> Response {
    let mut d = app.data.lock().unwrap();
    if d.pin_fails >= PIN_MAX_FAILS {
        return (StatusCode::FORBIDDEN, Html("<h1>Locked: too many wrong PINs. Restart the server.</h1>")).into_response();
    }
    if !ct_eq(f.pin.trim(), &app.pin) {
        d.pin_fails += 1;
        let fails = d.pin_fails;
        drop(d);
        app.event("pin_failed", json!({"fails": fails}));
        return (StatusCode::FORBIDDEN, Html("<h1>Wrong PIN</h1><p>Go back and try again.</p>")).into_response();
    }
    let Some(p) = d.pending.remove(&f.req) else {
        return (StatusCode::BAD_REQUEST, Html("<h1>Expired or unknown request</h1>")).into_response();
    };
    drop(d);
    let approved = f.decision == "approve";
    app.event("consent", json!({"client_id": p.client_id, "decision": if approved {"approve"} else {"deny"}}));
    let mut u = url::Url::parse(&p.redirect_uri).unwrap();
    if approved {
        let code = format!("bsc_{}", rand_b64(32));
        let mut d = app.data.lock().unwrap();
        d.sweep(now_ms(), app.cfg.max_entries);
        d.codes.insert(
            hash(&code),
            CodeRec { client_id: p.client_id, redirect_uri: p.redirect_uri.clone(), challenge: p.challenge, scope: p.scope, expires: now_ms() + 60_000 },
        );
        u.query_pairs_mut().append_pair("code", &code);
    } else {
        u.query_pairs_mut().append_pair("error", "access_denied");
    }
    if let Some(s) = &p.state {
        u.query_pairs_mut().append_pair("state", s);
    }
    (StatusCode::FOUND, [(header::LOCATION, u.to_string())]).into_response()
}

async fn token(State(app): State<App>, headers: HeaderMap, Form(f): Form<HashMap<String, String>>) -> Response {
    // Client authentication: Basic header or form fields.
    let (mut cid, mut csec) = (f.get("client_id").cloned(), f.get("client_secret").cloned());
    if let Some(b) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Basic ")) {
        if let Some(s) = base64::engine::general_purpose::STANDARD.decode(b.trim()).ok().and_then(|x| String::from_utf8(x).ok()) {
            if let Some((a, b)) = s.split_once(':') {
                let dec = |x: &str| x.to_string();
                cid = Some(dec(a));
                csec = Some(dec(b));
            }
        }
    }
    let Some(cid) = cid else { return json_err(StatusCode::UNAUTHORIZED, "invalid_client", "client_id required") };
    let mut d = app.data.lock().unwrap();
    match d.clients.get(&cid) {
        None => return json_err(StatusCode::UNAUTHORIZED, "invalid_client", "unknown client"),
        Some(c) => {
            // A public fixed client may omit the secret; a secret that is sent must still be right. This also
            // covers refresh: as for any public client, the rotated refresh token is then the only credential.
            let public_ok = c.fixed && app.cfg.public_redirect.is_some() && csec.is_none();
            if let (Some(h), false) = (&c.secret_hash, public_ok) {
                if csec.as_deref().map(hash).as_deref() != Some(h.as_str()) {
                    return json_err(StatusCode::UNAUTHORIZED, "invalid_client", "bad client credentials");
                }
            }
        }
    }
    let grant = f.get("grant_type").map(String::as_str).unwrap_or("");
    let mut used: Option<(String, String)> = None;
    let (tid, scope) = match grant {
        "authorization_code" => {
            let Some(code) = f.get("code") else { return json_err(StatusCode::BAD_REQUEST, "invalid_request", "code required") };
            let Some(rec) = d.codes.remove(&hash(code)) else {
                // A replayed code revokes whatever it produced.
                if let Some((tid, _)) = d.used_codes.get(&hash(code)).cloned() {
                    d.tokens.retain(|_, t| t.id != tid);
                    d.refresh.retain(|_, r| r.tid != tid);
                    drop(d);
                    app.event("code_replay", json!({"client_id": cid, "token_id": tid}));
                }
                return json_err(StatusCode::BAD_REQUEST, "invalid_grant", "unknown or used code");
            };
            let verifier = f.get("code_verifier").map(String::as_str).unwrap_or("");
            let ok = rec.client_id == cid
                && now_ms() < rec.expires
                && f.get("redirect_uri").map(|r| r == &rec.redirect_uri).unwrap_or(false)
                && !verifier.is_empty()
                && B64.encode(Sha256::digest(verifier.as_bytes())) == rec.challenge;
            if !ok {
                return json_err(StatusCode::BAD_REQUEST, "invalid_grant", "code verification failed");
            }
            let tid = format!("tid_{}", rand_b64(6));
            used = Some((hash(code), tid.clone()));
            (tid, rec.scope)
        }
        "refresh_token" => {
            let Some(r) = f.get("refresh_token") else { return json_err(StatusCode::BAD_REQUEST, "invalid_request", "refresh_token required") };
            match d.refresh.remove(&hash(r)) {
                Some(r) if r.cid == cid => (r.tid, "bastion".to_string()),
                _ => return json_err(StatusCode::BAD_REQUEST, "invalid_grant", "bad refresh token"),
            }
        }
        _ => return json_err(StatusCode::BAD_REQUEST, "unsupported_grant_type", "authorization_code or refresh_token"),
    };
    let access = format!("bst_{}", rand_b64(32));
    let refresh = format!("bsr_{}", rand_b64(32));
    d.sweep(now_ms(), app.cfg.max_entries);
    if let Some((h, t)) = used {
        d.used_codes.insert(h, (t, now_ms()));
    }
    d.tokens.insert(hash(&access), Tok { id: tid.clone(), client_id: cid.clone(), expires: now_ms() + app.cfg.access_ttl_ms, created: now_ms() });
    d.refresh.insert(hash(&refresh), RefRec { tid: tid.clone(), cid: cid.clone(), created: now_ms() });
    drop(d);
    app.event("token_issued", json!({"client_id": cid, "token_id": tid, "grant": grant}));
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"access_token": access, "token_type": "Bearer", "expires_in": app.cfg.access_ttl_ms / 1000, "refresh_token": refresh, "scope": scope})),
    )
        .into_response()
}

// ---------- MCP ----------

fn unauthorized(app: &App) -> Response {
    let v = format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"", base(app));
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, HeaderValue::from_str(&v).unwrap())], "unauthorized").into_response()
}
async fn mcp_get(State(app): State<App>, headers: HeaderMap) -> Response {
    if app.authenticate(&headers).is_none() {
        return unauthorized(&app);
    }
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}
async fn mcp_delete(State(app): State<App>, headers: HeaderMap) -> Response {
    if app.authenticate(&headers).is_none() {
        return unauthorized(&app);
    }
    StatusCode::OK.into_response()
}

const UI_URI: &str = "ui://bastion/pending/demo-1";

fn ui_html(app: &App) -> String {
    format!(
        "<!doctype html><body style='font-family:sans-serif'><h3>Pending approval demo-1</h3><p>State: waiting. Expires in 10 minutes.</p>\
<a href='{}/pending/demo-1' target=_blank><button>Open Bastion</button></a> \
<button onclick=\"parent.postMessage({{jsonrpc:'2.0',id:1,method:'tools/call',params:{{name:'hello',arguments:{{name:'card-press'}}}}}},'*')\">Press (test)</button>",
        base(app)
    )
}
async fn pending_page(axum::extract::Path(id): axum::extract::Path<String>) -> Html<String> {
    Html(format!("<h1>Bastion pending request {}</h1><p>Demo page: the approver would be here.</p>", esc(&id)))
}

fn tools() -> Value {
    json!([
        {"name": "hello", "description": "Greets a name.", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
        {"name": "whoami", "description": "Returns the token id, client id, MCP session id and request counts seen by the server.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "pending_request", "description": "Returns a demo pending approval as text and as an interactive card (ui:// resource).",
         "inputSchema": {"type": "object", "properties": {}},
         "_meta": {"ui": {"resourceUri": UI_URI}, "ui/resourceUri": UI_URI}},
        {"name": "wake_me", "description": "Schedules a POST to the routine webhook after the given seconds.", "inputSchema": {"type": "object", "properties": {"seconds": {"type": "integer"}}, "required": ["seconds"]}}
    ])
}

fn text(t: String) -> Value {
    json!({"content": [{"type": "text", "text": t}]})
}

async fn mcp(State(app): State<App>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Some(tok) = app.authenticate(&headers) else { return unauthorized(&app) };
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}})).into_response();
    };
    let sid_in = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
    let method = msg["method"].as_str().unwrap_or("").to_string();
    let id = msg.get("id").cloned();
    let mut new_session = None;
    let (sess_count, tok_count, session_id) = {
        let mut d = app.data.lock().unwrap();
        let n = now_ms();
        let (key, shown) = if method == "initialize" {
            d.sweep(n, app.cfg.max_entries);
            let s = format!("sess_{}", rand_b64(12));
            d.sessions.insert(s.clone(), Sess { count: 0, created: n, tid: tok.id.clone() });
            new_session = Some(s.clone());
            (s.clone(), s)
        } else if let Some(s) = &sid_in {
            // Only server-issued ids, and only for the token that opened them.
            match d.sessions.get(s) {
                Some(x) if x.tid == tok.id => (s.clone(), s.clone()),
                _ => return StatusCode::NOT_FOUND.into_response(),
            }
        } else {
            let k = format!("none:{}", tok.id);
            if !d.sessions.contains_key(&k) {
                d.sweep(n, app.cfg.max_entries);
                d.sessions.insert(k.clone(), Sess { count: 0, created: n, tid: tok.id.clone() });
            }
            (k, "none".to_string())
        };
        let sc = d.sessions.get_mut(&key).map(|x| { x.count += 1; x.count }).unwrap_or(0);
        if !d.token_counts.contains_key(&tok.id) {
            d.token_counts.insert(tok.id.clone(), (0, n));
        }
        let tc = d.token_counts.get_mut(&tok.id).unwrap();
        tc.0 += 1;
        (sc, tc.0, shown)
    };
    let Some(id) = id else { return StatusCode::ACCEPTED.into_response() }; // notification
    let ok = |r: Value| json!({"jsonrpc": "2.0", "id": id, "result": r});
    let result = match method.as_str() {
        "initialize" => {
            let want = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let v = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"].into_iter().find(|x| *x == want).unwrap_or("2025-06-18");
            ok(json!({"protocolVersion": v, "capabilities": {"tools": {}, "resources": {}}, "serverInfo": {"name": "bastion-spike", "version": "0.0.0"},
                "instructions": "Bastion spike S4. Tools: hello, whoami, pending_request, wake_me."}))
        }
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({"tools": tools()})),
        "resources/list" => ok(json!({"resources": [{"uri": UI_URI, "name": "pending-card", "mimeType": "text/html;profile=mcp-app"}]})),
        "resources/templates/list" => ok(json!({"resourceTemplates": []})),
        "prompts/list" => ok(json!({"prompts": []})),
        "resources/read" => {
            if msg["params"]["uri"].as_str() == Some(UI_URI) {
                ok(json!({"contents": [{"uri": UI_URI, "mimeType": "text/html;profile=mcp-app", "text": ui_html(&app)}]}))
            } else {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32002, "message": "not found"}})
            }
        }
        "tools/call" => {
            let args = &msg["params"]["arguments"];
            match msg["params"]["name"].as_str().unwrap_or("") {
                "hello" => ok(text(format!("Hello, {}!", args["name"].as_str().unwrap_or("stranger")))),
                "whoami" => ok(text(
                    json!({"token_id": tok.id, "client_id": tok.client_id, "mcp_session_id": session_id, "requests_in_session": sess_count, "requests_for_token": tok_count}).to_string(),
                )),
                "pending_request" => ok(json!({
                    "content": [
                        {"type": "text", "text": format!("Pending approval demo-1 (state: waiting, expires in 10 minutes). Open: {}/pending/demo-1", base(&app))},
                        {"type": "resource_link", "uri": format!("{}/pending/demo-1", base(&app)), "name": "Open Bastion"},
                        {"type": "resource", "resource": {"uri": UI_URI, "mimeType": "text/html;profile=mcp-app", "text": ui_html(&app)}}
                    ],
                    "structuredContent": {"request_id": "demo-1", "state": "waiting", "link": format!("{}/pending/demo-1", base(&app))},
                    "_meta": {"ui": {"resourceUri": UI_URI}}
                })),
                "wake_me" => {
                    let secs = args["seconds"].as_u64().unwrap_or(0).min(WAKE_MAX_SECS);
                    ok(text(wake(&app, secs, tok_count)))
                }
                other => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": format!("unknown tool {other}")}}),
            }
        }
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}),
    };
    let mut resp = Json(result).into_response();
    if let Some(s) = new_session {
        resp.headers_mut().insert("mcp-session-id", HeaderValue::from_str(&s).unwrap());
    }
    resp
}

fn wake(app: &App, secs: u64, count: u64) -> String {
    let secs = secs.min(WAKE_MAX_SECS);
    let path = app.cfg.state_dir.join("routine.toml");
    let Ok(s) = std::fs::read_to_string(&path) else { return "not scheduled: state/routine.toml is missing".into() };
    let Ok(t) = s.parse::<toml::Table>() else { return "not scheduled: routine.toml is not valid TOML".into() };
    let (Some(url), Some(key)) = (t.get("url").and_then(|v| v.as_str()).map(String::from), t.get("key").and_then(|v| v.as_str()).map(String::from)) else {
        return "not scheduled: routine.toml needs url and key".into();
    };
    {
        let mut d = app.data.lock().unwrap();
        if d.wake_pending {
            return "not scheduled: a wake is already pending".into();
        }
        d.wake_pending = true;
    }
    let app2 = app.clone();
    let scheduled = now_ms();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        // At least WAKE_MIN_GAP_MS between fires.
        let wait = {
            let d = app2.data.lock().unwrap();
            (d.last_wake_ms + WAKE_MIN_GAP_MS).saturating_sub(now_ms())
        };
        if wait > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }
        app2.data.lock().unwrap().last_wake_ms = now_ms();
        let body = json!({"event": "request.decided", "id": "demo-1", "count": count});
        let r = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().unwrap().post(&url).bearer_auth(&key).json(&body).send().await;
        let res = match r {
            Ok(r) => json!({"status": r.status().as_u16()}),
            Err(e) => json!({"error": trunc(&e.without_url().to_string())}),
        };
        app2.data.lock().unwrap().wake_pending = false;
        app2.event("wake_fired", json!({"scheduled_ms": scheduled, "fired_ms": now_ms(), "delay_s": secs, "result": res}));
    });
    app.event("wake_scheduled", json!({"delay_s": secs}));
    format!("scheduled: POST to the routine webhook in {secs} s (at most one pending, max {WAKE_MAX_SECS} s, fires at least {} s apart)", WAKE_MIN_GAP_MS / 1000)
}
