use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

#[test]
fn local_http_endpoint_is_accessible_from_fer_without_python() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP server");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept HTTP request");
        let mut request = [0_u8; 1024];
        let size = stream.read(&mut request).expect("read HTTP request");
        assert!(String::from_utf8_lossy(&request[..size]).starts_with("GET /status "));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}")
            .unwrap();
    });
    let lua = format!(
        "local r=http.check('GET', 'http://{address}/status'); assert(r.status == 200); assert(json.decode(r.body).ok); print('HTTP local fixture passed')"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &lua])
        .output()
        .expect("launch fer");
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "HTTP local fixture passed"
    );
}

#[test]
fn sandbox_excludes_http_bridge() {
    let result = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args([
            "--sandbox",
            "-e",
            "assert(http == nil and __fer_http_call == nil)",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn non_success_http_status_remains_a_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut incoming = [0_u8; 1024];
        let size = stream.read(&mut incoming).unwrap();
        assert!(String::from_utf8_lossy(&incoming[..size]).starts_with("GET /missing "));
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot-found").unwrap();
    });
    let lua = format!(
        "local r=http.get('http://{address}/missing'); assert(not r.ok and r.status == 404 and r.body == 'not-found'); print('404 response passed')"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &lua])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "404 response passed"
    );
}

#[test]
fn post_body_and_options_are_supported() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut incoming = [0_u8; 2048];
        let size = stream.read(&mut incoming).unwrap();
        let request = String::from_utf8_lossy(&incoming[..size]);
        assert!(request.starts_with("POST /submit "), "{request}");
        assert!(
            request.contains("x-ferrua-qa: yes") || request.contains("X-Ferrua-QA: yes"),
            "{request}"
        );
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .unwrap();
    });
    let lua = format!(
        "local options={{headers={{['X-Ferrua-QA']='yes'}}}}; local r=http.post('http://{address}/submit','data',options); assert(options.body == nil); assert(r.ok and r.status==201 and r.body == 'OK'); print('POST fixture passed')"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_fer"))
        .args(["-e", &lua])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "POST fixture passed"
    );
}
