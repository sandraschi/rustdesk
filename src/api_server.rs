/// rustdesk++ API server — lightweight HTTP REST API.
/// Started via `rustdesk --api-server <port>`.

use hbb_common::{
    config::Config,
    log,
    protobuf::Message as _,
    rendezvous_proto::*,
    socket_client,
    tokio,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

pub fn start_api_server(port: u16) {
    let addr = format!("127.0.0.1:{}", port);
    log::info!("Starting API server on {}", addr);
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("Failed to bind {}: {}", addr, e);
        std::process::exit(1);
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => { std::thread::spawn(|| handle_client(stream)); }
            Err(e) => log::error!("Connection error: {}", e),
        }
    }
}

fn handle_client(mut stream: TcpStream) {
    let mut buf = [0u8; 8192];
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
    let body = extract_body(request);

    match (method, path) {
        ("GET", "/api/v1/health") => {
            json(200, r#"{"status":"ok","server":"rustdesk++","version":"1.4.9-pp"}"#)
        }

        ("GET", p) if p.starts_with("/api/v1/peers") => {
            handle_peers()
        }

        ("GET", p) if p.starts_with("/api/v1/peer/") => {
            let peer_id = p.strip_prefix("/api/v1/peer/").and_then(|s| s.split('/').next()).unwrap_or("");
            handle_peer_status(peer_id)
        }

        ("POST", "/api/v1/file/upload") => {
            handle_file_upload(body)
        }

        ("POST", "/api/v1/file/download") => {
            handle_file_download(body)
        }

        ("POST", "/api/v1/exec") => {
            handle_exec(body)
        }

        _ => json(404, r#"{"error":"not_found"}"#),
    }
}

fn handle_peers() -> Vec<u8> {
    // Peer list requires SQLite access via hbbs DB tools. Use the rustdesk-mcp
    // webapp dashboard for a full peer overview, or run a separate query tool.
    json(200, r#"{"peers":[],"note":"Use hbbs DB directly or rustdesk-mcp webapp for peer list"}"#)
}

fn handle_peer_status(peer_id: &str) -> Vec<u8> {
    if peer_id.is_empty() {
        return json(400, r#"{"error":"peer_id required"}"#);
    }
    let rt = hbb_common::tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        let addr = crate::file_cli::get_rendezvous_addr();
        let mut stream = match socket_client::connect_tcp(addr, 10000).await {
            Ok(s) => s,
            Err(e) => return format!(r#"{{"peer_id":"{}","error":"connect: {}"}}"#, peer_id, e),
        };
        let licence_key = crate::get_key(true).await;
        let mut msg = RendezvousMessage::new();
        msg.set_punch_hole_request(PunchHoleRequest {
            id: peer_id.to_owned(),
            licence_key,
            conn_type: ConnType::FILE_TRANSFER.into(),
            version: crate::VERSION.to_owned(),
            nat_type: hbb_common::rendezvous_proto::NatType::UNKNOWN_NAT.into(),
            ..Default::default()
        });
        if stream.send(&msg).await.is_err() {
            return format!(r#"{{"peer_id":"{}","error":"send failed"}}"#, peer_id);
        }
        match crate::get_next_nonkeyexchange_msg(&mut stream, Some(10000)).await {
            Some(resp) => {
                if resp.has_punch_hole_response() {
                    let phr = resp.punch_hole_response();
                    let failure = phr.failure.enum_value().unwrap_or(punch_hole_response::Failure::ID_NOT_EXIST);
                    let status = match failure {
                        punch_hole_response::Failure::OFFLINE => "offline",
                        punch_hole_response::Failure::ID_NOT_EXIST => "not_found",
                        punch_hole_response::Failure::LICENSE_MISMATCH => "key_mismatch",
                        _ => "unknown",
                    };
                    if !phr.relay_server.is_empty() || !phr.socket_addr.is_empty() {
                        return format!(r#"{{"peer_id":"{}","status":"online","relay":{}}}"#, peer_id,
                            if !phr.relay_server.is_empty() { r#"true"# } else { r#"false"# });
                    }
                    format!(r#"{{"peer_id":"{}","status":"{}"}}"#, peer_id, status)
                } else if resp.has_relay_response() {
                    format!(r#"{{"peer_id":"{}","status":"online","relay":true}}"#, peer_id)
                } else {
                    format!(r#"{{"peer_id":"{}","status":"unknown"}}"#, peer_id)
                }
            }
            None => format!(r#"{{"peer_id":"{}","error":"timeout"}}"#, peer_id),
        }
    });
    json(200, &result)
}

fn handle_file_upload(body: &str) -> Vec<u8> {
    let (peer_id, local_path, remote_path, password) = parse_file_req(body);
    if peer_id.is_empty() || local_path.is_empty() || remote_path.is_empty() {
        return json(400, r#"{"error":"peer_id, local_path, remote_path required"}"#);
    }
    let peer_id = peer_id.to_owned();
    let local_path = local_path.to_owned();
    let remote_path = remote_path.to_owned();
    let password = password.to_owned();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(crate::file_cli::send_file(&peer_id, &local_path, &remote_path, &password)) {
            Ok(()) => log::info!("API upload: sent to {}: {}", peer_id, local_path),
            Err(e) => log::error!("API upload failed to {}: {}", peer_id, e),
        }
    });
    json(200, r#"{"success":true,"message":"upload started"}"#)
}

fn handle_file_download(body: &str) -> Vec<u8> {
    let (peer_id, remote_path, local_path, password) = parse_file_req(body);
    if peer_id.is_empty() || remote_path.is_empty() || local_path.is_empty() {
        return json(400, r#"{"error":"peer_id, remote_path, local_path required"}"#);
    }
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(crate::file_cli::recv_file(&peer_id, &remote_path, &local_path, &password)) {
            Ok(()) => log::info!("API download: received from {}: {}", peer_id, remote_path),
            Err(e) => log::error!("API download failed from {}: {}", peer_id, e),
        }
    });
    json(200, r#"{"success":true,"message":"download started"}"#)
}

fn handle_exec(body: &str) -> Vec<u8> {
    // POST body: {"peer_id":"...", "script":"...", "script_type":"ps1|py|bat"}
    // Sends script to peer, runs it, returns stdout
    // Uses two-step: send script file → recv result file
    json(501, r#"{"error":"not_implemented","message":"Exec via relay: send script then recv result. Use --send-file + --recv-file in sequence."}"#)
}

fn parse_file_req(body: &str) -> (String, String, String, String) {
    // Simple JSON parse without serde dependency
    let peer_id = extract_json_str(body, "peer_id").unwrap_or_default();
    let local = extract_json_str(body, "local_path").or_else(|| extract_json_str(body, "local")).unwrap_or_default();
    let remote = extract_json_str(body, "remote_path").or_else(|| extract_json_str(body, "remote")).unwrap_or_default();
    let password = extract_json_str(body, "password").unwrap_or_default();
    (peer_id, local, remote, password)
}

fn extract_json_str(body: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}":"#, key);
    let start = body.find(&pattern)?;
    let start = start + pattern.len();
    let bytes = body[start..].as_bytes();
    if bytes.first()? == &b'"' {
        // string value
        let mut end = start + 1;
        while end < body.len() && bytes[end - start] != b'"' {
            end += 1;
        }
        Some(body[start + 1..end].to_owned())
    } else {
        // number or bool
        let end = start + bytes.iter().take_while(|&&b| b != b',' && b != b'}' && b != b' ').count();
        Some(body[start..start + end].to_owned())
    }
}

fn json(status: u16, body: &str) -> Vec<u8> {
    let reason = match status { 200 => "OK", 400 => "Bad Request", 404 => "Not Found", 501 => "Not Implemented", _ => "Error" };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).into_bytes()
}

fn parse_request_line(request: &str) -> (&str, &str) {
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 { (parts[0], parts[1]) } else { ("", "") }
}

fn extract_body(request: &str) -> &str {
    request.split("\r\n\r\n").nth(1).unwrap_or("")
}
