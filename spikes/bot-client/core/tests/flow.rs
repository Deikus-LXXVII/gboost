use axum::{extract::State, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::{get, post}, Json, Router};
use gboost_s9_core::{build, webhook_url_ok, App, Config};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex},
};

/// Fake routine receiver: captures every call; `script` holds (delay_ms, status) for the next calls
/// (status 302 redirects to /trap); default is 200 at once.
#[derive(Default)]
struct HookCtl {
    captured: Mutex<Vec<(String, Value)>>,
    script: Mutex<VecDeque<(u64, u16)>>,
    trap: AtomicUsize,
    addr: Mutex<Option<SocketAddr>>,
}

struct Env {
    agent: String,
    owner: String,
    token: String,
    dir: std::path::PathBuf,
    ctl: Arc<HookCtl>,
    app: Arc<App>,
    http: reqwest::Client,
}
impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const KEY: &str = "whk_test_key_do_not_leak";

async fn serve(r: Router) -> SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, r.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    a
}

async fn hook(State(c): State<Arc<HookCtl>>, h: HeaderMap, Json(b): Json<Value>) -> impl IntoResponse {
    let a = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    c.captured.lock().unwrap().push((a, b));
    let (delay, status) = c.script.lock().unwrap().pop_front().unwrap_or((0, 200));
    if delay > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    if status == 302 {
        let to = format!("http://{}/trap", c.addr.lock().unwrap().unwrap());
        return (StatusCode::FOUND, [("location", to)], "").into_response();
    }
    (StatusCode::from_u16(status).unwrap(), [("x-t", "1".to_string())], "ok").into_response()
}
async fn trap(State(c): State<Arc<HookCtl>>) -> &'static str {
    c.trap.fetch_add(1, Ordering::SeqCst);
    "trapped"
}

async fn env_with(tweak: impl FnOnce(&mut Config), routine: bool) -> Env {
    env_url(tweak, if routine { Some(None) } else { None }).await
}
/// routine: None = no routine.toml; Some(None) = the fake hook; Some(Some(url)) = that url.
async fn env_url(tweak: impl FnOnce(&mut Config), routine: Option<Option<String>>) -> Env {
    let dir = std::env::temp_dir().join(format!("s9test_{}", rand::random::<u64>()));
    let ctl: Arc<HookCtl> = Default::default();
    let wa = serve(Router::new().route("/hook", post(hook)).route("/trap", get(trap).post(trap)).with_state(ctl.clone())).await;
    *ctl.addr.lock().unwrap() = Some(wa);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(u) = routine {
        let url = u.unwrap_or_else(|| format!("http://{wa}/hook"));
        std::fs::write(dir.join("routine.toml"), format!("url = \"{url}\"\nkey = \"{KEY}\"\n")).unwrap();
    }
    let mut cfg = Config::new(dir.clone());
    cfg.check_cooldown_s = 0;
    cfg.max_checks_per_hour_global = 1000;
    tweak(&mut cfg);
    let (agent, owner, app) = build(cfg).unwrap();
    let aa = serve(agent).await;
    let oa = serve(owner).await;
    Env { agent: format!("http://{aa}"), owner: format!("http://{oa}"), token: app.console_token().to_string(), dir, ctl, app, http: reqwest::Client::new() }
}
async fn env() -> Env { env_with(|_| {}, true).await }

impl Env {
    async fn enrol(&self, id: &str, secret: &str) -> u16 {
        self.http.post(format!("{}/enroll", self.agent)).json(&json!({"client_id": id, "secret": secret})).send().await.unwrap().status().as_u16()
    }
    async fn owner(&self, path: &str) -> Value {
        let r = self.http.post(format!("{}{path}", self.owner)).header("x-console-token", &self.token).send().await.unwrap();
        assert!(r.status().is_success(), "owner call {path} {}", r.status());
        r.json().await.unwrap()
    }
    async fn owner_status(&self, method: &str, path: &str, token: Option<&str>) -> (u16, Value) {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut r = self.http.request(m, format!("{}{path}", self.owner));
        if let Some(t) = token { r = r.header("x-console-token", t); }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(json!({})))
    }
    async fn clients(&self) -> Vec<Value> {
        let (c, v) = self.owner_status("GET", "/local/clients", Some(&self.token)).await;
        assert_eq!(c, 200);
        v["clients"].as_array().unwrap().clone()
    }
    fn log(&self, f: &str) -> String { std::fs::read_to_string(self.dir.join(f)).unwrap_or_default() }
    async fn approve(&self, id: &str) { self.owner(&format!("/local/clients/{id}/approve")).await; }
    async fn post(&self, path: &str, auth: &str, session: Option<&str>, body: Value) -> (u16, Value) {
        let mut r = self.http.post(format!("{}{path}", self.agent)).header("x-gboost-client", auth).json(&body);
        if let Some(s) = session { r = r.bearer_auth(s); }
        let r = r.send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(json!({})))
    }
    async fn start(&self, auth: &str) -> (u16, Value) { self.post("/session/start", auth, None, json!({})).await }
    fn last_code(&self) -> String {
        self.ctl.captured.lock().unwrap().last().unwrap().1["code"].as_str().unwrap().to_string()
    }
    /// enrol + approve + start + finish; returns session token
    async fn session_for(&self, id: &str, secret: &str) -> String {
        assert_eq!(self.enrol(id, secret).await, 200);
        self.approve(id).await;
        let auth = format!("{id}:{secret}");
        assert_eq!(self.start(&auth).await.0, 200);
        let (c, r) = self.post("/session/finish", &auth, None, json!({"code": self.last_code()})).await;
        assert_eq!(c, 200, "{r}");
        r["session"].as_str().unwrap().to_string()
    }
}
const A: (&str, &str) = ("gbc_clientAAAAAAAA", "secretsecretsecretsecretsecretAAAA");
const B: (&str, &str) = ("gbc_clientBBBBBBBB", "secretsecretsecretsecretsecretBBBB");
fn auth(c: (&str, &str)) -> String { format!("{}:{}", c.0, c.1) }

