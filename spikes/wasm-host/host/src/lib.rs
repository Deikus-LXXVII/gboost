//! S1 spike: host gives a plugin component exactly one capability, `fetch`, gated by a manifest allow-list.
use wasmtime::{bail, format_err, Result};
use serde::Deserialize;
use url::Url;
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store};

wasmtime::component::bindgen!({ path: "../wit", world: "plugin" });

use bastion::plugin::http::Response;

/// WASI interfaces this host offers to guests. Deliberately empty: no ambient authority.
pub const OFFERED_WASI: &[&str] = &[];
/// Non-WASI imports a component may have.
const OWN_IMPORTS: &[&str] = &["bastion:plugin/http@0.1.0", "bastion:plugin/types@0.1.0"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub api: String,
    pub net: Net,
    #[serde(default)]
    pub wasi: Vec<String>, // WASI interfaces the plugin asks for
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub fetch: Vec<String>, // allowed origins: scheme://host[:port], nothing else
}

/// Canonical origin ("https://host[:port]") of a manifest entry; refuses anything but a bare https origin.
fn manifest_origin(s: &str) -> Result<String> {
    let u = Url::parse(s)?;
    if u.scheme() != "https" || u.path() != "/" || u.query().is_some() || u.fragment().is_some()
        || !u.username().is_empty() || u.password().is_some() || s.contains('*') || s.ends_with('/') {
        bail!("manifest origin must be a bare https origin, got {s:?}");
    }
    check_host(&u)?;
    Ok(u.origin().ascii_serialization())
}

fn check_host(u: &Url) -> Result<()> {
    match u.host() {
        Some(url::Host::Domain(d)) if !d.ends_with('.') && d.contains('.') => Ok(()),
        _ => bail!("host must be a dotted domain name (no IP literal, no trailing dot)"),
    }
}

/// THE GATE. True only if `raw` is https, has no userinfo, names a domain host in canonical (lowercase,
/// no trailing dot) form, and its origin is exactly one of `allowed`.
pub fn origin_allowed(allowed: &[String], raw: &str) -> bool {
    if raw.chars().any(|c| c.is_control() || c == '\\' || c == ' ') {
        return false;
    }
    let Ok(u) = Url::parse(raw) else { return false };
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || check_host(&u).is_err() {
        return false;
    }
    let origin = u.origin().ascii_serialization();
    // the text as written must already be canonical: rejects HTTPS://ALLOWED.example.com and friends
    if !raw.starts_with(&origin) {
        return false;
    }
    // what follows the origin must start a path/query/fragment (or an explicit default port), not more host
    let rest = &raw[origin.len()..];
    if !(rest.is_empty() || rest.starts_with(['/', '?', '#']) || rest == ":443" || rest.starts_with(":443/")) {
        return false;
    }
    allowed.iter().any(|a| *a == origin)
}

pub struct HostState {
    allowed: Vec<String>,
    pub fetched: Vec<String>, // URLs that passed the gate (the fake "network")
    pub refused: Vec<String>,
}

impl bastion::plugin::types::Host for HostState {}
impl bastion::plugin::http::Host for HostState {
    fn fetch(&mut self, url: String) -> Result<Response, String> {
        if !origin_allowed(&self.allowed, &url) {
            self.refused.push(url.clone());
            return Err(format!("origin not allowed by manifest: {url}"));
        }
        self.fetched.push(url.clone());
        Ok(Response { status: 200, body: format!("canned body for {url}") })
    }
}

pub struct Loaded {
    pub store: Store<HostState>,
    pub plugin: Plugin,
}

pub fn engine() -> Engine {
    let mut c = wasmtime::Config::new();
    c.wasm_component_model_async(true); // the QuickJS-based TS guest uses waitable.join
    Engine::new(&c).unwrap()
}

/// Check manifest and component imports, then instantiate with only `fetch` linked.
pub fn load(engine: &Engine, manifest_toml: &str, wasm: &[u8]) -> Result<Loaded> {
    load_inner(engine, manifest_toml, |e| Component::from_binary(e, wasm))
}

/// # Safety
/// `pre` must come from `Engine::precompile_component` by this host (trusted install-time artifact).
pub unsafe fn load_precompiled(engine: &Engine, manifest_toml: &str, pre: &[u8]) -> Result<Loaded> {
    load_inner(engine, manifest_toml, |e| unsafe { Component::deserialize(e, pre) })
}

fn load_inner(engine: &Engine, manifest_toml: &str, mk: impl FnOnce(&Engine) -> Result<Component>) -> Result<Loaded> {
    let m: Manifest = toml::from_str(manifest_toml)?;
    if m.api != "bastion:plugin@0.1" {
        bail!("unsupported api {}", m.api);
    }
    for w in &m.wasi {
        if !OFFERED_WASI.contains(&w.as_str()) {
            bail!("manifest {} names WASI import {w:?}, which this host does not offer", m.id);
        }
    }
    let allowed = m.net.fetch.iter().map(|s| manifest_origin(s)).collect::<Result<Vec<_>>>()?;
    let component = mk(engine)?;
    for (name, _) in component.component_type().imports(engine) {
        let base = name.split('@').next().unwrap_or(name);
        let ok = OWN_IMPORTS.contains(&name) || (base.starts_with("wasi:") && OFFERED_WASI.iter().any(|o| o.starts_with(base)));
        if !ok {
            bail!("component imports {name:?}, which the host does not provide");
        }
    }
    let mut linker = Linker::new(engine);
    Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s: &mut HostState| s)?;
    let mut store = Store::new(engine, HostState { allowed, fetched: vec![], refused: vec![] });
    let plugin = Plugin::instantiate(&mut store, &component, &linker).map_err(|e| format_err!("instantiate: {e:#}"))?;
    Ok(Loaded { store, plugin })
}

impl Loaded {
    pub fn prepare(&mut self, tool: &str, params: &str) -> Result<Result<Effect, String>> {
        self.plugin.call_prepare(&mut self.store, tool, params)
    }
}
