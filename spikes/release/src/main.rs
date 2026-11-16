//! gboost-release (spike S11): release manifests and their signatures (minisign, Ed25519).
//!
//!   gboost-release keygen <dir> [--unencrypted]        new key pair: <dir>/release.key, <dir>/release.pub
//!   gboost-release manifest <files dir> <version> <schema> <min client>   writes <files dir>/manifest.json
//!   gboost-release sign <manifest> <secret key> [--password-stdin]   writes <manifest>.minisig (asks for the
//!                                                      key password, or reads one line from stdin)
//!   gboost-release verify <manifest> <public key>      checks <manifest>.minisig; exit 0 only if valid
//!   gboost-release hash <file>                         SHA-256, hex
//!
//! The maintainer signs on their own computer; the key never goes to CI (plan P10, 6.7). Nodes and the
//! client run `verify` from the version they already trust.
use minisign::{KeyPair, PublicKey, SecretKeyBox, SignatureBox};
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor, path::Path, process::exit};

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(2)
}

fn sha256_file(p: &Path) -> String {
    let data = fs::read(p).unwrap_or_else(|e| die(&format!("read {}: {e}", p.display())));
    Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect()
}

fn write_private(p: &Path, s: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(p)
            .unwrap_or_else(|e| die(&format!("create {}: {e}", p.display())));
        f.write_all(s.as_bytes()).unwrap_or_else(|e| die(&e.to_string()));
    }
    #[cfg(not(unix))]
    {
        if p.exists() {
            die(&format!("{} exists", p.display()));
        }
        fs::write(p, s).unwrap_or_else(|e| die(&e.to_string()));
    }
}

fn version_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 6 && p.chars().all(|c| c.is_ascii_digit()))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("keygen") if a.len() >= 3 => {
            let dir = Path::new(&a[2]);
            fs::create_dir_all(dir).unwrap_or_else(|e| die(&e.to_string()));
            let kp = if a.get(3).map(String::as_str) == Some("--unencrypted") {
                KeyPair::generate_unencrypted_keypair()
            } else {
                KeyPair::generate_encrypted_keypair(None) // asks for a password twice
            }
            .unwrap_or_else(|e| die(&e.to_string()));
            let sk = kp.sk.to_box(Some("gboost release key")).unwrap_or_else(|e| die(&e.to_string())).into_string();
            let pk = kp.pk.to_box().unwrap_or_else(|e| die(&e.to_string())).into_string();
            write_private(&dir.join("release.key"), &sk);
            fs::write(dir.join("release.pub"), pk).unwrap_or_else(|e| die(&e.to_string()));
            println!("{}", dir.join("release.pub").display());
        }
        Some("manifest") if a.len() == 6 => {
            let dir = Path::new(&a[2]);
            let (version, schema, min_client) = (&a[3], &a[4], &a[5]);
            if !version_ok(version) || !version_ok(min_client) {
                die("versions must look like 1.2.3");
            }
            let schema: u32 = schema.parse().unwrap_or_else(|_| die("schema must be a number"));
            let mut files = serde_json::Map::new();
            let mut names: Vec<String> = fs::read_dir(dir).unwrap_or_else(|e| die(&e.to_string()))
                .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".tar.gz") || n.ends_with(".zip")).collect();
            names.sort();
            if names.is_empty() {
                die("no .tar.gz or .zip files");
            }
            for n in names {
                files.insert(n.clone(), serde_json::json!({"sha256": sha256_file(&dir.join(&n))}));
            }
            let m = serde_json::json!({"product": "gboost", "version": version, "schema": schema,
                                       "min_client": min_client, "files": files});
            let out = dir.join("manifest.json");
            fs::write(&out, serde_json::to_string_pretty(&m).unwrap() + "\n").unwrap_or_else(|e| die(&e.to_string()));
            println!("{}", out.display());
        }
        Some("sign") if a.len() == 4 || (a.len() == 5 && a[4] == "--password-stdin") => {
            let manifest = Path::new(&a[2]);
            let data = fs::read(manifest).unwrap_or_else(|e| die(&e.to_string()));
            let m: serde_json::Value = serde_json::from_slice(&data).unwrap_or_else(|_| die("manifest is not JSON"));
            let version = m["version"].as_str().unwrap_or_else(|| die("manifest has no version"));
            let sk_s = fs::read_to_string(&a[3]).unwrap_or_else(|e| die(&e.to_string()));
            // An unencrypted key (tests only) loads directly; an encrypted one asks for its password.
            // With --password-stdin the password comes as one line on stdin (from a password store, never argv).
            let password = (a.len() == 5).then(|| {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).unwrap_or_else(|e| die(&e.to_string()));
                line.trim_end_matches(['\r', '\n']).to_string()
            });
            let sk = SecretKeyBox::from_string(&sk_s).and_then(|b| b.into_unencrypted_secret_key())
                .or_else(|_| SecretKeyBox::from_string(&sk_s).and_then(|b| b.into_secret_key(password)))
                .unwrap_or_else(|_| die("could not open the secret key (wrong password?)"));
            let pk = PublicKey::from_secret_key(&sk).ok();
            let comment = format!("gboost release {version}");
            let sig = minisign::sign(pk.as_ref(), &sk, Cursor::new(&data), Some(&comment), None)
                .unwrap_or_else(|e| die(&e.to_string()));
            let out = format!("{}.minisig", manifest.display());
            fs::write(&out, sig.into_string()).unwrap_or_else(|e| die(&e.to_string()));
            println!("{out}");
        }
        Some("verify") if a.len() == 4 => {
            let manifest = Path::new(&a[2]);
            let data = fs::read(manifest).unwrap_or_else(|e| die(&e.to_string()));
            let sig_s = fs::read_to_string(format!("{}.minisig", manifest.display())).unwrap_or_else(|_| die("no signature"));
            let pk = PublicKey::from_file(&a[3]).unwrap_or_else(|e| die(&e.to_string()));
            let sig = SignatureBox::from_string(&sig_s).unwrap_or_else(|_| die("bad signature file"));
            if minisign::verify(&pk, &sig, Cursor::new(&data), true, false, false).is_err() {
                die("signature does not match");
            }
            // The trusted comment is signed too; it must name the manifest's own version.
            let m: serde_json::Value = serde_json::from_slice(&data).unwrap_or_else(|_| die("manifest is not JSON"));
            let want = format!("gboost release {}", m["version"].as_str().unwrap_or(""));
            if sig.trusted_comment().ok().as_deref() != Some(want.as_str()) {
                die("signed comment does not match the manifest version");
            }
            println!("OK {}", m["version"].as_str().unwrap_or(""));
        }
        Some("hash") if a.len() == 3 => println!("{}", sha256_file(Path::new(&a[2]))),
        _ => die("usage: gboost-release keygen|manifest|sign|verify|hash ... (see the source header)"),
    }
}
