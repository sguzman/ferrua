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
            input
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-ci-token"),
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
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "GitHub fixture passed"
    );
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
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn github_rest_json_write_with_explicit_headers_and_no_redirects() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let handler = thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut bytes = [0u8; 8192];
        let len = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..len]).to_ascii_lowercase();
        assert!(
            request.starts_with("post /repos/fictional/demo/issues "),
            "{request}"
        );
        assert!(
            request.contains("authorization: bearer fixture-ci-token"),
            "{request}"
        );
        assert!(
            request.contains("content-type: application/json"),
            "{request}"
        );
        assert!(request.contains("x-ferrua-test: yes"), "{request}");
        // No reliance on HTTP payload arriving in a single socket read.
        stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"number\":1}").unwrap();
    });
    let code = format!(
        "local data,err=github.request_json('POST','/repos/fictional/demo/issues',{{base_url='http://{address}',json_body={{title='Sample'}},headers={{['X-Ferrua-Test']='yes'}}}}); assert(data,err); assert(data.number==1); print('GitHub POST passed')"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &code])
        .env("GITHUB_TOKEN", "fixture-ci-token")
        .output()
        .unwrap();
    handler.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "GitHub POST passed"
    );
}

#[test]
fn authorization_is_not_forwarded_to_redirect_destinations() {
    let receiver = TcpListener::bind("127.0.0.1:0").unwrap();
    receiver.set_nonblocking(true).unwrap();
    let destination = receiver.local_addr().unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let original = source.local_addr().unwrap();
    let handler = thread::spawn(move || {
        let (mut stream, _) = source.accept().unwrap();
        let mut buf = [0u8; 2048];
        let len = stream.read(&mut buf).unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..len])
                .to_lowercase()
                .contains("authorization: bearer fixture-ci-token")
        );
        let reply = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{destination}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(reply.as_bytes()).unwrap();
    });
    let code = format!(
        "local r,e=github.request('GET','/repos/fictional/demo',{{base_url='http://{original}'}}); assert(r,e); assert(r.status==302 and not r.ok); print('GitHub redirect blocked')"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &code])
        .env("GITHUB_TOKEN", "fixture-ci-token")
        .output()
        .unwrap();
    handler.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "GitHub redirect blocked"
    );
    assert!(
        receiver.accept().is_err(),
        "unexpected redirected connection"
    );
}