#[tokio::test]
async fn pending_until_approved() {
    let e = env().await;
    assert_eq!(e.enrol(A.0, A.1).await, 200);
    assert_eq!(e.enrol(A.0, "otherotherotherotherotherotherother").await, 409, "re-enrol refused");
    assert_eq!(e.start(&auth(A)).await.0, 403);
    assert!(e.ctl.captured.lock().unwrap().is_empty());
    e.approve(A.0).await;
    assert_eq!(e.start(&auth(A)).await.0, 200);
}

#[tokio::test]
async fn happy_path() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    let (c, r) = e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello", "args": {"name": "Bot"}})).await;
    assert_eq!((c, r["text"].as_str().unwrap()), (200, "Hello, Bot!"));
    let (_, r) = e.post("/rpc", &auth(A), Some(&s), json!({"tool": "whoami"})).await;
    let w: Value = serde_json::from_str(r["text"].as_str().unwrap()).unwrap();
    assert_eq!(w["client_id"], A.0);
    assert_eq!(w["calls_used"], 2);
    assert_eq!(w["calls_budget"], 500);
    assert!(w["peer"].as_str().unwrap().starts_with("127.0.0.1:"));
    let (c, r) = e.post("/rpc", &auth(A), Some(&s), json!({"tool": "pending_request"})).await;
    assert_eq!(c, 200);
    assert!(r["text"].as_str().unwrap().contains("req_demo"));
    // webhook: right bearer key, only the allowed fields, code never in the start response
    let h = e.ctl.captured.lock().unwrap();
    assert_eq!(h[0].0, format!("Bearer {KEY}"));
    let mut keys: Vec<_> = h[0].1.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["check_id", "client_fingerprint", "code", "kind", "source"]);
    assert_eq!(h[0].1["source"], "gboost");
    assert_eq!(h[0].1["kind"], "session_check");
}

#[tokio::test]
async fn start_response_has_no_code() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    let (c, r) = e.start(&auth(A)).await;
    assert_eq!(c, 200);
    assert_eq!(r["pending"], true);
    assert_eq!(r["expires_in"], 300);
    assert!(r.get("code").is_none() && r["check_id"].as_str().unwrap().len() >= 6);
    assert_eq!(r["check_id"], e.ctl.captured.lock().unwrap().last().unwrap().1["check_id"], "check_id matches the webhook");
    assert_eq!(r.as_object().unwrap().len(), 3);
}

#[tokio::test]
async fn code_single_use() {
    let e = env().await;
    e.session_for(A.0, A.1).await;
    let code = e.last_code();
    assert_eq!(e.post("/session/finish", &auth(A), None, json!({"code": code})).await.0, 401);
}

#[tokio::test]
async fn wrong_code_refused() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    e.start(&auth(A)).await;
    assert_eq!(e.post("/session/finish", &auth(A), None, json!({"code": "nope"})).await.0, 401);
}

#[tokio::test]
async fn expired_code_refused() {
    let e = env_with(|c| c.code_ttl_s = 0, true).await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    assert_eq!(e.start(&auth(A)).await.0, 200);
    assert_eq!(e.post("/session/finish", &auth(A), None, json!({"code": e.last_code()})).await.0, 401);
}

#[tokio::test]
async fn wrong_secret_401() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    assert_eq!(e.start(&format!("{}:wrongwrongwrong", A.0)).await.0, 401);
    assert_eq!(e.start("gbc_unknown:x").await.0, 401);
    assert!(e.ctl.captured.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_bound_to_client() {
    let e = env().await;
    let sa = e.session_for(A.0, A.1).await;
    e.enrol(B.0, B.1).await;
    e.approve(B.0).await;
    let (c, _) = e.post("/rpc", &auth(B), Some(&sa), json!({"tool": "hello"})).await;
    assert_eq!(c, 401);
}

#[tokio::test]
async fn rpc_needs_session() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    assert_eq!(e.post("/rpc", &auth(A), None, json!({"tool": "hello"})).await.0, 401);
    assert_eq!(e.post("/rpc", &auth(A), Some("gbs_fake"), json!({"tool": "hello"})).await.0, 401);
}

#[tokio::test]
async fn tailscale_headers_and_source_ip_not_trusted() {
    let e = env().await;
    for path in ["/rpc", "/session/start", "/session/finish"] {
        let r = e.http.post(format!("{}{path}", e.agent))
            .header("tailscale-user-login", "owner@example.com").header("tailscale-user-name", "Owner")
            .header("x-forwarded-for", "127.0.0.1").json(&json!({"tool": "hello", "code": "x"})).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 401, "{path}");
    }
}

#[tokio::test]
async fn revoke_kills_sessions() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 200);
    e.owner(&format!("/local/clients/{}/revoke", A.0)).await;
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 401);
    assert_eq!(e.start(&auth(A)).await.0, 401);
}

#[tokio::test]
async fn revoke_all_kills_sessions() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    e.owner("/local/sessions/revoke-all").await;
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 401);
    assert_eq!(e.start(&auth(A)).await.0, 200, "client itself stays approved");
}

#[tokio::test]
async fn start_rate_limit() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    for _ in 0..6 { assert_eq!(e.start(&auth(A)).await.0, 200); }
    assert_eq!(e.start(&auth(A)).await.0, 429);
    assert_eq!(e.ctl.captured.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn call_budget() {
    let e = env_with(|c| c.session_budget = 2, true).await;
    let s = e.session_for(A.0, A.1).await;
    for _ in 0..2 { assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 200); }
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 429);
}

