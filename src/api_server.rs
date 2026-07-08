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
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
        _ => {}
    }
}

fn handle_request(request: &str) -> Vec<u8> {
    let (method, path) = parse_request_line(request);

    match (method, path) {
        ("GET", "/api/v1/health") => {
            json_response(200, r#"{"status":"ok","server":"rustdesk++","version":"1.4.9-pp"}"#)
        }
        ("POST", "/api/v1/file/upload") => {
            json_response(200, r#"{"success":false,"error_type":"not_implemented","message":"File transfer via fork API requires the peer to be on the same self-hosted server. Use --send-file CLI flag instead.","suggestions":["Configure both machines to use the same hbbs/hbbr server","Run `rustdesk --send-file <peer_id> <local> <remote>` directly","Ensure the peer is online and connected to the same rendezvous server"]}"#)
        }
        ("POST", "/api/v1/file/download") => {
            json_response(200, r#"{"success":false,"error_type":"not_implemented","message":"Use --recv-file CLI flag"}"#)
        }
        _ => {
            json_response(404, r#"{"error":"not_found"}"#)
        }
    }
}

fn json_response(status: u16, body: &str) -> Vec<u8> {
    let reason = match status { 200 => "OK", 404 => "Not Found", _ => "Error" };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).into_bytes()
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
