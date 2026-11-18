//! MCP's Streamable HTTP transport, reduced to what a stateless server needs: POST a message, get JSON back.
//!
//! Listens on loopback only and rejects requests whose Host or Origin isn't local, so web pages can't reach it
//! through DNS rebinding. There is no other authentication. Parsing is strict: anything malformed gets 400, a
//! request that is too big gets 413, and no length, however odd, can crash it.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::Server;

/// The largest request accepted, head and body together.
pub const MAX_REQUEST: usize = 4 * 1024 * 1024;
/// The largest request head accepted.
pub const MAX_HEAD: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Without any query string.
    pub path: String,
    /// Names lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// What the bytes received on a connection so far amount to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parse {
    /// More bytes are needed.
    Incomplete,
    Complete(Request),
    /// No more bytes can make this a request the server understands.
    Invalid(String),
    TooLarge,
}

/// Parses one request from the start of `data`.
pub fn parse(data: &[u8]) -> Parse {
    let Some(head_end) = data.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if data.len() > MAX_HEAD {
            Parse::TooLarge
        } else {
            Parse::Incomplete
        };
    };
    if head_end > MAX_HEAD {
        return Parse::TooLarge;
    }
    let Ok(head) = std::str::from_utf8(&data[..head_end]) else {
        return Parse::Invalid("The request head isn't UTF-8.".into());
    };
    let mut lines = head.split("\r\n");
    let request_line: Vec<&str> = lines.next().unwrap_or_default().split(' ').collect();
    let [method, target, version] = request_line[..] else {
        return Parse::Invalid("Malformed request line.".into());
    };
    let is_token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    if !is_token(method)
        || target.is_empty()
        || !version.starts_with("HTTP/1.")
        || version.len() != 8
    {
        return Parse::Invalid("Malformed request line.".into());
    }

    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Parse::Invalid("Malformed header line.".into());
        };
        if !is_token(name) {
            return Parse::Invalid("Malformed header line.".into());
        }
        headers.push((
            name.to_ascii_lowercase(),
            value.trim_matches([' ', '\t']).to_owned(),
        ));
    }

    // Only Content-Length bodies are supported, so a chunked one would be misread.
    if headers.iter().any(|(n, _)| n == "transfer-encoding") {
        return Parse::Invalid(
            "Transfer-Encoding isn't supported; send a Content-Length instead.".into(),
        );
    }
    let lengths: Vec<&str> = headers
        .iter()
        .filter(|(n, _)| n == "content-length")
        .map(|(_, v)| v.as_str())
        .collect();
    let length = match lengths.as_slice() {
        [] => 0,
        [first, rest @ ..] if rest.iter().all(|v| v == first) => {
            if first.is_empty() || !first.bytes().all(|b| b.is_ascii_digit()) {
                return Parse::Invalid("Invalid Content-Length.".into());
            }
            match first.parse::<usize>() {
                Ok(n) if n <= MAX_REQUEST => n,
                _ => return Parse::TooLarge,
            }
        }
        _ => return Parse::Invalid("Conflicting Content-Length headers.".into()),
    };

    let body_start = head_end + 4;
    if body_start.saturating_add(length) > MAX_REQUEST {
        return Parse::TooLarge;
    }
    if data.len() - body_start < length {
        return Parse::Incomplete;
    }
    Parse::Complete(Request {
        method: method.to_owned(),
        path: target.split('?').next().unwrap_or_default().to_owned(),
        headers,
        body: data[body_start..body_start + length].to_vec(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(status: u16, body: &str) -> Self {
        Response {
            status,
            headers: vec![("Content-Type", "text/plain; charset=utf-8".into())],
            body: body.into(),
        }
    }

    pub fn serialized(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            202 => "Accepted",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Content Too Large",
            _ => "Error",
        };
        let mut head = format!("HTTP/1.1 {} {reason}\r\n", self.status);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.body.len()
        ));
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

/// The answer to one complete request.
pub fn respond(server: &Server, request: &Request) -> Response {
    let origin_ok = request.header("origin").is_none_or(is_local);
    if !request.header("host").is_some_and(is_local) || !origin_ok {
        return Response::text(403, "Only local requests are accepted.");
    }
    if request.path != "/mcp" {
        return Response::text(404, "MCP is served at /mcp.");
    }
    if request.method != "POST" {
        // No server-initiated stream and no sessions to end.
        let mut response = Response::text(405, "Only POST is supported.");
        response.headers.push(("Allow", "POST".into()));
        return response;
    }
    match server.handle_bytes(&request.body) {
        Some(reply) => Response {
            status: 200,
            headers: vec![("Content-Type", "application/json".into())],
            body: serde_json::to_vec(&reply).unwrap_or_default(),
        },
        None => Response {
            status: 202,
            headers: Vec::new(),
            body: Vec::new(),
        },
    }
}

/// `localhost`, `127.0.0.1` or `[::1]`, with or without a scheme and port.
pub fn is_local(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let host = value
        .split_once("://")
        .map_or(value.as_str(), |(_, rest)| rest);
    let host = host.split('/').next().unwrap_or_default();
    let host = if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default()
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Reads one request from `stream`, answers it and closes the connection.
pub fn serve_connection(server: &Server, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let response = loop {
        match parse(&buffer) {
            Parse::Complete(request) => break respond(server, &request),
            Parse::Invalid(reason) => break Response::text(400, &reason),
            Parse::TooLarge => break Response::text(413, "The request is too large."),
            Parse::Incomplete => match stream.read(&mut chunk) {
                // The client stopped before finishing a request, so there is no one to answer.
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            },
        }
    };
    let _ = stream.write_all(&response.serialized());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// Listens on `127.0.0.1:port` and serves each connection on its own thread. `on_ready` gets the bound address.
pub fn listen(
    server: Arc<Server>,
    port: u16,
    on_ready: impl FnOnce(SocketAddr),
) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    on_ready(listener.local_addr()?);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || serve_connection(&server, stream));
    }
    Ok(())
}
