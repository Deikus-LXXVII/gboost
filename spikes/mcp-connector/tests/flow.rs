use axum::{body::Body, extract::ConnectInfo, http::{header, Request, StatusCode}, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use http_body_util::BodyExt;
use mcp_connector::{build, Config};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use tower::ServiceExt;

struct T {
    app: mcp_connector::App,
    r: Router,
    dir: std::path::PathBuf,
    id: String,
    secret: String,
}
fn setup(ttl_ms: u64) -> T {
    setup_with(ttl_ms, |_| {})
}
fn setup_with(ttl_ms: u64, f: impl FnOnce(&mut Config)) -> T {
    let dir = std::env::temp_dir().join(format!("bst-test-{}-{}", std::process::id(), rand::random::<u64>()));
    let mut cfg = Config::new("https://example.test".into(), dir.clone(), ttl_ms);
    f(&mut cfg);
    let (r, app, fresh) = build(cfg);
    let (id, secret) = fresh.unwrap();
    T { app, r, dir, id, secret }
}
async fn send(r: &Router, mut req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    req.extensions_mut().insert(ConnectInfo("127.0.0.1:5555".parse::<SocketAddr>().unwrap()));
    let resp = r.clone().oneshot(req).await.unwrap();
    let (p, b) = resp.into_parts();
    (p.status, p.headers, String::from_utf8_lossy(&b.collect().await.unwrap().to_bytes()).into_owned())
}
fn form(uri: &str, body: String, auth: Option<String>) -> Request<Body> {
    let mut b = Request::post(uri).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded").header("user-agent", "test-agent/1");
    if let Some(a) = auth {
        b = b.header(header::AUTHORIZATION, a);
    }
    b.body(Body::from(body)).unwrap()
}
fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
const VERIFIER: &str = "verifier-verifier-verifier-verifier-verifier-1234";
const RU: &str = "https://grok.test/callback";

/// Runs authorize + approve, returns the code.
async fn get_code(t: &T) -> String {
    let ch = B64.encode(Sha256::digest(VERIFIER.as_bytes()));
    let q = format!("/authorize?response_type=code&client_id={}&redirect_uri={}&state=xyz&code_challenge={}&code_challenge_method=S256", enc(&t.id), enc(RU), ch);
    let (st, _, page) = send(&t.r, Request::get(q).body(Body::empty()).unwrap()).await;
    assert_eq!(st, StatusCode::OK);
    assert!(page.contains("Approve") && page.contains("Deny"));
    let rid = page.split("name=req value='").nth(1).unwrap().split('\'').next().unwrap().to_string();
    let (st, h, _) = send(&t.r, form("/authorize/decision", format!("req={}&decision=approve&pin={}", enc(&rid), t.app.pin()), None)).await;
    assert_eq!(st, StatusCode::FOUND);
    let loc = url::Url::parse(h[header::LOCATION].to_str().unwrap()).unwrap();
    assert!(loc.query_pairs().any(|(k, v)| k == "state" && v == "xyz"));
    loc.query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned()
}
async fn exchange(t: &T, code: &str, verifier: &str) -> (StatusCode, Value) {
    let basic = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", t.id, t.secret)));
    let body = format!("grant_type=authorization_code&code={}&redirect_uri={}&code_verifier={}", enc(code), enc(RU), enc(verifier));
    let (st, _, b) = send(&t.r, form("/token", body, Some(basic))).await;
    (st, serde_json::from_str(&b).unwrap())
}
async fn rpc(t: &T, tok: Option<&str>, body: Value) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut b = Request::post("/mcp").header(header::CONTENT_TYPE, "application/json").header("user-agent", "test-agent/1");
    if let Some(k) = tok {
        b = b.header(header::AUTHORIZATION, format!("Bearer {k}"));
    }
    send(&t.r, b.body(Body::from(body.to_string())).unwrap()).await
}
async fn login(t: &T) -> String {
    let code = get_code(t).await;
    let (st, v) = exchange(t, &code, VERIFIER).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["access_token"].as_str().unwrap().starts_with("bst_"));
    v["access_token"].as_str().unwrap().into()
}

#[tokio::test]
async fn no_token_is_401_with_metadata_pointer() {
    let t = setup(60_000);
    let (st, h, _) = rpc(&t, None, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(h[header::WWW_AUTHENTICATE].to_str().unwrap().contains("resource_metadata=\"https://example.test/.well-known/oauth-protected-resource\""));
    let (st, _, _) = rpc(&t, Some("bst_garbage"), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pkce_flow_yields_working_token_and_tools() {
    let t = setup(60_000);
    let tok = login(&t).await;
    let (st, h, _) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}})).await;
    assert_eq!(st, StatusCode::OK);
    let sid = h["mcp-session-id"].to_str().unwrap().to_string();
    let (_, _, b) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    let v: Value = serde_json::from_str(&b).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["hello", "whoami", "pending_request", "wake_me"]);
    let (_, _, b) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"whoami"}})).await;
    assert!(b.contains("tid_") && !b.contains(&tok));
    assert!(sid.starts_with("sess_"));
    let (_, _, b) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"wake_me","arguments":{"seconds":1}}})).await;
    assert!(b.contains("not scheduled"));
}

