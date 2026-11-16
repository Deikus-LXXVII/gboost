//! Web Push with VAPID. Payload-free: the service worker shows a fixed text, so the push service
//! learns nothing about the request. Logs the endpoint host only, never the endpoint or any key.

use std::{fs, io::Write, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Duration};
use web_push::{
    IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient, WebPushError,
    WebPushMessageBuilder,
};

pub const MAX_SUBS: usize = 5;
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Push {
    dir: PathBuf,
    private_b64: String,
    pub public_b64: String,
    sub_claim: String,
    subs: Mutex<Vec<SubscriptionInfo>>,
}

/// Owner-only file. Unix: mode 0600. Windows: the file inherits the user-profile ACL (only that user, SYSTEM
/// and Administrators), so the state directory must live under the user's profile.
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(data)
}

pub fn ensure_state_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn b64u(b: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(b)
}

/// Only the push services the three browsers use. Anything else is refused, so a tailnet peer cannot
/// make the box call an arbitrary https URL (SSRF).
pub fn host_allowed(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "fcm.googleapis.com"
        || h.ends_with(".push.apple.com")
        || h.ends_with(".push.services.mozilla.com")
}

fn host_of(endpoint: &str) -> String {
    url::Url::parse(endpoint).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| "?".into())
}

impl Push {
    pub fn load_or_create(dir: &Path, sub_claim: String) -> Result<Self, String> {
        ensure_state_dir(dir).map_err(|e| e.to_string())?;
        let kp = dir.join("vapid.key");
        let private_b64 = match fs::read_to_string(&kp) {
            Ok(s) => s.trim().to_string(),
            Err(_) => loop {
                use rand::RngCore;
                let mut k = [0u8; 32];
                rand::rngs::OsRng.fill_bytes(&mut k);
                let s = b64u(&k);
                if VapidSignatureBuilder::from_base64_no_sub(&s).is_ok() {
                    write_private(&kp, s.as_bytes()).map_err(|e| e.to_string())?;
                    break s;
                }
            },
        };
        let partial = VapidSignatureBuilder::from_base64_no_sub(&private_b64).map_err(|e| e.to_string())?;
        let public_b64 = b64u(&partial.get_public_key());
        let subs = fs::read_to_string(dir.join("subscriptions.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(Push { dir: dir.to_path_buf(), private_b64, public_b64, sub_claim, subs: Mutex::new(subs) })
    }

    pub fn subscribe(&self, sub: SubscriptionInfo) -> Result<(), String> {
        let u = url::Url::parse(&sub.endpoint).map_err(|_| "bad endpoint")?;
        if u.scheme() != "https" || !u.username().is_empty() || u.port().is_some() {
            return Err("endpoint must be plain https".into());
        }
        if !u.host_str().map(host_allowed).unwrap_or(false) {
            return Err("endpoint host not an allowed push service".into());
        }
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|s| s.endpoint != sub.endpoint);
        if subs.len() >= MAX_SUBS {
            return Err("too many subscriptions".into());
        }
        println!("push: subscription added, endpoint host {}", host_of(&sub.endpoint));
        subs.push(sub);
        self.persist(&subs);
        Ok(())
    }

    fn persist(&self, subs: &[SubscriptionInfo]) {
        let _ = write_private(&self.dir.join("subscriptions.json"), serde_json::to_string(subs).unwrap().as_bytes());
    }

    pub fn count(&self) -> usize {
        self.subs.lock().unwrap().len()
    }

    /// Send the content-free push to every subscription, each in its own task with a timeout, so a
    /// slow endpoint never holds up the caller.
    pub fn notify(self: &Arc<Self>) {
        let subs: Vec<SubscriptionInfo> = self.subs.lock().unwrap().clone();
        for sub in subs {
            let me = self.clone();
            tokio::spawn(async move { me.send_one(sub).await });
        }
    }

    async fn send_one(&self, sub: SubscriptionInfo) {
        let host = host_of(&sub.endpoint);
        let client = match IsahcWebPushClient::new() {
            Ok(c) => c,
            Err(e) => return println!("push: client error: {e}"),
        };
        let work = async {
            let mut sig = VapidSignatureBuilder::from_base64(&self.private_b64, &sub)?;
            sig.add_claim("sub", self.sub_claim.as_str());
            let mut m = WebPushMessageBuilder::new(&sub);
            m.set_vapid_signature(sig.build()?);
            m.set_ttl(3600);
            client.send(m.build()?).await
        };
        let res: Result<(), WebPushError> = match tokio::time::timeout(SEND_TIMEOUT, work).await {
            Ok(r) => r,
            Err(_) => return println!("push: send to {host} timed out"),
        };
        match res {
            Ok(()) => println!("push: sent to {host}"),
            Err(WebPushError::EndpointNotValid(_)) | Err(WebPushError::EndpointNotFound(_)) => {
                println!("push: {host} says subscription gone; removing");
                let mut subs = self.subs.lock().unwrap();
                subs.retain(|s| s.endpoint != sub.endpoint);
                self.persist(&subs);
            }
            Err(e) => println!("push: send to {host} failed: {e:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_real_push_hosts_pass() {
        for h in ["web.push.apple.com", "fcm.googleapis.com", "updates.push.services.mozilla.com"] {
            assert!(host_allowed(h), "{h}");
        }
        for h in ["127.0.0.1", "localhost", "evil.example", "push.apple.com.evil.example", "xpush.apple.com",
                  "169.254.169.254", "googleapis.com", "fcm.googleapis.com.evil.io"] {
            assert!(!host_allowed(h), "{h}");
        }
    }
    #[test]
    fn subscribe_validates_and_caps() {
        let dir = std::env::temp_dir().join(format!("approver-push-test-{}", uuid::Uuid::new_v4()));
        let p = Push::load_or_create(&dir, "https://x.test".into()).unwrap();
        let mk = |e: &str| SubscriptionInfo::new(e.to_string(), "k".to_string(), "a".to_string());
        assert!(p.subscribe(mk("https://127.0.0.1/x")).is_err());
        assert!(p.subscribe(mk("http://web.push.apple.com/x")).is_err());
        assert!(p.subscribe(mk("https://web.push.apple.com:8443/x")).is_err());
        for i in 0..MAX_SUBS {
            p.subscribe(mk(&format!("https://web.push.apple.com/{i}"))).unwrap();
        }
        assert!(p.subscribe(mk("https://web.push.apple.com/extra")).is_err());
        p.subscribe(mk("https://web.push.apple.com/0")).unwrap(); // same endpoint replaces, not adds
        assert_eq!(p.count(), MAX_SUBS);
        let _ = fs::remove_dir_all(&dir);
    }
}