#[tokio::test]
async fn missing_routine_is_503() {
    let e = env_with(|_| {}, false).await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    let (c, r) = e.start(&auth(A)).await;
    assert_eq!(c, 503);
    assert!(r["error"].as_str().unwrap().contains("routine.toml"));
}

#[tokio::test]
async fn pending_enrol_cap() {
    let e = env_with(|c| c.enrol_per_ip = 100, true).await;
    for i in 0..5 { assert_eq!(e.enrol(&format!("gbc_cap{i}xxxxxxxx"), "secretsecretsecretsecretsecretCAPS").await, 200); }
    assert_eq!(e.enrol("gbc_cap9xxxxxxxx", "secretsecretsecretsecretsecretCAPS").await, 429);
}

#[tokio::test]
async fn listener_separation_and_console_auth() {
    let e = env().await;
    // owner routes absent on agent listener
    for p in ["/local/clients", "/local/sessions/revoke-all"] {
        let r = e.http.get(format!("{}{p}", e.agent)).header("x-console-token", &e.token).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 404, "{p}");
        let r = e.http.post(format!("{}{p}", e.agent)).header("x-console-token", &e.token).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 404, "{p}");
    }
    // agent routes absent on owner listener
    for p in ["/enroll", "/session/start", "/rpc", "/client/gboost_client.py"] {
        let r = e.http.post(format!("{}{p}", e.owner)).header("x-console-token", &e.token).json(&json!({})).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 404, "{p}");
    }
    // console token required
    let r = e.http.get(format!("{}/local/clients", e.owner)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = e.http.get(format!("{}/local/clients", e.owner)).header("x-console-token", "wrong").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = e.http.get(format!("{}/local/clients", e.owner)).header("x-console-token", &e.token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    // the client file is served on the agent listener
    let r = e.http.get(format!("{}/client/gboost_client.py", e.agent)).send().await.unwrap();
    assert!(r.text().await.unwrap().contains("gboost-client"));
}

#[tokio::test]
async fn state_files_private_and_no_secrets_in_log() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await;
    e.post("/rpc", &auth(A), Some("gbs_bogus_token_value"), json!({"tool": "hello"})).await;
    e.start(&format!("{}:leakcheck_wrong_secret", A.0)).await;
    let code = e.last_code();
    for f in ["clients.json", "console.token", "requests.log", "security.log"] {
        assert_owner_only(&e.dir.join(f));
    }
    let rlog = std::fs::read_to_string(e.dir.join("requests.log")).unwrap();
    let slog = std::fs::read_to_string(e.dir.join("security.log")).unwrap();
    let log = format!("{rlog}{slog}");
    let clients = std::fs::read_to_string(e.dir.join("clients.json")).unwrap();
    for secret in [A.1, KEY, s.as_str(), code.as_str(), e.token.as_str(), "gbs_bogus_token_value", "leakcheck_wrong_secret"] {
        assert!(!log.contains(secret), "secret leaked into a log: {}", &secret[..4]);
        assert!(!clients.contains(secret), "secret in clients.json");
    }
    assert!(!log.contains(&s[4..]) && !log.contains(&s), "session token in log");
    assert!(slog.contains("session_created") && slog.contains("\"enrol\""));
    assert!(!rlog.contains("session_created"), "security events stay out of requests.log");
    let _ = &e.app;
}

const SECRET: &str = "secretsecretsecretsecretsecretXXXX";

#[tokio::test]
async fn tailscale_headers_change_nothing_with_valid_creds() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    let who = |extra: bool| {
        let (e, s) = (&e, s.clone());
        async move {
            let mut r = e.http.post(format!("{}/rpc", e.agent)).header("x-gboost-client", auth(A)).bearer_auth(s).json(&json!({"tool": "whoami"}));
            if extra {
                r = r.header("tailscale-user-login", "owner@example.com").header("tailscale-user-name", "Owner")
                    .header("tailscale-user-profile-pic", "http://x/y.png").header("x-forwarded-for", "100.100.100.100").header("x-real-ip", "100.100.100.100");
            }
            let r = r.send().await.unwrap();
            assert_eq!(r.status().as_u16(), 200);
            let v: Value = r.json().await.unwrap();
            serde_json::from_str::<Value>(v["text"].as_str().unwrap()).unwrap()
        }
    };
    let (plain, with) = (who(false).await, who(true).await);
    assert_eq!(plain["client_id"], A.0);
    assert_eq!(with["client_id"], A.0, "identity comes from the client record");
    assert_eq!(plain["fingerprint"], with["fingerprint"]);
    assert!(!with.to_string().contains("owner@example.com") && !with.to_string().contains("100.100.100.100"));
    assert!(with["peer"].as_str().unwrap().starts_with("127.0.0.1:"), "peer is the socket address");
}

#[tokio::test]
async fn cross_client_code_refused() {
    let e = env().await;
    for c in [A, B] { e.enrol(c.0, c.1).await; e.approve(c.0).await; }
    // B has no pending check at all
    assert_eq!(e.start(&auth(A)).await.0, 200);
    let code_a = e.last_code();
    assert_eq!(e.post("/session/finish", &auth(B), None, json!({"code": code_a})).await.0, 401);
    // B has its own pending check; A's code still does not work for B
    assert_eq!(e.start(&auth(B)).await.0, 200);
    let code_b = e.last_code();
    assert_eq!(e.post("/session/finish", &auth(B), None, json!({"code": code_a})).await.0, 401);
    assert_eq!(e.post("/session/finish", &auth(B), None, json!({"code": code_b})).await.0, 200);
    // A's code was untouched by B's attempts
    assert_eq!(e.post("/session/finish", &auth(A), None, json!({"code": code_a})).await.0, 200);
}

