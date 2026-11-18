use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use jiff::civil::date;
use ostracon_core::Repository;
use ostracon_mcp::Server;
use ostracon_mcp::http::{self, MAX_REQUEST, Parse, parse};
use serde_json::{Value, json};

fn server() -> Server {
    Server::with_clock(Repository::in_memory().unwrap(), || {
        date(2026, 1, 14)
            .at(10, 30, 0, 0)
            .in_tz("Europe/London")
            .unwrap()
    })
}

fn rpc(server: &Server, method: &str, params: Value) -> Value {
    server
        .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
        .unwrap()
}

#[test]
fn initialize_negotiates_the_protocol_version() {
    let server = server();
    let reply = rpc(
        &server,
        "initialize",
        json!({"protocolVersion": "2025-06-18"}),
    );
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply["result"]["serverInfo"]["name"], "ostracon");
    let reply = rpc(
        &server,
        "initialize",
        json!({"protocolVersion": "1999-01-01"}),
    );
    assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn notifications_get_no_reply() {
    let server = server();
    assert_eq!(
        server.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        None
    );
}

#[test]
fn lists_and_calls_tools() {
    let server = server();
    let tools = rpc(&server, "tools/list", json!({}));
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 10);

    let added = rpc(
        &server,
        "tools/call",
        json!({"name": "add_task", "arguments": {"name": "Pay rent tomorrow"}}),
    );
    assert_eq!(added["result"]["isError"], false);
    assert_eq!(added["result"]["structuredContent"]["due"], "2026-01-15");
    let text: Value =
        serde_json::from_str(added["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["name"], "Pay rent");

    let failed = rpc(
        &server,
        "tools/call",
        json!({"name": "get_task", "arguments": {"id": 42}}),
    );
    assert_eq!(
        failed["result"]["isError"], true,
        "tool failures are results the model can read"
    );
    assert_eq!(
        failed["result"]["content"][0]["text"],
        "No task with ID 42."
    );

    let unknown = rpc(&server, "tools/call", json!({"name": "rm_rf"}));
    assert_eq!(unknown["error"]["code"], -32602);
}

#[test]
fn json_rpc_errors() {
    let server = server();
    assert_eq!(
        server.handle_bytes(b"{not json").unwrap()["error"]["code"],
        -32700
    );
    assert_eq!(
        rpc(&server, "frobnicate", json!({}))["error"]["code"],
        -32601
    );
    assert_eq!(server.handle(&json!([])).unwrap()["error"]["code"], -32600);
    assert_eq!(
        server.handle(&json!({"id": 3})).unwrap()["error"]["code"],
        -32600
    );
    let batch = server
        .handle(&json!([
            {"jsonrpc": "2.0", "id": 1, "method": "ping"},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "ping"},
        ]))
        .unwrap();
    assert_eq!(batch.as_array().unwrap().len(), 2);
}

fn request(body: &str, extra: &str) -> Vec<u8> {
    format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[test]
fn http_parsing_is_strict() {
    assert!(matches!(parse(&request("{}", "")), Parse::Complete(r) if r.body == b"{}"));
    assert_eq!(
        parse(b"POST /mcp HTTP/1.1\r\nHost: x\r\n"),
        Parse::Incomplete
    );
    assert_eq!(
        parse(b"POST /mcp HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc"),
        Parse::Incomplete
    );
    let bad = |bytes: &[u8]| matches!(parse(bytes), Parse::Invalid(_));
    assert!(bad(b"GARBAGE\r\n\r\n"));
    assert!(bad(b"POST /mcp HTTP/1.1 extra\r\n\r\n"));
    assert!(bad(b"POST /mcp HTTP/1.1\r\nno colon here\r\n\r\n"));
    assert!(bad(b"POST /mcp HTTP/1.1\r\nContent-Length: -1\r\n\r\n"));
    assert!(bad(b"POST /mcp HTTP/1.1\r\nContent-Length: +5\r\n\r\n"));
    assert!(bad(b"POST /mcp HTTP/1.1\r\nContent-Length: \r\n\r\n"));
    assert!(bad(
        b"POST /mcp HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n"
    ));
    assert!(bad(
        b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"
    ));
    assert!(bad(b"POST /mcp HTTP/1.1\r\n\xff: x\r\n\r\n"));
    let huge = b"POST /mcp HTTP/1.1\r\nContent-Length: 99999999999999999999999999\r\n\r\n";
    assert_eq!(
        parse(huge),
        Parse::TooLarge,
        "an absurd length is refused, not a crash"
    );
    let big = format!(
        "POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        MAX_REQUEST + 1
    );
    assert_eq!(parse(big.as_bytes()), Parse::TooLarge);
    assert_eq!(
        parse(&vec![b'a'; 70 * 1024]),
        Parse::TooLarge,
        "a head that never ends"
    );
}

#[test]
fn only_local_hosts_and_origins() {
    assert!(http::is_local("127.0.0.1:8765"));
    assert!(http::is_local("localhost"));
    assert!(http::is_local("[::1]:80"));
    assert!(http::is_local("http://localhost:3000"));
    assert!(!http::is_local("evil.com"));
    assert!(!http::is_local("127.0.0.1.evil.com"));
    assert!(!http::is_local("http://localhost.evil.com"));
    let server = server();
    let respond = |bytes: Vec<u8>| match parse(&bytes) {
        Parse::Complete(request) => http::respond(&server, &request).status,
        other => panic!("{other:?}"),
    };
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    assert_eq!(respond(request(ping, "")), 200);
    assert_eq!(respond(request(ping, "Origin: https://evil.com\r\n")), 403);
    assert_eq!(
        respond(request(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ""
        )),
        202
    );
    assert_eq!(
        respond(b"POST /mcp HTTP/1.1\r\nHost: evil.com\r\n\r\n".to_vec()),
        403
    );
    assert_eq!(
        respond(b"GET /mcp HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec()),
        405
    );
    assert_eq!(
        respond(b"POST /other HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec()),
        404
    );
}

#[test]
fn serves_over_loopback() {
    let server = Arc::new(server());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || http::listen(server, 0, |addr| tx.send(addr).unwrap()));
    let addr = rx.recv().unwrap();
    let exchange = |bytes: &[u8]| {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(bytes).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    };
    let reply = exchange(&request(
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#,
        "",
    ));
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(reply.contains("list_projects"));
    let reply = exchange(b"POST /mcp HTTP/1.1\r\nContent-Length: nope\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    let reply = exchange(
        format!(
            "POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST * 2
        )
        .as_bytes(),
    );
    assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
}
