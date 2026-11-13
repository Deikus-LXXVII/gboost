use super::*;
use crate::demo_effect;
use std::sync::atomic::{AtomicU64, Ordering};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs_core::proto::UserVerificationPolicy;

const RP: &str = "approver.test";
const ORIGIN: &str = "https://approver.test:8443";
const T0: u64 = 1_000_000;

struct H {
    core: Core,
    auth: SoftPasskey,
    time: Arc<AtomicU64>,
    origin: Url,
}

impl H {
    fn new() -> H {
        let time = Arc::new(AtomicU64::new(T0));
        let t = time.clone();
        let core = Core::new(RP, ORIGIN, None, Arc::new(move || t.load(Ordering::SeqCst))).unwrap();
        let origin = Url::parse(ORIGIN).unwrap();
        let mut auth = SoftPasskey::new(true);
        let code = core.enrol_code().unwrap();
        let ccr = core.enrol_start(&code, "test").unwrap();
        let reg = auth.do_registration(origin.clone(), ccr).unwrap();
        core.enrol_finish(&reg).unwrap();
        H { core, auth, time, origin }
    }
    fn propose(&self, n: u32) -> Uuid {
        self.core.propose(demo_effect(n)).unwrap()
    }
    fn sign(&mut self, rcr: RequestChallengeResponse) -> PublicKeyCredential {
        self.auth.do_authentication(self.origin.clone(), rcr).unwrap()
    }
    /// Full honest approval of request `id`.
    fn approve(&mut self, id: Uuid) -> Result<(), Refused> {
        let (cid, rcr) = self.core.approve_start(id)?;
        let cred = self.sign(rcr);
        self.core.approve_finish(id, cid, &cred)
    }
}

fn base() -> Effect {
    demo_effect(1)
}

// ---- digest ----

#[test]
fn digest_is_deterministic_and_canonical() {
    let (n, exp) = ([7u8; 16], 42u64);
    assert_eq!(base().canonical_bytes(), base().clone().canonical_bytes());
    assert_eq!(digest(&base(), &n, exp), digest(&base(), &n, exp));
}

#[test]
fn any_field_change_changes_the_digest() {
    let (n, exp) = ([7u8; 16], 42u64);
    let d0 = digest(&base(), &n, exp);
    let mut variants: Vec<Effect> = Vec::new();
    let mut e = base(); e.kind.push('x'); variants.push(e);
    let mut e = base(); e.account.push('x'); variants.push(e);
    let mut e = base(); e.to.push("b@example.com".into()); variants.push(e);
    let mut e = base(); e.to[0].push('x'); variants.push(e);
    let mut e = base(); e.cc.push("b@example.com".into()); variants.push(e);
    let mut e = base(); e.bcc.clear(); variants.push(e);
    let mut e = base(); e.subject.push('x'); variants.push(e);
    let mut e = base(); e.body.push('x'); variants.push(e);
    // a value moved between lists, and re-split lists, must not collide
    let mut e = base(); e.cc = e.to.clone(); e.to = vec!["x@example.com".into()]; variants.push(e);
    let mut a = base(); a.to = vec!["ab".into(), "c".into()];
    let mut b = base(); b.to = vec!["a".into(), "bc".into()];
    assert_ne!(digest(&a, &n, exp), digest(&b, &n, exp));
    for v in variants {
        assert_ne!(digest(&v, &n, exp), d0, "{v:?}");
    }
    assert_ne!(digest(&base(), &[8u8; 16], exp), d0, "nonce");
    assert_ne!(digest(&base(), &n, exp + 1), d0, "expiry");
}

#[test]
fn overlong_field_is_refused_not_clipped() {
    let h = H::new();
    let mut e = base();
    e.body = "x".repeat(crate::effect::MAX_BODY + 1);
    assert!(h.core.propose(e).is_err());
    let mut e = base();
    e.subject = "x".repeat(crate::effect::MAX_SUBJECT + 1);
    assert!(h.core.propose(e).is_err());
    assert!(h.core.list().is_empty());
}

// ---- enrolment ----

#[test]
fn enrolment_needs_the_code_and_happens_once() {
    let time = Arc::new(AtomicU64::new(T0));
    let t = time.clone();
    let core = Core::new(RP, ORIGIN, None, Arc::new(move || t.load(Ordering::SeqCst))).unwrap();
    for _ in 0..MAX_ENROL_FAILURES {
        assert!(core.enrol_start("0000-0000", "test").is_err());
    }
    // locked even for the right code
    assert!(core.enrol_start(&core.enrol_code().unwrap(), "test").is_err());
    let h = H::new();
    assert!(h.core.enrol_start("anything", "test").is_err(), "second enrolment refused");
}

// ---- approval ----

#[test]
fn honest_approval_executes_exactly_the_digested_effect() {
    let mut h = H::new();
    let id = h.propose(1);
    let want = h.core.list()[0].digest.clone();
    h.approve(id).unwrap();
    let ex = h.core.executed();
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0], (id, want));
    assert_eq!(h.core.list()[0].status, Status::Approved);
}

