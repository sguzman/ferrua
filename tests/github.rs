use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

#[test]
fn github_repository_metadata_works_with_local_fixture() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let size = stream.read(&mut request).unwrap();
        let input = String::from_utf8_lossy(&request[..size]);
        assert!(input.starts_with("GET /repos/fictional/demo "), "{input}");
        assert!(
            input.to_ascii_lowercase().contains("authorization: bearer fixture-ci-token"),
            "{input}"
        );
        let body = r#"{"name":"demo","full_name":"fictional/demo"}"#;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body.as_bytes()).unwrap();
    });
    let lua = format!(
        "local r,e=github.repository('fictional','demo',{{base_url='http://{address}'}}); assert(r,e); assert(r.full_name=='fictional/demo'); print('GitHub fixture passed')"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &lua])
        .env("GITHUB_TOKEN", "fixture-ci-token")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "GitHub fixture passed");
}

#[test]
fn github_helper_rejects_absolute_injected_hosts_and_missing_sandbox() {
    let out = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args([
            "-e",
            "assert(not pcall(github.request,'GET','//evil.test/path')); assert(not pcall(github.request,'GET','https://evil.test/x')); assert(not pcall(github.request,'GET','/../secret')); print('path checks passed')",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}
