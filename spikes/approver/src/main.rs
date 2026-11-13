//! Phase 0 spike S3: approval by passkey over an action digest, plus a Web Push test.
//! Binds 127.0.0.1 only. Put `tailscale serve` (or nothing) in front of it.

use approver::{core::*, demo_effect, push::Push};
use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::{AtomicU32, Ordering}, Arc},
};
use std::net::SocketAddr;
use uuid::Uuid;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

struct App {
    core: Core,
    push: Arc<Push>,
    console_token: String,
    demo_n: AtomicU32,
}
type S = State<Arc<App>>;

struct ApiErr(StatusCode, String);
impl From<Refused> for ApiErr {
    fn from(r: Refused) -> Self {
        log_limited(&format!("refused: {}", r.0));
        ApiErr(StatusCode::BAD_REQUEST, r.0)
    }
}
impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
type R = Result<Json<Value>, ApiErr>;

/// Console lines caused by unauthenticated callers are capped (5 a second) so a peer cannot flood the log.
fn log_limited(msg: &str) {
    use std::sync::Mutex;
    static W: Mutex<(u64, u32)> = Mutex::new((0, 0));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut w = W.lock().unwrap();
    if w.0 != now {
        if w.1 > 5 {
            println!("(suppressed {} lines)", w.1 - 5);
        }
        *w = (now, 0);
    }
    w.1 += 1;
    if w.1 <= 5 {
        println!("{msg}");
    }
}

/// Who is calling, for the log: peer address (always 127.0.0.1 behind `tailscale serve`) and the
/// Tailscale identity headers that `serve` adds. Display only, never trusted for a decision.
fn source(peer: SocketAddr, h: &HeaderMap) -> String {
    let t = |k: &str| {
        h.get(k)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.chars().filter(|c| !c.is_control()).take(80).collect::<String>())
            .unwrap_or_default()
    };
    format!("{peer} login={:?} name={:?}", t("tailscale-user-login"), t("tailscale-user-name"))
}

fn env(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

fn create_demo(app: &Arc<App>) {
    let n = app.demo_n.fetch_add(1, Ordering::SeqCst) + 1;
    match app.core.propose(demo_effect(n)) {
        Ok(id) => {
            println!("demo action {id} created ({} pending)", app.core.pending_count());
            app.push.notify();
        }
        Err(e) => println!("demo refused: {}", e.0),
    }
}

/// Before enrolment nothing is returned. After it, the list is readable by anyone who can reach the
/// page (no session in this spike); see the README limits.
async fn state(State(a): S) -> Json<Value> {
    if !a.core.enrolled() {
        return Json(json!({ "enrolled": false, "requests": [] }));
    }
    Json(json!({ "enrolled": true, "requests": a.core.list() }))
}

#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn enrol_start(State(a): S, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(c): Json<Code>) -> R {
    Ok(Json(serde_json::to_value(a.core.enrol_start(&c.code, &source(peer, &headers))?).unwrap()))
}
async fn enrol_finish(State(a): S, Json(c): Json<RegisterPublicKeyCredential>) -> R {
    a.core.enrol_finish(&c)?;
    Ok(Json(json!({ "ok": true })))
}

async fn approve_start(State(a): S, Path(id): Path<Uuid>) -> R {
    let (cid, opts) = a.core.approve_start(id)?;
    Ok(Json(json!({ "ceremony": cid, "options": opts })))
}
#[derive(Deserialize)]
struct Finish {
    ceremony: Uuid,
    credential: PublicKeyCredential,
}
async fn approve_finish(State(a): S, Path(id): Path<Uuid>, Json(f): Json<Finish>) -> R {
    a.core.approve_finish(id, f.ceremony, &f.credential)?;
    Ok(Json(json!({ "ok": true })))
}
async fn decline(State(a): S, Path(id): Path<Uuid>) -> R {
    a.core.decline(id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn push_key(State(a): S) -> Json<Value> {
    Json(json!({ "key": a.push.public_b64 }))
}
async fn push_subscribe(State(a): S, Json(s): Json<web_push::SubscriptionInfo>) -> R {
    if !a.core.enrolled() {
        return Err(ApiErr(StatusCode::FORBIDDEN, "enrol first".into()));
    }
    a.push.subscribe(s).map_err(|e| ApiErr(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true })))
}