#[tokio::test]
async fn webhook_failure_is_502_and_removes_only_its_own_code() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    // plain failure: 502 and the code is gone
    e.ctl.script.lock().unwrap().push_back((0, 500));
    assert_eq!(e.start(&auth(A)).await.0, 502);
    let dead = e.last_code();
    let (c, r) = e.post("/session/finish", &auth(A), None, json!({"code": dead})).await;
    assert_eq!((c, r["error"].as_str().unwrap()), (401, "no pending session check"));
    assert!(e.log("security.log").contains("webhook_fail"));
    // slow failing start, then a newer successful start: the newer code survives
    e.ctl.script.lock().unwrap().push_back((500, 500));
    let (e1, a1) = (e.http.clone(), format!("{}/session/start", e.agent));
    let au = auth(A);
    let slow = tokio::spawn(async move { e1.post(a1).header("x-gboost-client", au).json(&json!({})).send().await.unwrap().status().as_u16() });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(e.start(&auth(A)).await.0, 200);
    let newer = e.last_code();
    assert_eq!(slow.await.unwrap(), 502);
    assert_eq!(e.post("/session/finish", &auth(A), None, json!({"code": newer})).await.0, 200, "newer code not removed");
}

#[tokio::test]
async fn webhook_redirect_not_followed() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    e.ctl.script.lock().unwrap().push_back((0, 302));
    assert_eq!(e.start(&auth(A)).await.0, 502);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(e.ctl.trap.load(Ordering::SeqCst), 0, "the redirect target was never contacted");
    assert_eq!(e.ctl.captured.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn webhook_url_rules() {
    for ok in ["https://example.com/hook", "http://127.0.0.1:9/h", "http://localhost/h", "http://[::1]:1/h"] {
        assert!(webhook_url_ok(ok), "{ok}");
    }
    for bad in ["http://example.com/hook", "http://10.0.0.5/h", "ftp://127.0.0.1/", "file:///etc/passwd", "https://u:p@example.com/", "nonsense"] {
        assert!(!webhook_url_ok(bad), "{bad}");
    }
    let e = env_url(|_| {}, Some(Some("http://example.com/hook".into()))).await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    let (c, r) = e.start(&auth(A)).await;
    assert_eq!(c, 503);
    assert!(r["error"].as_str().unwrap().contains("https"));
    assert!(e.ctl.captured.lock().unwrap().is_empty());
}

#[tokio::test]
async fn revoked_client_gets_generic_401_everywhere() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    assert_eq!(e.start(&auth(A)).await.0, 200); // a pending check exists
    let code = e.last_code();
    e.owner(&format!("/local/clients/{}/revoke", A.0)).await;
    let (c, r) = e.post("/session/finish", &auth(A), None, json!({"code": code})).await;
    assert_eq!(c, 401);
    let (c2, wrong) = e.post("/session/finish", &format!("{}:wrongwrongwrongwrongwrongwrongwrong", A.0), None, json!({"code": "x"})).await;
    assert_eq!(c2, 401);
    assert_eq!(r, wrong, "revoked looks the same as a wrong secret");
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.1, wrong);
    assert!(e.log("security.log").contains("auth_revoked"));
}

#[tokio::test]
async fn approve_on_revoked_is_409() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    e.owner(&format!("/local/clients/{}/revoke", A.0)).await;
    let (c, _) = e.owner_status("POST", &format!("/local/clients/{}/approve", A.0), Some(&e.token)).await;
    assert_eq!(c, 409);
    assert_eq!(e.clients().await[0]["state"], "revoked");
    assert_eq!(e.start(&auth(A)).await.0, 401);
    let (c, _) = e.owner_status("POST", "/local/clients/gbc_nobody0000/approve", Some(&e.token)).await;
    assert_eq!(c, 404);
}

#[tokio::test]
async fn enrol_validation_is_400() {
    let e = env().await;
    let long_id = format!("gbc_{}", "a".repeat(70));
    let long_secret = "s".repeat(300);
    let cases: Vec<(&str, &str)> = vec![
        ("nogbc_prefix_abcdef", SECRET), ("gbc_short", SECRET), (&long_id, SECRET), ("gbc_bad id/with*chars", SECRET),
        ("gbc_okokokokokok", "tooshort"), ("gbc_okokokokokok", &long_secret),
    ];
    for (id, sec) in cases {
        assert_eq!(e.enrol(id, sec).await, 400, "{id} / {}", sec.len());
    }
    assert!(e.clients().await.is_empty());
    assert_eq!(e.enrol("gbc_okokokokokok", SECRET).await, 200);
}

#[tokio::test]
async fn pending_enrol_expires_and_is_purged() {
    let e = env_with(|c| c.pending_ttl_s = 0, true).await;
    assert_eq!(e.enrol(A.0, A.1).await, 200);
    assert!(e.clients().await.is_empty(), "expired pending enrolment purged");
    assert!(!e.log("clients.json").contains(A.0), "purged from clients.json too");
    assert_eq!(e.start(&auth(A)).await.0, 401);
    assert_eq!(e.enrol(A.0, A.1).await, 200, "the id is free again");
    // default ttl keeps it
    let e = env().await;
    e.enrol(A.0, A.1).await;
    assert_eq!(e.clients().await.len(), 1);
}

#[tokio::test]
async fn revoked_records_are_purged() {
    let e = env_with(|c| c.revoked_purge_s = 0, true).await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    e.owner(&format!("/local/clients/{}/revoke", A.0)).await;
    assert!(e.clients().await.is_empty());
    assert!(!e.log("clients.json").contains(A.0));
    // a recent revoked record is kept
    let e = env().await;
    e.enrol(A.0, A.1).await;
    e.owner(&format!("/local/clients/{}/revoke", A.0)).await;
    assert_eq!(e.clients().await[0]["state"], "revoked");
}

