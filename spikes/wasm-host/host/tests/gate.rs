use wasm_host::bastion::plugin::types::Effect;
use wasm_host::{engine, load, origin_allowed};

const MANIFEST: &str = r#"
id = "demo"
api = "bastion:plugin@0.1"
[net]
fetch = ["https://allowed.example.com"]
"#;
const GUESTS: &[&str] = &["rust", "ts-sm", "ts-qjs"];

fn wasm(name: &str) -> Vec<u8> {
    let p = format!("{}/../artifacts/{name}.wasm", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e} (run ../build.sh)"))
}

#[test]
fn allowed_origin_succeeds_and_prepare_returns_typed_effect() {
    for g in GUESTS {
        let mut l = load(&engine(), MANIFEST, &wasm(g)).unwrap();
        let e = l.prepare("mail", "https://allowed.example.com/x?q=1").unwrap().unwrap();
        match e {
            Effect::MailSend(m) => {
                assert_eq!(m.to, vec!["a@example.com"], "{g}");
                assert_eq!(m.subject, "status 200", "{g}");
                assert_eq!(m.body, "canned body for https://allowed.example.com/x?q=1", "{g}");
            }
            other => panic!("{g}: wrong effect {other:?}"),
        }
        assert_eq!(l.store.data().fetched.len(), 1);
        match l.prepare("note", "hi").unwrap().unwrap() {
            Effect::NoteWrite(n) => assert_eq!((n.path.as_str(), n.text.as_str()), ("n.md", "hi"), "{g}"),
            other => panic!("{g}: wrong effect {other:?}"),
        }
        assert!(l.prepare("nope", "").unwrap().is_err());
    }
}

const BAD: &[&str] = &[
    "https://evil.com/",
    "https://allowed.example.com.evil.com/",
    "https://allowed.example.com@evil.com/",
    "https://allowed.example.com:pw@evil.com/",
    "https://allowed.example.com%2f@evil.com/",
    "https://user@allowed.example.com/",
    "https://user:pw@allowed.example.com/",
    "HTTPS://ALLOWED.EXAMPLE.COM/",
    "https://ALLOWED.example.com/",
    "https://allowed.example.com./",
    "https://allowed.example.com.",
    "https://93.184.216.34/",
    "https://[::1]/",
    "https://0x5db8d822/",
    "http://allowed.example.com/",
    "https://allowed.example.com:8443/",
    "https://allowed.example.com\\@evil.com/",
    "https://allowed.example.com\t.evil.com/",
    "https://allowed.example.com:443@evil.com/",
    "https://allowed.example.comevil.com/",
    "ftp://allowed.example.com/",
    "//allowed.example.com/",
    "https:/\\evil.com",
    "",
];

#[test]
fn gate_function_refuses_tricks() {
    let allowed = vec!["https://allowed.example.com".to_string()];
    for u in BAD {
        assert!(!origin_allowed(&allowed, u), "should refuse {u:?}");
    }
    for u in ["https://allowed.example.com", "https://allowed.example.com/", "https://allowed.example.com/a?b#c", "https://allowed.example.com:443/a"] {
        assert!(origin_allowed(&allowed, u), "should allow {u:?}");
    }
}

#[test]
fn guests_cannot_reach_refused_origins() {
    for g in GUESTS {
        let mut l = load(&engine(), MANIFEST, &wasm(g)).unwrap();
        for u in BAD.iter().filter(|u| !u.is_empty()) {
            let r = l.prepare("mail", u).unwrap();
            assert!(r.is_err(), "{g}: {u:?} should be refused, got {r:?}");
        }
        assert!(l.store.data().fetched.is_empty(), "{g}: fake network was reached");
        assert!(!l.store.data().refused.is_empty());
    }
}

#[test]
fn manifest_naming_unoffered_wasi_fails_to_load() {
    let m = format!("wasi = [\"wasi:sockets/tcp\"]\n{MANIFEST}");
    let e = load(&engine(), &m, &wasm("rust")).err().expect("must fail").to_string();
    assert!(e.contains("wasi:sockets/tcp") && e.contains("does not offer"), "{e}");
}

#[test]
fn component_with_wasi_imports_fails_to_load() {
    let e = load(&engine(), MANIFEST, &wasm("ts-sm-fullwasi")).err().expect("must fail").to_string();
    assert!(e.contains("wasi:"), "{e}");
}

#[test]
fn bad_manifests_rejected() {
    for origin in ["https://*.example.com", "https://1.2.3.4", "http://a.example.com", "https://a.example.com/path", "https://a.example.com/", "https://a.example.com."] {
        let m = format!("id=\"x\"\napi=\"bastion:plugin@0.1\"\n[net]\nfetch=[\"{origin}\"]\n");
        assert!(load(&engine(), &m, &wasm("rust")).is_err(), "{origin}");
    }
    let m = format!("{MANIFEST}extra = 1\n");
    assert!(load(&engine(), &m, &wasm("rust")).is_err());
}

#[test]
fn cold_start_timing() {
    for g in GUESTS {
        let w = wasm(g);
        let e = engine();
        let t = std::time::Instant::now();
        let mut l = load(&e, MANIFEST, &w).unwrap();
        let load_t = t.elapsed();
        let t = std::time::Instant::now();
        l.prepare("note", "x").unwrap().unwrap();
        // production path: compile once at install (precompile), deserialize on each start
        let pre = e.precompile_component(&w).unwrap();
        let t = std::time::Instant::now();
        let mut l2 = unsafe { wasm_host::load_precompiled(&e, MANIFEST, &pre) }.unwrap();
        let pre_t = t.elapsed();
        l2.prepare("note", "x").unwrap().unwrap();
        println!("TIMING {g}: precompiled deserialize+instantiate {pre_t:?}");
        println!("TIMING {g}: size {} B, compile+instantiate {load_t:?}, first call {:?}", w.len(), t.elapsed());
    }
}