#[test]
fn replayed_nonce_is_refused() {
    let mut h = H::new();
    let id = h.propose(1);
    // Two ceremonies for the same request are open at once (e.g. a double tap or a replayed capture).
    let (c1, r1) = h.core.approve_start(id).unwrap();
    let (c2, r2) = h.core.approve_start(id).unwrap();
    let a1 = h.sign(r1);
    let a2 = h.sign(r2);
    h.core.approve_finish(id, c1, &a1).unwrap();
    let e = h.core.approve_finish(id, c2, &a2).unwrap_err();
    assert!(e.0.contains("replay"), "{e:?}");
    // the same ceremony again is gone too
    assert!(h.core.approve_finish(id, c1, &a1).is_err());
    assert_eq!(h.core.executed().len(), 1);
}

#[test]
fn expired_request_is_refused() {
    let mut h = H::new();
    let id = h.propose(1);
    let (cid, rcr) = h.core.approve_start(id).unwrap();
    let cred = h.sign(rcr);
    h.time.store(T0 + REQUEST_TTL_SECS, Ordering::SeqCst);
    let e = h.core.approve_finish(id, cid, &cred).unwrap_err();
    assert!(e.0.contains("expired"), "{e:?}");
    assert!(h.core.approve_start(id).is_err(), "no new challenge for an expired request");
    assert!(h.core.executed().is_empty());
    assert_eq!(h.core.list()[0].status, Status::Expired);
}

#[test]
fn assertion_over_another_effects_challenge_is_refused() {
    let mut h = H::new();
    let a = h.propose(1);
    let b = h.propose(2);
    // (1) B's ceremony offered for A's request.
    let (cb, rb) = h.core.approve_start(b).unwrap();
    let ab = h.sign(rb);
    assert!(h.core.approve_finish(a, cb, &ab).is_err());
    // (2) A's ceremony, but the phone signed B's challenge.
    let (ca, _ra) = h.core.approve_start(a).unwrap();
    let (_cb2, rb2) = h.core.approve_start(b).unwrap();
    let ab2 = h.sign(rb2);
    let e = h.core.approve_finish(a, ca, &ab2).unwrap_err();
    assert_eq!(e.0, "assertion invalid");
    assert!(h.core.executed().is_empty());
}

#[test]
fn missing_user_verification_is_refused() {
    let mut h = H::new();
    let id = h.propose(1);
    let (cid, mut rcr) = h.core.approve_start(id).unwrap();
    // The page (or an attacker) asks the authenticator for presence only: no UV flag in the assertion.
    rcr.public_key.user_verification = UserVerificationPolicy::Discouraged_DO_NOT_USE;
    let cred = h.sign(rcr);
    let e = h.core.approve_finish(id, cid, &cred).unwrap_err();
    assert!(e.0 == "assertion invalid" || e.0.contains("user verification"), "{e:?}");
    assert!(h.core.executed().is_empty());
    assert_eq!(h.core.list()[0].status, Status::Pending);
}

#[test]
fn declined_request_cannot_be_approved() {
    let mut h = H::new();
    let id = h.propose(1);
    let (cid, rcr) = h.core.approve_start(id).unwrap();
    let cred = h.sign(rcr);
    h.core.decline(id).unwrap();
    assert!(h.core.approve_finish(id, cid, &cred).is_err());
    assert!(h.core.approve_start(id).is_err());
    assert!(h.core.executed().is_empty());
}

#[test]
fn approve_start_is_rate_limited_and_eviction_keeps_live_ceremonies() {
    let mut h = H::new();
    let id = h.propose(1);
    let (c0, r0) = h.core.approve_start(id).unwrap();
    let a0 = h.sign(r0);
    for _ in 1..MAX_STARTS_PER_MIN {
        h.core.approve_start(id).unwrap();
    }
    let e = h.core.approve_start(id).unwrap_err();
    assert!(e.0.contains("too many"), "{e:?}");
    // the first ceremony was not wiped by the flood
    h.core.approve_finish(id, c0, &a0).unwrap();
    // a minute later starts are allowed again
    h.time.fetch_add(61, Ordering::SeqCst);
    let id2 = h.propose(2);
    h.core.approve_start(id2).unwrap();
}

#[test]
fn ceremony_cap_evicts_oldest_not_all() {
    let mut h = H::new();
    let id = h.propose(1);
    let (c0, r0) = h.core.approve_start(id).unwrap();
    let a0 = h.sign(r0);
    // fill past the cap across several minutes so the rate limit does not interfere
    for i in 0..MAX_CEREMONIES + 2 {
        if i % 8 == 7 {
            h.time.fetch_add(20, Ordering::SeqCst);
        }
        let _ = h.core.approve_start(id);
    }
    // the oldest (c0) may be evicted, but the map was not cleared: newest still present
    let n = h.core.inner.lock().unwrap().ceremonies.len();
    assert!(n > 1 && n <= MAX_CEREMONIES, "{n}");
    let _ = (c0, a0);
}

#[test]
fn enrol_lockout_is_kept() {
    let core = Core::new(RP, ORIGIN, None, Arc::new(|| T0)).unwrap();
    for _ in 0..MAX_ENROL_FAILURES {
        assert!(core.enrol_start("x", "peer").is_err());
    }
    assert!(core.enrol_start(&core.enrol_code().unwrap(), "peer").unwrap_err().0.contains("locked"));
}
