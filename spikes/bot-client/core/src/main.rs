use gboost_s9_core::{build, Config};
use std::net::SocketAddr;

fn env_u64(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    let agent_bind = std::env::var("S9_AGENT_BIND").unwrap_or_else(|_| "127.0.0.1:8790".into());
    let owner_bind = std::env::var("S9_OWNER_BIND").unwrap_or_else(|_| "127.0.0.1:8791".into());
    let state_dir = std::env::var("S9_STATE_DIR").unwrap_or_else(|_| "state".into());
    let state_path = std::path::PathBuf::from(&state_dir);
    let mut cfg = Config::new(state_path.clone());
    cfg.session_ttl_s = env_u64("S9_SESSION_TTL_S", cfg.session_ttl_s);
    cfg.code_ttl_s = env_u64("S9_CODE_TTL_S", cfg.code_ttl_s);
    cfg.check_cooldown_s = env_u64("S9_CHECK_COOLDOWN_S", cfg.check_cooldown_s);
    cfg.max_checks_per_hour_global = env_u64("S9_MAX_CHECKS_PER_HOUR", cfg.max_checks_per_hour_global as u64) as usize;
    let ab: SocketAddr = match agent_bind.parse() {
        Ok(a) => a,
        Err(_) => fail("S9_AGENT_BIND must be ip:port"),
    };
    if ab.ip().is_unspecified() && std::env::var("S9_ALLOW_ANY_BIND").as_deref() != Ok("1") {
        fail("refusing to bind the agent listener to 0.0.0.0 or [::]; bind to loopback (and publish it with a proxy such as `tailscale serve`) or to one private address, or set S9_ALLOW_ANY_BIND=1");
    }
    let ob: SocketAddr = match owner_bind.parse() {
        Ok(a) => a,
        Err(_) => fail("S9_OWNER_BIND must be ip:port"),
    };
    if !ob.ip().is_loopback() {
        fail("S9_OWNER_BIND must be a loopback address");
    }
    let (agent, owner, app) = match build(cfg) {
        Ok(x) => x,
        Err(e) => fail(&format!("cannot set up the state directory: {e}")),
    };
    let al = match tokio::net::TcpListener::bind(ab).await {
        Ok(l) => l,
        Err(e) => fail(&format!("cannot bind the agent listener: {e}")),
    };
    let ol = match tokio::net::TcpListener::bind(ob).await {
        Ok(l) => l,
        Err(e) => fail(&format!("cannot bind the owner listener: {e}")),
    };
    println!("Agent listener: http://{}", al.local_addr().map(|a| a.to_string()).unwrap_or_default());
    println!("Owner listener: http://{} (header x-console-token, file {})", ol.local_addr().map(|a| a.to_string()).unwrap_or_default(), state_path.join("console.token").display());
    if !app.routine_configured() {
        println!("NOTE: {} is missing; sessions cannot open until it has url and key.", state_path.join("routine.toml").display());
    }
    let a = axum::serve(al, agent.into_make_service_with_connect_info::<SocketAddr>());
    let o = axum::serve(ol, owner.into_make_service_with_connect_info::<SocketAddr>());
    let (ra, ro) = tokio::join!(a, o);
    if let Err(e) = ra.and(ro) {
        fail(&format!("listener stopped: {e}"));
    }
}
