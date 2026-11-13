//! Approval core: enrolment, requests, challenge state, assertion verification, single use.
//! Everything that decides lives here; the page only draws and relays.

use crate::effect::{digest, hex, Effect};
use crate::push::{ensure_state_dir, write_private};
use rand::RngCore;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;
use uuid::Uuid;
use webauthn_rs::prelude::*;

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs())
}

/// A refusal carries a short reason; nothing about the cause of a crypto failure leaks to the page.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused(pub String);

fn refuse<T>(s: &str) -> Result<T, Refused> {
    Err(Refused(s.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Approved,
    Declined,
    Expired,
}

struct Request {
    effect: Effect,
    nonce: [u8; 16],
    expiry: u64,
    status: Status,
}

struct Ceremony {
    created: u64,
    request_id: Uuid,
    auth: PasskeyAuthentication,
    challenge: [u8; 32],
}

#[derive(Serialize)]
pub struct RequestView {
    pub id: Uuid,
    pub effect: Effect,
    pub expiry: u64,
    pub status: Status,
    pub digest: String,
}

struct Inner {
    passkeys: Vec<Passkey>,
    enrol_code: Option<String>,
    enrol_failures: u32,
    reg_state: Option<PasskeyRegistration>,
    requests: HashMap<Uuid, Request>,
    order: Vec<Uuid>,
    ceremonies: HashMap<Uuid, Ceremony>,
    spent: HashSet<[u8; 16]>,
    starts: Vec<u64>,
    executed: Vec<(Uuid, String)>,
}

pub struct Core {
    webauthn: Webauthn,
    inner: Mutex<Inner>,
    clock: Clock,
    state_dir: Option<PathBuf>,
}

const MAX_CEREMONIES: usize = 32;
const CEREMONY_TTL_SECS: u64 = 120;
const MAX_STARTS_PER_MIN: usize = 10;
const MAX_ENROL_FAILURES: u32 = 5;
pub const REQUEST_TTL_SECS: u64 = 300;

pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = (a.len() != b.len()) as u8;
    for i in 0..a.len().min(b.len()) {
        d |= a[i] ^ b[i];
    }
    d == 0
}

fn new_code() -> String {
    let n = rand::rngs::OsRng.next_u64() % 100_000_000;
    format!("{:04}-{:04}", n / 10_000, n % 10_000)
}

impl Core {
    pub fn new(rp_id: &str, origin: &str, state_dir: Option<PathBuf>, clock: Clock) -> Result<Self, String> {
        let origin = Url::parse(origin).map_err(|e| e.to_string())?;
        let webauthn = WebauthnBuilder::new(rp_id, &origin)
            .map_err(|e| format!("{e:?}"))?
            .rp_name("Bastion approver (spike)")
            .build()
            .map_err(|e| format!("{e:?}"))?;
        let mut passkeys: Vec<Passkey> = Vec::new();
        if let Some(d) = &state_dir {
            ensure_state_dir(d).map_err(|e| e.to_string())?;
            if let Ok(s) = std::fs::read_to_string(d.join("passkeys.json")) {
                passkeys = serde_json::from_str(&s).map_err(|e| e.to_string())?;
            }
        }
        let enrol_code = if passkeys.is_empty() { Some(new_code()) } else { None };
        Ok(Core {
            webauthn,
            inner: Mutex::new(Inner {
                passkeys,
                enrol_code,
                enrol_failures: 0,
                reg_state: None,
                requests: HashMap::new(),
                order: Vec::new(),
                ceremonies: HashMap::new(),
                spent: HashSet::new(),
                starts: Vec::new(),
                executed: Vec::new(),
            }),
            clock,
            state_dir,
        })
    }

    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// Printed on the server console only; stands in for "scan a code at the box".
    pub fn enrol_code(&self) -> Option<String> {
        self.inner.lock().unwrap().enrol_code.clone()
    }

    pub fn enrolled(&self) -> bool {
        !self.inner.lock().unwrap().passkeys.is_empty()
    }

    pub fn executed(&self) -> Vec<(Uuid, String)> {
        self.inner.lock().unwrap().executed.clone()
    }

    // ---- enrolment ----

    pub fn enrol_start(&self, code: &str, src: &str) -> Result<CreationChallengeResponse, Refused> {
        let mut g = self.inner.lock().unwrap();
        if !g.passkeys.is_empty() {
            return refuse("already enrolled");
        }
        if g.enrol_failures >= MAX_ENROL_FAILURES {
            return refuse("enrolment locked; restart the server");
        }
        let ok = g.enrol_code.as_deref().map(|c| ct_eq(c, code.trim())).unwrap_or(false);
        if !ok {
            g.enrol_failures += 1;
            println!("enrol: wrong code from {src} ({}/{MAX_ENROL_FAILURES})", g.enrol_failures);
            if g.enrol_failures >= MAX_ENROL_FAILURES {
                println!("enrol: LOCKED after {MAX_ENROL_FAILURES} failures. Reset: stop and restart the server (a new code is printed).");
            }
            return refuse("wrong enrolment code");
        }
        let (ccr, st) = self
            .webauthn
            .start_passkey_registration(Uuid::new_v4(), "owner", "Owner", None)
            .map_err(|_| Refused("registration failed".into()))?;
        g.reg_state = Some(st);
        Ok(ccr)
    }

    pub fn enrol_finish(&self, reg: &RegisterPublicKeyCredential) -> Result<(), Refused> {
        let mut g = self.inner.lock().unwrap();
        if !g.passkeys.is_empty() {
            return refuse("already enrolled");
        }
        let st = g.reg_state.take().ok_or_else(|| Refused("no enrolment in progress".into()))?;
        let pk = self
            .webauthn
            .finish_passkey_registration(reg, &st)
            .map_err(|_| Refused("registration failed".into()))?;
        g.passkeys.push(pk);
        g.enrol_code = None; // one time
        self.persist_passkeys(&g.passkeys);
        println!("enrol: passkey registered; enrolment code burned");
        Ok(())
    }

    fn persist_passkeys(&self, pks: &[Passkey]) {
        if let Some(d) = &self.state_dir {
            let _ = write_private(&d.join("passkeys.json"), serde_json::to_string(pks).unwrap().as_bytes());
        }
    }

    // ---- requests ----

    pub fn propose(&self, effect: Effect) -> Result<Uuid, Refused> {
        effect.validate().map_err(Refused)?;
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let id = Uuid::new_v4();
        let expiry = self.now() + REQUEST_TTL_SECS;
        let mut g = self.inner.lock().unwrap();
        g.requests.insert(id, Request { effect, nonce, expiry, status: Status::Pending });
        g.order.push(id);
        Ok(id)
    }

    fn sweep(&self, g: &mut Inner) {
        let now = self.now();
        for r in g.requests.values_mut() {
            if r.status == Status::Pending && now >= r.expiry {
                r.status = Status::Expired;
            }
        }
    }

    pub fn list(&self) -> Vec<RequestView> {
        let mut g = self.inner.lock().unwrap();
        self.sweep(&mut g);
        g.order
            .iter()
            .rev()
            .map(|id| {
                let r = &g.requests[id];
                RequestView {
                    id: *id,
                    effect: r.effect.clone(),
                    expiry: r.expiry,
                    status: r.status,
                    digest: hex(&digest(&r.effect, &r.nonce, r.expiry)),
                }
            })
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.list().iter().filter(|r| r.status == Status::Pending).count()
    }

    pub fn decline(&self, id: Uuid) -> Result<(), Refused> {
        let mut g = self.inner.lock().unwrap();
        self.sweep(&mut g);
        let r = g.requests.get_mut(&id).ok_or_else(|| Refused("unknown request".into()))?;
        if r.status != Status::Pending {
            return refuse("request is not pending");
        }
        r.status = Status::Declined;
        Ok(())
    }

    // ---- approval ----

    /// Starts a ceremony whose challenge is the action digest, not random bytes.
    pub fn approve_start(&self, id: Uuid) -> Result<(Uuid, RequestChallengeResponse), Refused> {
        let mut g = self.inner.lock().unwrap();
        self.sweep(&mut g);
        let r = g.requests.get(&id).ok_or_else(|| Refused("unknown request".into()))?;
        if r.status != Status::Pending {
            return refuse("request is not pending");
        }
        if g.spent.contains(&r.nonce) {
            return refuse("already used");
        }
        let chal = digest(&r.effect, &r.nonce, r.expiry);
        if g.passkeys.is_empty() {
            return refuse("not enrolled");
        }
        let (mut rcr, auth) = self
            .webauthn
            .start_passkey_authentication(&g.passkeys)
            .map_err(|_| Refused("cannot start".into()))?;
        let auth = set_challenge(auth, &chal)?;
        rcr.public_key.challenge = chal.to_vec().into();
        // Rate limit: at most MAX_STARTS_PER_MIN challenges a minute, for everyone together.
        let now = self.now();
        g.starts.retain(|t| now < t + 60);
        if g.starts.len() >= MAX_STARTS_PER_MIN {
            return refuse("too many approval attempts; wait a minute");
        }
        g.starts.push(now);
        // Make room: drop expired ceremonies, then the oldest one. Never the whole map.
        g.ceremonies.retain(|_, c| now < c.created + CEREMONY_TTL_SECS);
        while g.ceremonies.len() >= MAX_CEREMONIES {
            let oldest = g.ceremonies.iter().min_by_key(|(_, c)| c.created).map(|(k, _)| *k).unwrap();
            g.ceremonies.remove(&oldest);
        }
        let cid = Uuid::new_v4();
        g.ceremonies.insert(cid, Ceremony { created: now, request_id: id, auth, challenge: chal });
        Ok((cid, rcr))
    }

    pub fn approve_finish(&self, id: Uuid, cid: Uuid, cred: &PublicKeyCredential) -> Result<(), Refused> {
        let mut g = self.inner.lock().unwrap();
        self.sweep(&mut g);
        // A ceremony is used once, whatever the outcome.
        let cer = g.ceremonies.remove(&cid).ok_or_else(|| Refused("no such challenge".into()))?;
        if self.now() >= cer.created + CEREMONY_TTL_SECS {
            return refuse("challenge expired");
        }
        if cer.request_id != id {
            return refuse("challenge belongs to another request");
        }
        let (effect, nonce, expiry, status) = {
            let r = g.requests.get(&id).ok_or_else(|| Refused("unknown request".into()))?;
            (r.effect.clone(), r.nonce, r.expiry, r.status)
        };
        if self.now() >= expiry || status == Status::Expired {
            return refuse("expired");
        }
        if status == Status::Declined {
            return refuse("request was declined");
        }
        // Single use: the nonce is the guard (I1.4).
        if g.spent.contains(&nonce) {
            return refuse("replay: nonce already spent");
        }
        // The challenge the phone signed must be the digest of exactly this effect (I3.1).
        if cer.challenge != digest(&effect, &nonce, expiry) {
            return refuse("digest mismatch");
        }
        let res = self
            .webauthn
            .finish_passkey_authentication(cred, &cer.auth)
            .map_err(|_| Refused("assertion invalid".into()))?;
        // Belt and braces: the library enforces the policy too, the core does not rely on it alone.
        if !res.user_verified() {
            return refuse("user verification missing");
        }
        for pk in g.passkeys.iter_mut() {
            pk.update_credential(&res);
        }
        self.persist_passkeys(&g.passkeys);
        g.spent.insert(nonce);
        g.requests.get_mut(&id).unwrap().status = Status::Approved;
        // "Execute": the core acts on the effect it holds, never on anything the phone sent.
        let dg = hex(&cer.challenge);
        println!(
            "EXECUTE {} to={:?} subject={:?} digest={}",
            effect.kind, effect.to, effect.subject, dg
        );
        g.executed.push((id, dg));
        Ok(())
    }
}

/// Replace the random challenge in a started authentication with the action digest.
/// webauthn-rs has no public API for a caller-chosen challenge; the serialisable state is the way in.
fn set_challenge(auth: PasskeyAuthentication, chal: &[u8; 32]) -> Result<PasskeyAuthentication, Refused> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut v = serde_json::to_value(&auth).map_err(|_| Refused("state".into()))?;
    let slot = v.pointer_mut("/ast/challenge").ok_or_else(|| Refused("state layout changed".into()))?;
    *slot = serde_json::Value::String(URL_SAFE_NO_PAD.encode(chal));
    serde_json::from_value(v).map_err(|_| Refused("state".into()))
}

#[cfg(test)]
mod tests;