#[tokio::test]
async fn enrol_limits_per_ip_and_total() {
    let e = env().await;
    for i in 0..3 { assert_eq!(e.enrol(&format!("gbc_ip{i}xxxxxxxxx"), SECRET).await, 200); }
    assert_eq!(e.enrol("gbc_ip9xxxxxxxxx", SECRET).await, 429, "4th from the same address");
    let e = env_with(|c| { c.enrol_per_ip = 100; c.max_clients = 2; }, true).await;
    for i in 0..2 { assert_eq!(e.enrol(&format!("gbc_tot{i}xxxxxxxx"), SECRET).await, 200); }
    assert_eq!(e.enrol("gbc_tot9xxxxxxxx", SECRET).await, 429, "total client cap");
}

#[tokio::test]
async fn console_token_required_on_every_owner_route() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    let id = A.0;
    let routes = [
        ("GET", "/local/clients".to_string()), ("POST", format!("/local/clients/{id}/approve")),
        ("POST", format!("/local/clients/{id}/revoke")), ("POST", "/local/sessions/revoke-all".to_string()),
        ("DELETE", "/local/clients".to_string()), ("GET", "/local/anything".to_string()),
    ];
    for (m, p) in &routes {
        for tok in [None, Some("wrong"), Some("")] {
            let (c, _) = e.owner_status(m, p, tok).await;
            assert!(c == 401, "{m} {p} with {tok:?} gave {c}");
        }
    }
    assert_eq!(e.clients().await[0]["state"], "pending", "nothing changed without the token");
    for (m, p) in &routes[..4] {
        let (c, _) = e.owner_status(m, p, Some(&e.token)).await;
        assert_eq!(c, 200, "{m} {p} with the token");
    }
}

#[tokio::test]
async fn new_session_replaces_older_ones() {
    let e = env().await;
    let s1 = e.session_for(A.0, A.1).await;
    assert_eq!(e.start(&auth(A)).await.0, 200);
    let (c, r) = e.post("/session/finish", &auth(A), None, json!({"code": e.last_code()})).await;
    assert_eq!(c, 200);
    let s2 = r["session"].as_str().unwrap().to_string();
    assert_eq!(e.post("/rpc", &auth(A), Some(&s1), json!({"tool": "hello"})).await.0, 401, "older session revoked");
    assert_eq!(e.post("/rpc", &auth(A), Some(&s2), json!({"tool": "hello"})).await.0, 200);
}

#[tokio::test]
async fn expired_sessions_are_swept() {
    let e = env_with(|c| c.session_ttl_s = 0, true).await;
    let s = e.session_for(A.0, A.1).await;
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": "hello"})).await.0, 401);
}

#[tokio::test]
async fn unauthenticated_noise_is_not_logged_per_request() {
    let e = env().await;
    e.enrol(A.0, A.1).await;
    for i in 0..60 {
        let _ = e.http.get(format!("{}/nope{i}", e.agent)).send().await.unwrap();
        e.post("/rpc", "gbc_unknown000:x", None, json!({"tool": "hello"})).await;
        e.post("/enroll", "", None, json!({"client_id": "bad", "secret": "x"})).await;
        e.start("garbage").await;
    }
    for _ in 0..20 { e.start(&format!("{}:wrongwrongwrong", A.0)).await; } // known client, wrong secret
    let req = e.log("requests.log");
    for st in ["400", "401", "404"] {
        assert!(!req.contains(&format!("\"status\":{st}")), "no per-request lines for noise: {req}");
    }
    assert!(!req.contains("noise_summary"), "no aggregate yet inside the minute");
    assert!(req.lines().count() <= 1, "only the legitimate enrol request line: {req}");
    let sec = e.log("security.log");
    assert_eq!(sec.matches("auth_fail").count(), 1, "known-client failures are rate limited in the security log");
    assert!(!sec.contains("nope") && !sec.contains("gbc_unknown000"));
}

#[tokio::test]
async fn noise_aggregate_line_once_per_window() {
    let e = env_with(|c| c.noise_flush_s = 1, true).await;
    for i in 0..50 { let _ = e.http.get(format!("{}/nope{i}", e.agent)).send().await.unwrap(); }
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let _ = e.http.get(format!("{}/nope", e.agent)).send().await.unwrap();
    let req = e.log("requests.log");
    let sums: Vec<&str> = req.lines().filter(|l| l.contains("noise_summary")).collect();
    assert!(!sums.is_empty() && sums.len() <= 2, "{req}");
    assert!(req.lines().count() == sums.len(), "only aggregate lines: {req}");
    let total: u64 = sums.iter().map(|l| serde_json::from_str::<Value>(l).unwrap()["requests"].as_u64().unwrap()).sum();
    assert_eq!(total, 51);
}

#[tokio::test]
async fn attacker_supplied_tool_name_is_capped_in_log() {
    let e = env().await;
    let s = e.session_for(A.0, A.1).await;
    let tool = "x".repeat(500);
    assert_eq!(e.post("/rpc", &auth(A), Some(&s), json!({"tool": tool})).await.0, 404);
    let req = e.log("requests.log");
    assert!(req.contains(&"x".repeat(64)) && !req.contains(&"x".repeat(65)));
}

#[tokio::test]
async fn slow_request_times_out() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let e = env_with(|c| c.request_timeout_s = 1, true).await;
    let addr = e.agent.trim_start_matches("http://").to_string();
    let mut sock = tokio::net::TcpStream::connect(&addr).await.unwrap();
    sock.write_all(b"POST /enroll HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{").await.unwrap();
    let mut buf = vec![0u8; 512];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), sock.read(&mut buf)).await.expect("server answered in time").unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 408"), "{}", String::from_utf8_lossy(&buf[..n]));
}