#[tokio::test]
async fn wrong_verifier_refused_and_code_single_use() {
    let t = setup(60_000);
    let code = get_code(&t).await;
    let (st, v) = exchange(&t, &code, "wrong-wrong-wrong-wrong-wrong-wrong-wrong-wrong").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"], "invalid_grant");
    // The failed attempt burned the code.
    let (st, _) = exchange(&t, &code, VERIFIER).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn plain_pkce_and_bad_secret_refused() {
    let t = setup(60_000);
    let q = format!("/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=plain", enc(&t.id), enc(RU), VERIFIER);
    let (st, h, _) = send(&t.r, Request::get(q).body(Body::empty()).unwrap()).await;
    assert_eq!(st, StatusCode::FOUND);
    assert!(h[header::LOCATION].to_str().unwrap().contains("error=invalid_request"));
    let code = get_code(&t).await;
    let body = format!("grant_type=authorization_code&code={}&redirect_uri={}&code_verifier={}&client_id={}&client_secret=nope", enc(&code), enc(RU), VERIFIER, enc(&t.id));
    let (st, _, _) = send(&t.r, form("/token", body, None)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_token_refused() {
    let t = setup(0);
    let tok = login(&t).await;
    let (st, _, _) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_rotates_and_log_never_holds_secrets() {
    let t = setup(60_000);
    let code = get_code(&t).await;
    let basic = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", t.id, t.secret)));
    let body = format!("grant_type=authorization_code&code={}&redirect_uri={}&code_verifier={}", enc(&code), enc(RU), enc(VERIFIER));
    let (_, _, b) = send(&t.r, form("/token", body, Some(basic.clone()))).await;
    let v: Value = serde_json::from_str(&b).unwrap();
    let (access, refresh) = (v["access_token"].as_str().unwrap().to_string(), v["refresh_token"].as_str().unwrap().to_string());
    rpc(&t, Some(&access), json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"hello","arguments":{"name":"x"}}})).await;
    let (st, _, b) = send(&t.r, form("/token", format!("grant_type=refresh_token&refresh_token={}", enc(&refresh)), Some(basic.clone()))).await;
    assert_eq!(st, StatusCode::OK);
    let v2: Value = serde_json::from_str(&b).unwrap();
    let (st, _, _) = send(&t.r, form("/token", format!("grant_type=refresh_token&refresh_token={}", enc(&refresh)), Some(basic))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST); // old refresh token is spent
    let (st, _, _) = rpc(&t, Some(v2["access_token"].as_str().unwrap()), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
    assert_eq!(st, StatusCode::OK);

    let log = std::fs::read_to_string(t.dir.join("requests.log")).unwrap();
    assert!(log.contains("test-agent/1") && log.contains("\"tool\":\"hello\"") && log.contains("tid_") && log.contains("127.0.0.1:5555"));
    let secrets: Vec<String> = vec![access.clone(), refresh.clone(), t.secret.clone(), v2["access_token"].as_str().unwrap().to_string(), code.clone()];
    for secret in &secrets {
        assert!(!log.contains(secret.as_str()), "log leaked a secret");
    }
    assert!(!log.contains(&B64.encode(format!("{}:{}", t.id, t.secret))));
}

async fn page_rid(t: &T) -> (String, String) {
    let ch = B64.encode(Sha256::digest(VERIFIER.as_bytes()));
    let q = format!("/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256", enc(&t.id), enc(RU), ch);
    let (st, h, page) = send(&t.r, Request::get(q).body(Body::empty()).unwrap()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(h["x-frame-options"], "DENY");
    assert!(h["content-security-policy"].to_str().unwrap().contains("frame-ancestors 'none'"));
    (page.split("name=req value='").nth(1).unwrap().split('\'').next().unwrap().to_string(), page)
}

#[tokio::test]
async fn register_is_off_by_default_and_on_when_enabled() {
    let t = setup(60_000);
    let body = json!({"redirect_uris":["https://x.test/cb"],"client_name":"evil"}).to_string();
    let mk = || Request::post("/register").header(header::CONTENT_TYPE, "application/json").body(Body::from(body.clone())).unwrap();
    assert_eq!(send(&t.r, mk()).await.0, StatusCode::NOT_FOUND);
    let (_, _, meta) = send(&t.r, Request::get("/.well-known/oauth-authorization-server").body(Body::empty()).unwrap()).await;
    assert!(!meta.contains("registration_endpoint"));
    let t2 = setup_with(60_000, |c| c.allow_dcr = true);
    let (st, _, b) = send(&t2.r, mk()).await;
    assert_eq!(st, StatusCode::CREATED);
    let log = std::fs::read_to_string(t2.dir.join("requests.log")).unwrap();
    assert!(!log.contains("evil") && !log.contains("x.test/cb"), "register body was logged");
    let _ = b;
}

#[tokio::test]
async fn consent_needs_the_pin_and_locks_after_five_failures() {
    let t = setup(60_000);
    let (rid, _) = page_rid(&t).await;
    // No PIN field at all: rejected.
    let (st, _, _) = send(&t.r, form("/authorize/decision", format!("req={}&decision=approve", enc(&rid)), None)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    for _ in 0..5 {
        let (st, h, _) = send(&t.r, form("/authorize/decision", format!("req={}&decision=approve&pin=00000000x", enc(&rid)), None)).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        assert!(!h.contains_key(header::LOCATION));
    }
    // Locked: even the right PIN is refused now.
    let (st, _, _) = send(&t.r, form("/authorize/decision", format!("req={}&decision=approve&pin={}", enc(&rid), t.app.pin()), None)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(t.app.pin().len() >= 6);
}

#[tokio::test]
async fn right_pin_after_a_few_wrong_ones_still_works() {
    let t = setup(60_000);
    let (rid, _) = page_rid(&t).await;
    for _ in 0..4 {
        send(&t.r, form("/authorize/decision", format!("req={}&decision=approve&pin=1", enc(&rid)), None)).await;
    }
    let (st, h, _) = send(&t.r, form("/authorize/decision", format!("req={}&decision=approve&pin={}", enc(&rid), t.app.pin()), None)).await;
    assert_eq!(st, StatusCode::FOUND);
    assert!(h[header::LOCATION].to_str().unwrap().contains("code="));
}

#[tokio::test]
async fn unknown_session_is_404_and_session_is_bound_to_token() {
    let t = setup(60_000);
    let tok = login(&t).await;
    let call = |sid: &str, tok: &str| {
        Request::post("/mcp").header(header::AUTHORIZATION, format!("Bearer {tok}")).header("mcp-session-id", sid.to_string())
            .body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string())).unwrap()
    };
    assert_eq!(send(&t.r, call("sess_forged", &tok)).await.0, StatusCode::NOT_FOUND);
    let (_, h, _) = rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})).await;
    let sid = h["mcp-session-id"].to_str().unwrap().to_string();
    assert_eq!(send(&t.r, call(&sid, &tok)).await.0, StatusCode::OK);
    let tok2 = login(&t).await;
    assert_eq!(send(&t.r, call(&sid, &tok2)).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn code_replay_revokes_the_tokens() {
    let t = setup(60_000);
    let code = get_code(&t).await;
    let (st, v) = exchange(&t, &code, VERIFIER).await;
    assert_eq!(st, StatusCode::OK);
    let tok = v["access_token"].as_str().unwrap().to_string();
    assert_eq!(rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":1,"method":"ping"})).await.0, StatusCode::OK);
    assert_eq!(exchange(&t, &code, VERIFIER).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(rpc(&t, Some(&tok), json!({"jsonrpc":"2.0","id":1,"method":"ping"})).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wake_is_clamped_and_one_pending() {
    let t = setup(60_000);
    std::fs::write(t.dir.join("routine.toml"), "url = \"http://127.0.0.1:1/hook\"\nkey = \"k\"\n").unwrap();
    let tok = login(&t).await;
    let call = |s: u64| json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wake_me","arguments":{"seconds":s}}});
    let (_, _, b) = rpc(&t, Some(&tok), call(99999)).await;
    assert!(b.contains("scheduled: POST") && b.contains("in 600 s"), "{b}");
    let (_, _, b) = rpc(&t, Some(&tok), call(5)).await;
    assert!(b.contains("already pending"), "{b}");
}

#[tokio::test]
async fn log_values_truncated_rotated_and_files_private() {
    let t = setup_with(60_000, |c| c.log_max_bytes = 20_000);
    let big = "A".repeat(5000);
    for _ in 0..40 {
        let req = Request::post("/mcp").header("x-big", big.clone()).header("user-agent", big.clone()).body(Body::from("{}")).unwrap();
        send(&t.r, req).await;
    }
    let log = std::fs::read_to_string(t.dir.join("requests.log")).unwrap();
    assert!(!log.contains(&"A".repeat(600)), "value not truncated");
    assert!(log.contains("[truncated]"));
    assert!(t.dir.join("requests.log.1").exists(), "log did not rotate");
    assert!(log.len() <= 20_000);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["requests.log", "client.json"] {
            assert_eq!(std::fs::metadata(t.dir.join(f)).unwrap().permissions().mode() & 0o777, 0o600, "{f}");
        }
    }
}

#[tokio::test]
async fn oversized_body_refused_and_maps_stay_capped() {
    let t = setup_with(60_000, |c| c.max_entries = 3);
    let (st, _, _) = send(&t.r, Request::post("/mcp").body(Body::from(vec![b'x'; 70_000])).unwrap()).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    for _ in 0..10 {
        page_rid(&t).await;
    }
    assert!(t.app.pending_len() <= 3, "pending grew to {}", t.app.pending_len());
}
