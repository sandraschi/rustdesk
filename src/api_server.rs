/// rustdesk++ API server — lightweight HTTP + MCP JSON-RPC.
/// Started via `rustdesk --api-server <port>`.

use hbb_common::log;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn start_api_server(port: u16) {
    let addr = format!("127.0.0.1:{}", port);
    log::info!("Starting API server on {}", addr);
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("Failed to bind {}: {}", addr, e);
        std::process::exit(1);
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(|| handle_client(stream));
            }
            Err(e) => {
                log::error!("Connection error: {}", e);
            }
        }
    }
}

fn handle_client(mut stream: TcpStream) {
    let mut buf = [0u8; 4096];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
            let request = String::from_utf8_lossy(&buf[..n]);
            let response = handle_request(&request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
        _ => {}
    }
}

fn handle_request(request: &str) -> String {
    let (method, path) = parse_request_line(request);

    match (method, path) {
        ("GET", "/api/v1/health") => {
            r#"HTTP/1.1 200 OK
Content-Type: application/json

{"status":"ok","server":"rustdesk++","version":"1.4.9-pp"}"#.to_string()
        }
        ("POST", "/api/v1/file/upload") => {
            // Match the file upload endpoint
            let body = extract_body(request);
            r#"HTTP/1.1 200 OK
Content-Type: application/json

{"success":false,"error":"file_transfer","message":"File transfer via API server not yet implemented. Use the CLI --send-file flag."}"#.to_string()
        }
        _ => {
            r#"HTTP/1.1 404 Not Found
Content-Type: application/json

{"error":"not_found"}"#.to_string()
        }
    }
}

fn parse_request_line(request: &str) -> (&str, &str) {
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        (parts[0], parts[1])
    } else {
        ("", "")
    }
}

fn extract_body(request: &str) -> &str {
    request.split("\r\n\r\n").nth(1).unwrap_or("")
}