// ---- Remote MCP connector ----

struct Mcp<'a> { e: &'a Env, key: String }
impl<'a> Mcp<'a> {
    async fn raw(&self, key: Option<&str>, extra: &[(&str, &str)], body: Value) -> reqwest::Response {
        let mut r = self.e.http.post(format!("{}/mcp", self.e.agent)).json(&body);
        if let Some(k) = key { r = r.header("x-gboost-client", k); }
        for (k, v) in extra { r = r.header(*k, *v); }
        r.send().await.unwrap()
    }
    async fn rpc(&self, method: &str, params: Value) -> Value {
        let r = self.raw(Some(&self.key), &[], json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).await;
        assert_eq!(r.status(), 200);
        r.json().await.unwrap()
    }
    /// (isError, text)
    async fn call(&self, tool: &str, args: Value) -> (bool, String) {
        let v = self.rpc("tools/call", json!({"name": tool, "arguments": args})).await;
        let r = &v["result"];
        (r["isError"].as_bool().unwrap(), r["content"][0]["text"].as_str().unwrap().to_string())
    }
}
async fn new_key(e: &Env) -> (String, Value) {
    let r = e.owner("/local/clients/new").await;
    (r["header_value"].as_str().unwrap().to_string(), r)
}
const PENDING_MSG: &str = "GBOOST sent a session check to your routine webhook. When the routine has called gboost_deliver_code, call this tool again.";
fn hooks(e: &Env) -> usize { e.ctl.captured.lock().unwrap().len() }

#[tokio::test]
async fn owner_creates_client_both_modes() {
    let e = env().await;
    let (key, r) = new_key(&e).await;
    assert_eq!(r["header_name"], "x-gboost-client");
    assert!(key.starts_with("gbc_") && key.contains(':'));
    assert_eq!(r["fingerprint"].as_str().unwrap().len(), 16);
    let cl = e.clients().await;
    assert_eq!(cl[0]["state"], "approved");
    // to_file: key only in the file, 0600
    let r = e.owner("/local/clients/new?to_file=1").await;
    assert!(r.get("header_value").is_none() && r["client_id"].as_str().is_some() && r["fingerprint"].as_str().is_some());
    let path = r["file"].as_str().unwrap();
    let txt = std::fs::read_to_string(path).unwrap();
    assert!(txt.trim().starts_with(r["client_id"].as_str().unwrap()));
    let secret = txt.trim().split_once(':').unwrap().1.to_string();
    assert!(!r.to_string().contains(&secret));
    assert_owner_only(std::path::Path::new(path));
    // console token required
    let (c, _) = e.owner_status("POST", "/local/clients/new", None).await;
    assert_eq!(c, 401);
    // the key works on the agent listener
    let m = Mcp { e: &e, key: txt.trim().to_string() };
    assert_eq!(m.rpc("ping", json!({})).await["result"], json!({}));
    // secret never in logs
    assert!(!e.log("security.log").contains(&secret) && !e.log("requests.log").contains(&secret));
}

#[tokio::test]
async fn mcp_auth_is_generic_401_without_www_authenticate() {
    let e = env().await;
    let (key, _) = new_key(&e).await;
    let m = Mcp { e: &e, key: key.clone() };
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let bad = format!("{}:wrongwrongwrongwrongwrongwrongwrongwrong", key.split(':').next().unwrap());
    for k in [None, Some(bad.as_str()), Some("garbage"), Some("gbc_nope:nope")] {
        let r = m.raw(k, &[], body.clone()).await;
        assert_eq!(r.status(), 401);
        assert!(r.headers().get("www-authenticate").is_none());
        let v: Value = r.json().await.unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["error"]["message"], "unauthorized");
    }
    let r = e.http.get(format!("{}/mcp", e.agent)).send().await.unwrap();
    assert_eq!(r.status(), 405);
}

#[tokio::test]
async fn mcp_initialize_and_tools_list_need_only_key() {
    let e = env().await;
    let (key, _) = new_key(&e).await;
    let m = Mcp { e: &e, key };
    let r = m.raw(Some(&m.key), &[], json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).await;
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("mcp-session-id").is_some());
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["id"], 7);
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    let r = m.raw(Some(&m.key), &[], json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    assert_eq!(r.status(), 202);
    let v = m.rpc("tools/list", json!({})).await;
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["gboost_hello", "gboost_whoami", "gboost_pending_request", "gboost_deliver_code", "gboost_status"]);
    assert_eq!(hooks(&e), 0, "no webhook for initialize or tools/list");
}

