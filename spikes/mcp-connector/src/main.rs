use mcp_connector::{build, Config};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("BASTION_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let public_url = std::env::var("BASTION_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let state_dir = std::env::var("BASTION_STATE_DIR").unwrap_or_else(|_| "state".into()).into();
    let ttl: u64 = std::env::var("BASTION_TOKEN_TTL_SECS").ok().and_then(|p| p.parse().ok()).unwrap_or(3600);
    let (router, app, fresh) = build({ let mut c = Config::new(public_url.clone(), state_dir, ttl * 1000); c.allow_dcr = std::env::var("BASTION_ALLOW_DCR").as_deref() == Ok("1"); c });
    match fresh {
        Some((id, secret)) => {
            println!("=== First start: OAuth client created (shown once) ===\nClient ID:     {id}\nClient Secret: {secret}\n=====");
        }
        None => println!("Client credentials exist in state/client.json (secret is stored hashed; delete the file to regenerate)."),
    }
    println!("Owner PIN (type it on the consent page): {}", app.pin());
    println!("Public base URL: {public_url}  (MCP endpoint {public_url}/mcp)");
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    println!("Listening on 127.0.0.1:{port}");
    axum::serve(l, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