/// Not authority: only creates a pending demo request. Needs the console token, so a page on the
/// tailnet cannot spam the inbox. (Behind `tailscale serve` every request looks local, so the
/// peer address is no test.)
async fn local_demo(State(a): S, headers: HeaderMap) -> Response {
    let given = headers.get("x-console-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !approver::core::ct_eq(given, &a.console_token) {
        log_limited("local/demo: bad console token");
        return StatusCode::FORBIDDEN.into_response();
    }
    create_demo(&a);
    StatusCode::OK.into_response()
}

fn asset(ct: &'static str, body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, ct)], body).into_response()
}

#[tokio::main]
async fn main() {
    let rp_id = env("APPROVER_RP_ID", "your-mac.your-tailnet.ts.net");
    let origin = env("APPROVER_ORIGIN", "https://your-mac.your-tailnet.ts.net:8443");
    let port: u16 = env("APPROVER_PORT", "8787").parse().expect("APPROVER_PORT");
    let dir = PathBuf::from(env("APPROVER_STATE_DIR", "state"));
    let sub = env("APPROVER_VAPID_SUB", &origin);

    let core = Core::new(&rp_id, &origin, Some(dir.clone()), system_clock()).expect("core");
    let push = Arc::new(Push::load_or_create(&dir, sub).expect("push"));
    let token = {
        use rand::RngCore;
        let mut b = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut b);
        approver::effect::hex(&b)
    };
    approver::push::write_private(&dir.join("console.token"), token.as_bytes()).expect("token");
    let app = Arc::new(App { core, push, console_token: token, demo_n: AtomicU32::new(0) });

    let router = Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", include_bytes!("../static/index.html")) }))
        .route("/app.js", get(|| async { asset("text/javascript", include_bytes!("../static/app.js")) }))
        .route("/sw.js", get(|| async { asset("text/javascript", include_bytes!("../static/sw.js")) }))
        .route("/manifest.webmanifest", get(|| async { asset("application/manifest+json", include_bytes!("../static/manifest.webmanifest")) }))
        .route("/icon.png", get(|| async { asset("image/png", include_bytes!("../static/icon.png")) }))
        .route("/api/state", get(state))
        .route("/api/enrol/start", post(enrol_start))
        .route("/api/enrol/finish", post(enrol_finish))
        .route("/api/requests/{id}/approve/start", post(approve_start))
        .route("/api/requests/{id}/approve/finish", post(approve_finish))
        .route("/api/requests/{id}/decline", post(decline))
        .route("/api/push/key", get(push_key))
        .route("/api/push/subscribe", post(push_subscribe))
        .route("/local/demo", post(local_demo))
        .layer(axum::middleware::map_response(|mut r: Response| async move {
            let h = r.headers_mut();
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            h.insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'"),
            );
            h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
            r
        }))
        .with_state(app.clone());

    println!("approver spike listening on http://127.0.0.1:{port} only");
    println!("RP ID  : {rp_id}\norigin : {origin}\nstate  : {}", dir.display());
    match app.core.enrol_code() {
        Some(c) => println!("\n  ENROLMENT CODE: {c}   (one time; 5 wrong tries lock it)\n"),
        None => println!("already enrolled (delete {}/passkeys.json to start over)", dir.display()),
    }
    println!("console: type `n` + Enter for a demo mail.send action, `l` to list.");
    println!("local  : curl -X POST -H \"x-console-token: $(cat {}/console.token)\" http://127.0.0.1:{port}/local/demo", dir.display());

    let a2 = app.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            match l.trim() {
                "n" => create_demo(&a2),
                "l" => {
                    for r in a2.core.list() {
                        println!("{} {:?} {}", r.id, r.status, r.effect.subject);
                    }
                }
                _ => {}
            }
        }
    });

    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    axum::serve(l, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