#[tokio::test]
async fn mcp_full_flow() {
    let e = env().await;
    let (key, _) = new_key(&e).await;
    let m = Mcp { e: &e, key };
    // no session: check sent, tool error with the message
    assert_eq!(m.call("gboost_hello", json!({})).await, (true, PENDING_MSG.to_string()));
    assert_eq!(hooks(&e), 1);
    {
        let h = e.ctl.captured.lock().unwrap();
        assert_eq!(h[0].0, format!("Bearer {KEY}"));
        let mut ks: Vec<_> = h[0].1.as_object().unwrap().keys().cloned().collect();
        ks.sort();
        assert_eq!(ks, ["check_id", "client_fingerprint", "code", "kind", "source"]);
    }
    // pending: no second webhook, from any gated tool
    assert_eq!(m.call("gboost_whoami", json!({})).await, (true, PENDING_MSG.to_string()));
    assert_eq!(hooks(&e), 1);
    let st: Value = serde_json::from_str(&m.call("gboost_status", json!({})).await.1).unwrap();
    assert_eq!((st["session_active"].clone(), st["check_pending"].clone()), (json!(false), json!(true)));
    // wrong code
    let (err_, txt) = m.call("gboost_deliver_code", json!({"code": "nope"})).await;
    assert!(err_ && !txt.contains("open"));
    assert_eq!(m.call("gboost_hello", json!({})).await.0, true, "still no session");
    // right code
    let code = e.last_code();
    assert_eq!(m.call("gboost_deliver_code", json!({"code": code, "check_id": "x"})).await, (false, "Session open".to_string()));
    // single use
    assert_eq!(m.call("gboost_deliver_code", json!({"code": code})).await.0, true);
    assert_eq!(m.call("gboost_hello", json!({"name": "Bot"})).await, (false, "Hello, Bot!".to_string()));
    let (_, w) = m.call("gboost_whoami", json!({})).await;
    let w: Value = serde_json::from_str(&w).unwrap();
    assert_eq!(w["calls_used"], 2);
    assert_eq!(w["calls_budget"], 500);
    assert!(w["seen"]["peer"].as_str().unwrap().starts_with("127.0.0.1:"));
    assert!(w.get("peer").is_none() && w.get("mcp_session_id").is_none());
    assert!(w["claimed_by_caller"].get("mcp_session_id").is_some());
    assert!(w["expires_ms"].as_u64().unwrap() > w["created_ms"].as_u64().unwrap());
    let (_, p) = m.call("gboost_pending_request", json!({})).await;
    assert!(p.contains("req_demo"));
    let st: Value = serde_json::from_str(&m.call("gboost_status", json!({})).await.1).unwrap();
    assert_eq!(st["session_active"], true);
    assert_eq!(hooks(&e), 1);
}

#[tokio::test]
async fn mcp_other_clients_code_refused_and_sessions_separate() {
    let e = env().await;
    let a = Mcp { e: &e, key: new_key(&e).await.0 };
    let b = Mcp { e: &e, key: new_key(&e).await.0 };
    a.call("gboost_hello", json!({})).await;
    let code_a = e.last_code();
    assert_eq!(b.call("gboost_deliver_code", json!({"code": code_a})).await.0, true);
    assert_eq!(b.call("gboost_hello", json!({})).await.1, PENDING_MSG, "b gets its own check");
    assert_eq!(hooks(&e), 2);
    // a's code still works for a (b's failed try did not consume it)
    assert_eq!(a.call("gboost_deliver_code", json!({"code": code_a})).await.1, "Session open");
    assert_eq!(b.call("gboost_hello", json!({})).await.0, true, "b has no session");
}

#[tokio::test]
async fn mcp_revoke_kills_session() {
    let e = env().await;
    let (key, r) = new_key(&e).await;
    let m = Mcp { e: &e, key };
    m.call("gboost_hello", json!({})).await;
    m.call("gboost_deliver_code", json!({"code": e.last_code()})).await;
    assert_eq!(m.call("gboost_hello", json!({})).await.0, false);
    e.owner(&format!("/local/clients/{}/revoke", r["client_id"].as_str().unwrap())).await;
    let resp = m.raw(Some(&m.key), &[], json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn mcp_revoke_all_kills_session() {
    let e = env().await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    m.call("gboost_hello", json!({})).await;
    m.call("gboost_deliver_code", json!({"code": e.last_code()})).await;
    assert_eq!(m.call("gboost_hello", json!({})).await.0, false);
    e.owner("/local/sessions/revoke-all").await;
    assert_eq!(m.call("gboost_hello", json!({})).await.1, PENDING_MSG);
}

#[tokio::test]
async fn mcp_forwarded_headers_ignored() {
    let e = env().await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    let hdrs = [("x-forwarded-for", "100.64.1.2"), ("tailscale-user-login", "owner@example.com"), ("tailscale-user-name", "Owner")];
    // headers alone do not authenticate
    let r = m.raw(None, &hdrs, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    assert_eq!(r.status(), 401);
    // with the key they are shown as data, nothing changes
    m.call("gboost_hello", json!({})).await;
    m.call("gboost_deliver_code", json!({"code": e.last_code()})).await;
    let r = m.raw(Some(&m.key), &hdrs, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "gboost_whoami"}})).await;
    let v: Value = r.json().await.unwrap();
    let w: Value = serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(w["claimed_by_caller"]["x_forwarded_for"], "100.64.1.2");
    assert!(w["seen"]["peer"].as_str().unwrap().starts_with("127.0.0.1:"));
}

#[tokio::test]
async fn mcp_budget_and_expiry() {
    let e = env_with(|c| c.session_budget = 2, true).await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    m.call("gboost_hello", json!({})).await;
    m.call("gboost_deliver_code", json!({"code": e.last_code()})).await;
    assert_eq!(m.call("gboost_hello", json!({})).await.0, false);
    assert_eq!(m.call("gboost_hello", json!({})).await.0, false);
    let (er, t) = m.call("gboost_hello", json!({})).await;
    assert!(er && t.contains("budget"));
    let e2 = env_with(|c| c.session_ttl_s = 0, true).await;
    let m2 = Mcp { e: &e2, key: new_key(&e2).await.0 };
    m2.call("gboost_hello", json!({})).await;
    m2.call("gboost_deliver_code", json!({"code": e2.last_code()})).await;
    assert_eq!(m2.call("gboost_hello", json!({})).await.1, PENDING_MSG, "expired session starts a new check");
}

