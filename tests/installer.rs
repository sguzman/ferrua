//! Exercise the shell bootstrap installer using only local fixtures.
//! No Python, public network, preinstalled Lua, or writes to the user's HOME.
#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn scratch() -> PathBuf {
    let name = format!(
        "ferrua-install-{}-{:?}",
        std::process::id(),
        thread::current().id()
    );
    let path = std::env::temp_dir().join(name);
    fs::create_dir_all(&path).unwrap();
    path
}

fn serve_fixture(archive: Vec<u8>, checksum: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.contains(".sha256 ") {
                &checksum
            } else {
                assert!(line.contains("fer-v0.1.0-linux-x86_64.tar.gz "), "{line}");
                &archive
            };
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (url, server)
}

fn create_fixture(root: &Path, bad_checksum: bool) -> (Vec<u8>, Vec<u8>) {
    let source = root.join("contents");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("fer"),
        "#!/bin/sh\nprintf 'fer fixture works\\n'\n",
    )
    .unwrap();
    fs::write(source.join("README.md"), "Test fixture\n").unwrap();
    fs::write(source.join("LICENSE"), "Test fixture license\n").unwrap();
    let archive = "fer-v0.1.0-linux-x86_64.tar.gz";
    let status = Command::new("tar")
        .args([
            "-czf",
            archive,
            "-C",
            "contents",
            "fer",
            "README.md",
            "LICENSE",
        ])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
    let digest = Command::new("sha256sum")
        .arg(archive)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(digest.status.success());
    let mut checksum = digest.stdout;
    if bad_checksum {
        checksum[0] = if checksum[0] == b'0' { b'1' } else { b'0' };
    }
    (fs::read(root.join(archive)).unwrap(), checksum)
}

fn exercise(bad_checksum: bool) {
    let root = scratch();
    let (archive, checksum) = create_fixture(&root, bad_checksum);
    let expected = String::from_utf8_lossy(&checksum);
    let expected = expected.chars().take(64).collect::<String>();
    let (base, server) = serve_fixture(archive, checksum);
    let install = root.join("prefix");
    let run = Command::new("sh")
        .args([
            "scripts/install.sh",
            "v0.1.0",
            install.to_str().unwrap(),
            if bad_checksum { "" } else { &expected },
        ])
        .env("FERRUA_RELEASE_BASE", base)
        .output()
        .unwrap();
    server.join().unwrap();
    let destination = install.join("bin/fer");
    if bad_checksum {
        assert!(!run.status.success());
        assert!(!destination.exists(), "tampered archive was installed");
        assert!(String::from_utf8_lossy(&run.stderr).contains("SHA-256 mismatch"));
    } else {
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let output = Command::new(destination).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "fer fixture works\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn installs_verified_versioned_archive_from_local_http() {
    exercise(false);
}

#[test]
fn rejects_mismatched_digest_without_installing() {
    exercise(true);
}

#[test]
fn rejects_consistent_release_with_wrong_independent_digest() {
    let root = scratch();
    let (archive, checksum) = create_fixture(&root, false);
    let (base, server) = serve_fixture(archive, checksum);
    let prefix = root.join("pinned");
    let digest = "0".repeat(64);
    let response = Command::new("sh")
        .args(["scripts/install.sh", "v0.1.0", prefix.to_str().unwrap(), &digest])
        .env("FERRUA_RELEASE_BASE", base)
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!response.status.success());
    assert!(!prefix.join("bin/fer").exists());
    assert!(String::from_utf8_lossy(&response.stderr)
        .contains("independently pinned SHA-256 mismatch"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_nonsemantic_version_without_network_access() {
    let run = Command::new("sh")
        .args(["scripts/install.sh", "../latest"])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("expected explicit semver"));
}

#[test]
fn installed_real_archive_executes_outside_checkout() {
    // Set in the release-packaging CI step. This is an extra acceptance gate
    // for the compiled artifact, not just the tiny shell-script fixture above.
    let tarball = match std::env::var("FERRUA_RELEASE_TARBALL") {
        Ok(tarball) => tarball,
        Err(_) => return,
    };
    let root = scratch();
    let archive = fs::read(&tarball).expect("CI built a real release archive");
    let checksum =
        fs::read(format!("{tarball}.sha256")).expect("CI generated the real archive checksum");
    let (base, server) = serve_fixture(archive, checksum);
    let prefix = root.join("installed");
    let install = Command::new("sh")
        .args(["scripts/install.sh", "v0.1.0", prefix.to_str().unwrap()])
        .env("FERRUA_RELEASE_BASE", base)
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let binary = prefix.join("bin/fer");
    let script = root.join("independent.lua");
    fs::write(
        &script,
        r#"assert(json.decode('{"answer":42}').answer==42)
local result=process.check('printf', {'%s','independent'})
assert(result.stdout=='independent')
assert(fs.exists('independent.lua'))
print('standalone Ferrua archive accepted')
"#,
    )
    .unwrap();
    let result = Command::new(binary)
        .current_dir(&root)
        .arg("independent.lua")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "standalone Ferrua archive accepted"
    );
    fs::remove_dir_all(&root).unwrap();
}
