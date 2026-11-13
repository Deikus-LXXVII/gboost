//! The effect an approval is about, its canonical bytes, and the digest the phone signs.
//! Canonical form: every field is length-prefixed (u32 big endian), lists are count-prefixed, so
//! no two different effects share bytes. A domain tag keeps the digest from meaning anything else.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_SUBJECT: usize = 200;
pub const MAX_BODY: usize = 4000;
pub const MAX_ADDRS: usize = 20;
const DOMAIN: &[u8] = b"bastion/approval/v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    /// Effect type, fixed wording per type on the screen. Demo: "mail.send".
    pub kind: String,
    /// Account the effect acts on, set by the core from the connection.
    pub account: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

fn put(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn put_list(out: &mut Vec<u8>, l: &[String]) {
    out.extend_from_slice(&(l.len() as u32).to_be_bytes());
    for s in l {
        put(out, s);
    }
}

impl Effect {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut o = Vec::new();
        put(&mut o, &self.kind);
        put(&mut o, &self.account);
        put_list(&mut o, &self.to);
        put_list(&mut o, &self.cc);
        put_list(&mut o, &self.bcc);
        put(&mut o, &self.subject);
        put(&mut o, &self.body);
        o
    }

    /// I3.2: a field too long to show in full is refused, never clipped.
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != "mail.send" {
            return Err("unknown effect type".into());
        }
        if self.subject.chars().count() > MAX_SUBJECT || self.body.chars().count() > MAX_BODY {
            return Err("field too long to display in full".into());
        }
        if self.to.is_empty() || self.to.len() + self.cc.len() + self.bcc.len() > MAX_ADDRS {
            return Err("recipient list empty or too long".into());
        }
        Ok(())
    }
}

/// challenge = SHA-256(domain || canonical(effect) || nonce || expiry)
pub fn digest(effect: &Effect, nonce: &[u8; 16], expiry: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(effect.canonical_bytes());
    h.update(nonce);
    h.update(expiry.to_be_bytes());
    h.finalize().into()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