#[tokio::test]
async fn mcp_webhook_failures_and_rate_limit() {
    let e = env_with(|c| c.max_starts_per_hour = 1, true).await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    e.ctl.script.lock().unwrap().push_back((0, 500));
    let (er, t) = m.call("gboost_hello", json!({})).await;
    assert!(er && t.contains("could not reach"));
    // failed start left no pending code, but it counted against the limit
    let (er, t) = m.call("gboost_hello", json!({})).await;
    assert!(er && t.contains("Too many"));
    let e = env_with(|_| {}, false).await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    let (er, t) = m.call("gboost_hello", json!({})).await;
    assert!(er && t.contains("no routine webhook"));
    assert_eq!(m.rpc("tools/list", json!({})).await["result"]["tools"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn mcp_cooldown_between_checks() {
    let e = env_with(|c| c.check_cooldown_s = 60, true).await;
    let m = Mcp { e: &e, key: new_key(&e).await.0 };
    e.ctl.script.lock().unwrap().push_back((0, 500));
    assert!(m.call("gboost_hello", json!({})).await.1.contains("could not reach"));
    // failed start left no pending code, but the cooldown holds: no second webhook call
    assert_eq!(m.call("gboost_hello", json!({})).await, (true, "A session check was sent recently; wait.".to_string()));
    assert_eq!(hooks(&e), 1);
    // REST start is held by the same cooldown
    let (key, r) = new_key(&e).await;
    let _ = r;
    assert_eq!(e.start(&key).await.0, 200);
    assert_eq!(e.start(&key).await.0, 429);
    assert_eq!(hooks(&e), 2);
}

#[tokio::test]
async fn global_check_cap_across_clients() {
    let e = env_with(|c| c.max_checks_per_hour_global = 2, true).await;
    let (k1, _) = new_key(&e).await;
    let (k2, _) = new_key(&e).await;
    let (k3, _) = new_key(&e).await;
    assert_eq!(e.start(&k1).await.0, 200);
    assert_eq!(e.start(&k2).await.0, 200);
    assert_eq!(e.start(&k3).await.0, 429);
    assert_eq!(hooks(&e), 2);
    let m = Mcp { e: &e, key: k3 };
    assert!(m.call("gboost_hello", json!({})).await.1.contains("Too many"));
    assert_eq!(hooks(&e), 2);
}

#[tokio::test]
async fn deliver_code_keeps_an_open_session() {
    let e = env().await;
    let (key, _) = new_key(&e).await;
    let m = Mcp { e: &e, key: key.clone() };
    m.call("gboost_hello", json!({})).await;
    assert_eq!(m.call("gboost_deliver_code", json!({"code": e.last_code()})).await.1, "Session open");
    m.call("gboost_hello", json!({})).await;
    let w = |t: String| serde_json::from_str::<Value>(&t).unwrap();
    let before = w(m.call("gboost_whoami", json!({})).await.1);
    assert_eq!(before["calls_used"], 2);
    // a second check is started and answered while the session is open
    assert_eq!(e.start(&key).await.0, 200);
    assert_eq!(m.call("gboost_deliver_code", json!({"code": e.last_code()})).await, (false, "Session already open".to_string()));
    let after = w(m.call("gboost_whoami", json!({})).await.1);
    assert_eq!(after["calls_used"], 3, "budget not reset");
    assert_eq!((&after["session"], &after["created_ms"], &after["expires_ms"]), (&before["session"], &before["created_ms"], &before["expires_ms"]));
    // the code was consumed
    assert!(m.call("gboost_deliver_code", json!({"code": e.last_code()})).await.0);
}

#[tokio::test]
async fn webhook_post_timeout_cleans_the_code() {
    let e = env_with(|c| c.webhook_post_timeout_ms = 200, true).await;
    e.enrol(A.0, A.1).await;
    e.approve(A.0).await;
    e.ctl.script.lock().unwrap().push_back((1500, 200));
    let t = std::time::Instant::now();
    assert_eq!(e.start(&auth(A)).await.0, 502);
    assert!(t.elapsed() < std::time::Duration::from_millis(1200), "own timeout, not the slow hook");
    let (c, r) = e.post("/session/finish", &auth(A), None, json!({"code": e.last_code()})).await;
    assert_eq!((c, r["error"].as_str().unwrap()), (401, "no pending session check"));
    assert!(e.log("security.log").contains("webhook_fail"));
}

#[tokio::test]
async fn cancelled_start_cleans_the_code() {
    let e = env().await;
    let (key, _) = new_key(&e).await;
    e.ctl.script.lock().unwrap().push_back((1500, 200));
    let short = reqwest::Client::builder().timeout(std::time::Duration::from_millis(200)).build().unwrap();
    let r = short.post(format!("{}/session/start", e.agent)).header("x-gboost-client", &key).json(&json!({})).send().await;
    assert!(r.is_err(), "client gave up");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (c, r) = e.post("/session/finish", &key, None, json!({"code": e.last_code()})).await;
    assert_eq!((c, r["error"].as_str().unwrap()), (401, "no pending session check"));
}

#[tokio::test]
async fn new_client_to_file_refuses_overwrite_and_rolls_back_on_save_failure() {
    let e = env().await;
    e.owner("/local/clients/new?to_file=1").await;
    let first = std::fs::read_to_string(e.dir.join("new-client.txt")).unwrap();
    let (c, _) = e.owner_status("POST", "/local/clients/new?to_file=1", Some(&e.token)).await;
    assert_eq!(c, 409);
    assert_eq!(std::fs::read_to_string(e.dir.join("new-client.txt")).unwrap(), first);
    assert_eq!(e.clients().await.len(), 1, "refused request made no client");
    // persisting fails: no client stays in memory
    let _ = std::fs::remove_file(e.dir.join("clients.json"));
    std::fs::create_dir(e.dir.join("clients.json")).unwrap();
    let (c, _) = e.owner_status("POST", "/local/clients/new", Some(&e.token)).await;
    assert_eq!(c, 500);
    assert_eq!(e.clients().await.len(), 1);
}

#[cfg(unix)]
fn assert_owner_only(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600, "{}", p.display());
}
/// Windows: the file inherits the user-profile ACL; only check that it exists.
#[cfg(not(unix))]
fn assert_owner_only(p: &std::path::Path) {
    assert!(p.exists(), "{}", p.display());
}
