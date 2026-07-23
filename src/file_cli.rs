/// rustdesk++ headless file transfer.
/// Uses rendezvous + relay protocol to transfer files without GUI.

use hbb_common::{
    config::{Config, RENDEZVOUS_PORT},
    fs::{self, TransferJob},
    log,
    message_proto::*,
    protobuf::{Enum as _, Message as _},
    rendezvous_proto::*,
    socket_client,
    tokio,
    Stream,
};

const TIMEOUT_SECS: u64 = 30;

/// After relay pairing, skip messages until LoginResponse.
/// Sends LoginRequest with SHA256 hashed password (matching server verify_h1).
async fn do_login(stream: &mut Stream, peer_id: &str, password: &str) -> Result<(), String> {
    use hbb_common::sha2::{Digest, Sha256};

    // Drain initial messages and extract Hash salt+challenge for password hashing
    let mut hash_salt = String::new();
    let mut hash_challenge = String::new();
    loop {
        let buf = match stream.next_timeout(2000).await {
            Some(Ok(b)) => b,
            _ => break,
        };
        if let Ok(m) = Message::parse_from_bytes(&buf) {
            if m.has_hash() {
                let h = m.hash();
                hash_salt = h.salt.clone();
                hash_challenge = h.challenge.clone();
                log::info!("do_login: got hash salt='{}' challenge='{}'", hash_salt, hash_challenge);
                continue;
            }
        }
        log::info!("do_login: draining {} bytes", buf.len());
    }

    // Compute h1 = SHA256(password + salt), then h2 = SHA256(h1 + challenge)
    let password_hashed = if !hash_challenge.is_empty() {
        let mut h1_hasher = Sha256::new();
        h1_hasher.update(password.as_bytes());
        h1_hasher.update(hash_salt.as_bytes());
        let h1 = h1_hasher.finalize();
        let mut h2_hasher = Sha256::new();
        h2_hasher.update(h1);
        h2_hasher.update(hash_challenge.as_bytes());
        Some(h2_hasher.finalize().to_vec())
    } else {
        None
    };

    // Try hashed password first, then raw password
    let attempts: Vec<Vec<u8>> = {
        let raw = password.as_bytes().to_vec();
        if let Some(hashed) = password_hashed {
            vec![hashed, raw]
        } else {
            vec![raw]
        }
    };

    for (i, pwd_bytes) in attempts.iter().enumerate() {
        let mut lr = LoginRequest::new();
        lr.username = peer_id.to_owned();
        lr.password = pwd_bytes.clone().into();
        lr.my_id = Config::get_id();
        lr.my_platform = "Windows".to_owned();
        lr.version = crate::VERSION.to_owned();
        lr.set_file_transfer(FileTransfer::new());
        let mut msg = Message::new();
        msg.set_login_request(lr);
        stream.send(&msg).await.map_err(|e| format!("login send: {}", e))?;

        for _ in 0..3 {
            let buf = match stream.next_timeout(15000).await {
                Some(Ok(b)) => b,
                Some(Err(e)) => return Err(format!("login recv: {}", e)),
                None => break,
            };
            let msg: Message = match Message::parse_from_bytes(&buf) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if msg.has_login_response() {
                let resp = msg.login_response();
                if resp.has_error() && !resp.error().is_empty() {
                    if i + 1 < attempts.len() {
                        log::info!("do_login: attempt {} failed ({}), retrying raw", i + 1, resp.error());
                    } else {
                        return Err(format!("login failed: {}", resp.error()));
                    }
                } else {
                    log::info!("do_login: login accepted");
                    return Ok(());
                }
            }
        }
    }
    Err("login: failed after all attempts".into())
}

pub async fn send_file(
    peer_id: &str,
    local_path: &str,
    remote_path: &str,
    password: &str,
) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let _path = std::path::Path::new(local_path);
    let file_name = fs::get_file_name(_path);
    let _file_size = std::fs::metadata(local_path)
        .map_err(|e| format!("cannot stat {}: {}", local_path, e))?
        .len();

    let job_id = fs::get_next_job_id();

    // Send the transfer request (remote path)
    let mut action = FileAction::new();
    action.set_send(FileTransferSendRequest {
        id: job_id,
        path: remote_path.to_string(),
        include_hidden: false,
        file_num: 1,
        file_type: file_transfer_send_request::FileType::Generic.into(),
        ..Default::default()
    });
    send_msg(&mut stream, action).await?;

    // Wait for digest/confirm response
    let _resp = recv_msg(&mut stream).await?;

    let remote = if remote_path.ends_with('/') || remote_path.ends_with('\\') {
        format!("{}{}", remote_path, file_name)
    } else {
        remote_path.to_string()
    };

    // Stream file blocks
    let mut job = TransferJob::new_read(
        job_id,
        fs::JobType::Generic,
        remote,
        fs::DataSource::FilePath(std::path::PathBuf::from(local_path)),
        1,
        false, false, false,
    ).map_err(|e| format!("job: {}", e))?;

    loop {
        let opt_block = job.read().await.map_err(|e| format!("read: {}", e))?;
        if let Some(block) = opt_block {
            let mut fr = FileResponse::new();
            fr.set_block(block);
            send_file_resp(&mut stream, fr).await?;
        } else {
            let mut fr = FileResponse::new();
            fr.set_done(FileTransferDone::new());
            send_file_resp(&mut stream, fr).await?;
            break;
        }
    }
    Ok(())
}

pub async fn recv_file(
    peer_id: &str,
    remote_path: &str,
    local_path: &str,
    password: &str,
) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;

    // Send receive request
    let mut action = FileAction::new();
    action.set_receive(FileTransferReceiveRequest {
        id: fs::get_next_job_id(),
        path: remote_path.to_string(),
        files: Vec::new(),
        file_num: 1,
        total_size: 0,
        ..Default::default()
    });
    send_msg(&mut stream, action).await?;

    let parent = std::path::Path::new(local_path).parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;

    let mut file = tokio::fs::File::create(local_path).await
        .map_err(|e| format!("create: {}", e))?;

    // The server sends: [digest?] → block × N → done
    loop {
        let buf = match stream.next_timeout(TIMEOUT_SECS * 1000).await {
            Some(Ok(b)) => b,
            _ => return Err("recv timeout".into()),
        };
        let msg: Message = match Message::parse_from_bytes(&buf) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if msg.has_file_response() {
            let resp = msg.file_response();
            if resp.has_digest() {
                log::info!("recv_file: digest (file_size={}), sending confirm", resp.digest().file_size);
                let mut confirm = FileResponse::new();
                confirm.set_digest(resp.digest().clone());
                send_file_resp(&mut stream, confirm).await?;
                continue;
            }
            if resp.has_block() {
                use tokio::io::AsyncWriteExt;
                file.write_all(&resp.block().data[..]).await
                    .map_err(|e| format!("write: {}", e))?;
                continue;
            }
            if resp.has_done() {
                log::info!("recv_file: done");
                break;
            }
            if resp.has_error() {
                return Err(format!("remote error: {}", resp.error().error));
            }
            log::info!("recv_file: unexpected FileResponse, waiting for blocks");
            continue;
        }
        // Skip handshake keepalive messages
        if msg.has_hash() || msg.has_test_delay() || msg.has_login_response() {
            continue;
        }
        log::info!("recv_file: skipping msg type ({} bytes)", buf.len());
    }
    Ok(())
}

pub async fn list_dir(peer_id: &str, remote_path: &str, password: &str) -> Result<Vec<String>, String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;

    let mut action = FileAction::new();
    action.set_read_dir(ReadDir {
        path: remote_path.to_string(),
        include_hidden: false,
        ..Default::default()
    });
    send_msg(&mut stream, action).await?;

    let resp = recv_msg(&mut stream).await?;
    if resp.has_dir() {
        let dir = resp.dir();
        Ok(dir.entries.iter().map(|e| {
                let kind = match e.entry_type.enum_value().unwrap_or(FileType::File) {
                    FileType::Dir | FileType::DirDrive => "DIR",
                    _ => "FILE",
                };
                format!("[{}] {} {}b", kind, e.name, e.size)
            }).collect())
    } else if resp.has_error() {
        Err(format!("remote: {}", resp.error().error))
    } else {
        Err("unexpected response".into())
    }
}

pub async fn delete_remote(peer_id: &str, remote_path: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut action = FileAction::new();
    action.set_remove_file(FileRemoveFile { path: remote_path.to_string(), ..Default::default() });
    send_msg(&mut stream, action).await?;
    // IPC CM processes asynchronously — brief wait for ack, then return
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

pub async fn move_remote(peer_id: &str, old_path: &str, new_path: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut action = FileAction::new();
    action.set_rename(FileRename {
        path: old_path.to_string(),
        new_name: new_path.to_string(),
        ..Default::default()
    });
    send_msg(&mut stream, action).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

pub async fn send_dir(peer_id: &str, local_dir: &str, remote_dir: &str, password: &str) -> Result<(), String> {
    let entries = std::fs::read_dir(local_dir)
        .map_err(|e| format!("read local dir {}: {}", local_dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("entry: {}", e))?;
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().unwrap().to_string_lossy();
            let remote = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
            let local = path.to_string_lossy().to_string();
            send_file(peer_id, &local, &remote, password).await?;
        }
    }
    Ok(())
}

/// Check if a peer is online. Connects to hbbs and sends a minimal probe.
pub async fn peer_info(peer_id: &str) -> Result<String, String> {
    let addr = get_rendezvous_addr();
    let mut stream = socket_client::connect_tcp(addr, 30000)
        .await.map_err(|e| format!("rendezvous: {}", e))?;
    let mut msg = RendezvousMessage::new();
    let licence_key = crate::get_key(true).await;
    msg.set_punch_hole_request(PunchHoleRequest {
        id: peer_id.to_owned(),
        licence_key,
        conn_type: ConnType::FILE_TRANSFER.into(),
        version: crate::VERSION.to_owned(),
        nat_type: hbb_common::rendezvous_proto::NatType::UNKNOWN_NAT.into(),
        ..Default::default()
    });
    stream.send(&msg).await.map_err(|e| format!("send: {}", e))?;
    if let Some(resp) = crate::get_next_nonkeyexchange_msg(&mut stream, Some(10000)).await {
        if resp.has_punch_hole_response() {
            use hbb_common::protobuf::Enum;
            let phr = resp.punch_hole_response();
            let failure = phr.failure.enum_value().unwrap_or(punch_hole_response::Failure::ID_NOT_EXIST);
            let msg_str = if phr.other_failure.is_empty() {
                match failure {
                    punch_hole_response::Failure::OFFLINE => format!("{}: offline", peer_id),
                    punch_hole_response::Failure::ID_NOT_EXIST => format!("{}: not found", peer_id),
                    punch_hole_response::Failure::LICENSE_MISMATCH => format!("{}: key mismatch", peer_id),
                    punch_hole_response::Failure::LICENSE_OVERUSE => format!("{}: key overuse", peer_id),
                    _ => format!("{}: failure code {}", peer_id, failure.value()),
                }
            } else {
                format!("{}: {}", peer_id, phr.other_failure)
            };
            if !phr.relay_server.is_empty() || !phr.socket_addr.is_empty() {
                return Ok(format!("{}: online via {}", peer_id, if !phr.relay_server.is_empty() { "relay" } else { "direct" }));
            }
            return Ok(msg_str);
        } else if resp.has_relay_response() {
            return Ok(format!("{}: online (relay ready)", peer_id));
        }
    }
    Err("peer_info: no response from server".into())
}

pub async fn create_remote_dir(peer_id: &str, remote_path: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut action = FileAction::new();
    action.set_create(FileDirCreate { path: remote_path.to_string(), ..Default::default() });
    send_msg(&mut stream, action).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

pub async fn remote_restart(peer_id: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut misc = Misc::new();
    misc.set_restart_remote_device(true);
    let mut msg = Message::new();
    msg.set_misc(misc);
    stream.send(&msg).await.map_err(|e| format!("restart send: {}", e))?;
    log::info!("Restart command sent to {}", peer_id);
    Ok(())
}

pub async fn remote_shutdown(peer_id: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut misc = Misc::new();
    misc.set_stop_service(true);
    let mut msg = Message::new();
    msg.set_misc(misc);
    stream.send(&msg).await.map_err(|e| format!("shutdown send: {}", e))?;
    log::info!("Shutdown command sent to {}", peer_id);
    Ok(())
}

pub async fn remote_screenshot(peer_id: &str, output_path: &str, password: &str) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
    do_login(&mut stream, peer_id, password).await?;
    let mut req = ScreenshotRequest::new();
    req.display = 0; // primary display
    let mut msg = Message::new();
    msg.set_screenshot_request(req);
    stream.send(&msg).await.map_err(|e| format!("screenshot send: {}", e))?;
    loop {
        let buf = match stream.next_timeout(TIMEOUT_SECS * 1000).await {
            Some(Ok(b)) => b,
            _ => return Err("screenshot: no response".into()),
        };
        let msg: Message = match Message::parse_from_bytes(&buf) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if msg.has_screenshot_response() {
            let resp = msg.screenshot_response();
            if !resp.data.is_empty() {
                use std::io::Write;
                let mut file = std::fs::File::create(output_path)
                    .map_err(|e| format!("create output: {}", e))?;
                file.write_all(&resp.data)
                    .map_err(|e| format!("write output: {}", e))?;
                log::info!("Screenshot saved to {}", output_path);
                return Ok(());
            }
            if !resp.msg.is_empty() {
                return Err(format!("screenshot error: {}", resp.msg));
            }
        } else if msg.has_hash() || msg.has_test_delay() || msg.has_login_response() {
            continue;
        }
    }
}

/// Send one file to multiple peers sequentially.
pub async fn broadcast_file(
    peer_ids: &str,
    local_path: &str,
    remote_path: &str,
    password: &str,
) -> Result<Vec<(String, String)>, String> {
    let ids: Vec<&str> = peer_ids.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if ids.is_empty() {
        return Err("no peer IDs provided".into());
    }
    let mut results = Vec::new();
    for id in &ids {
        match send_file(id, local_path, remote_path, password).await {
            Ok(()) => results.push((id.to_string(), "ok".to_string())),
            Err(e) => results.push((id.to_string(), format!("error: {}", e))),
        }
    }
    Ok(results)
}

/// Collect files from multiple peers by listing each remote dir and pulling files.
pub async fn collect_files(
    peer_ids: &str,
    remote_dir: &str,
    local_dir: &str,
    password: &str,
) -> Result<Vec<(String, String)>, String> {
    let ids: Vec<&str> = peer_ids.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if ids.is_empty() {
        return Err("no peer IDs provided".into());
    }
    std::fs::create_dir_all(local_dir).map_err(|e| format!("mkdir local: {}", e))?;

    let mut results = Vec::new();
    for id in &ids {
        let entries = list_dir(id, remote_dir, password).await?;
        for entry in &entries {
            if entry.starts_with("[FILE]") {
                let name = entry.split_whitespace().nth(1).unwrap_or("");
                if !name.is_empty() {
                    let rpath = format!("{}/{}", remote_dir.trim_end_matches('/'), name);
                    let lpath = format!("{}/{}", local_dir.trim_end_matches('/'), name);
                    match recv_file(id, &rpath, &lpath, password).await {
                        Ok(()) => results.push((format!("{}:{}", id, name), "ok".to_string())),
                        Err(e) => results.push((format!("{}:{}", id, name), format!("error: {}", e))),
                    }
                }
            }
        }
    }
    Ok(results)
}

/// Batch: read a JSON manifest and execute the sequence of operations.
///
/// Manifest format:
/// ```json
/// {
///   "password": "optional_default",
///   "operations": [
///     { "op": "send_file", "peer_id": "...", "local": "...", "remote": "..." },
///     { "op": "recv_file", "peer_id": "...", "remote": "...", "local": "..." },
///     ...
///   ]
/// }
/// ```
pub async fn batch_ops(manifest_path: &str, default_password: &str) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("read manifest '{}': {}", manifest_path, e))?;
    // Minimal JSON parsing without serde dependency -- same pattern as api_server.rs
    let password = extract_json_str(&content, "password").unwrap_or_else(|| default_password.to_owned());
    let ops_block = extract_json_array(&content, "operations").unwrap_or_default();

    if ops_block.is_empty() {
        return Err("no operations in manifest".into());
    }

    let mut results = Vec::new();
    // Parse each operation object from the array block
    // ops_block is the text between [ and ] for the operations array
    let mut depth = 0;
    let mut start = 0;
    let mut objs = Vec::new();
    for (i, byte) in ops_block.bytes().enumerate() {
        match byte {
            b'{' if depth == 0 => start = i,
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'}' if depth == 0 => {
                objs.push(&ops_block[start..=i]);
            }
            _ => {}
        }
    }

    for obj_str in &objs {
        let op = extract_json_str(obj_str, "op").unwrap_or_default();
        let peer_id = extract_json_str(obj_str, "peer_id").unwrap_or_default();
        let pwd = extract_json_str(obj_str, "password").unwrap_or_else(|| password.clone());

        let result = match op.as_str() {
            "send_file" => {
                let local = extract_json_str(obj_str, "local").unwrap_or_default();
                let remote = extract_json_str(obj_str, "remote").unwrap_or_default();
                send_file(&peer_id, &local, &remote, &pwd).await
                    .map(|_| format!("[{}] send_file {} -> {}: ok", peer_id, local, remote))
                    .unwrap_or_else(|e| format!("[{}] send_file: {}", peer_id, e))
            }
            "recv_file" => {
                let remote = extract_json_str(obj_str, "remote").unwrap_or_default();
                let local = extract_json_str(obj_str, "local").unwrap_or_default();
                recv_file(&peer_id, &remote, &local, &pwd).await
                    .map(|_| format!("[{}] recv_file {} <- {}: ok", peer_id, remote, local))
                    .unwrap_or_else(|e| format!("[{}] recv_file: {}", peer_id, e))
            }
            "list_dir" => {
                let path = extract_json_str(obj_str, "path").unwrap_or_default();
                match list_dir(&peer_id, &path, &pwd).await {
                    Ok(entries) => format!("[{}] list_dir {}: {} entries", peer_id, path, entries.len()),
                    Err(e) => format!("[{}] list_dir: {}", peer_id, e),
                }
            }
            "delete_remote" => {
                let path = extract_json_str(obj_str, "path").unwrap_or_default();
                delete_remote(&peer_id, &path, &pwd).await
                    .map(|_| format!("[{}] delete_remote {}: ok", peer_id, path))
                    .unwrap_or_else(|e| format!("[{}] delete_remote: {}", peer_id, e))
            }
            "move_remote" => {
                let old = extract_json_str(obj_str, "old").unwrap_or_default();
                let new = extract_json_str(obj_str, "new").unwrap_or_default();
                move_remote(&peer_id, &old, &new, &pwd).await
                    .map(|_| format!("[{}] move_remote {} -> {}: ok", peer_id, old, new))
                    .unwrap_or_else(|e| format!("[{}] move_remote: {}", peer_id, e))
            }
            "send_dir" => {
                let local = extract_json_str(obj_str, "local").unwrap_or_default();
                let remote = extract_json_str(obj_str, "remote").unwrap_or_default();
                send_dir(&peer_id, &local, &remote, &pwd).await
                    .map(|_| format!("[{}] send_dir {} -> {}: ok", peer_id, local, remote))
                    .unwrap_or_else(|e| format!("[{}] send_dir: {}", peer_id, e))
            }
            "create_dir" => {
                let path = extract_json_str(obj_str, "path").unwrap_or_default();
                create_remote_dir(&peer_id, &path, &pwd).await
                    .map(|_| format!("[{}] create_dir {}: ok", peer_id, path))
                    .unwrap_or_else(|e| format!("[{}] create_dir: {}", peer_id, e))
            }
            "restart" => {
                remote_restart(&peer_id, &pwd).await
                    .map(|_| format!("[{}] restart: ok", peer_id))
                    .unwrap_or_else(|e| format!("[{}] restart: {}", peer_id, e))
            }
            "shutdown" => {
                remote_shutdown(&peer_id, &pwd).await
                    .map(|_| format!("[{}] shutdown: ok", peer_id))
                    .unwrap_or_else(|e| format!("[{}] shutdown: {}", peer_id, e))
            }
            "screenshot" => {
                let output = extract_json_str(obj_str, "output").unwrap_or_else(|| "screenshot.png".to_owned());
                remote_screenshot(&peer_id, &output, &pwd).await
                    .map(|_| format!("[{}] screenshot -> {}: ok", peer_id, output))
                    .unwrap_or_else(|e| format!("[{}] screenshot: {}", peer_id, e))
            }
            "peer_info" => {
                peer_info(&peer_id).await
                    .unwrap_or_else(|e| format!("[{}] peer_info: {}", peer_id, e))
            }
            _ => format!("[{}] unknown op '{}'", peer_id, op),
        };
        results.push(result);
    }
    Ok(results)
}

fn extract_json_str<'a>(body: &'a str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}":"#, key);
    let start = body.find(&pattern)?;
    let start = start + pattern.len();
    let bytes = body[start..].as_bytes();
    if bytes.first()? == &b'"' {
        let mut end = start + 1;
        while end < body.len() && bytes[end - start] != b'"' {
            end += 1;
        }
        Some(body[start + 1..end].to_owned())
    } else {
        let end = start + bytes.iter().take_while(|&&b| b != b',' && b != b'}' && b != b' ').count();
        Some(body[start..start + end].to_owned())
    }
}

fn extract_json_array<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!(r#""{}":"#, key);
    let start = body.find(&pattern)? + pattern.len();
    let bytes = body[start..].as_bytes();
    let array_start = bytes.iter().position(|&b| b == b'[')? + start;
    let mut depth = 0;
    for (i, byte) in body[array_start..].bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&body[array_start..=array_start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Local RustDesk status: ID, service, connected peers.
pub fn local_status() -> Result<String, String> {
    let id = Config::get_id();
    let svc_running = std::process::Command::new("sc")
        .args(["query", "RustDesk"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false);
    let rd = Config::get_rendezvous_server();
    let rs = Config::get_option("relay-server");
    Ok(format!(
        "RustDesk++ Headless\n  ID: {}\n  Service: {}\n  Rendezvous: {}\n  Relay: {}\n  Version: {}",
        id, if svc_running { "running" } else { "stopped" }, rd, rs, crate::VERSION
    ))
}

pub fn get_rendezvous_addr() -> String {
    // connect_tcp has DNS resolution issues on this build.
    // Use hardcoded IP for known servers, fall back to config for custom.
    let server = Config::get_rendezvous_server();
    let custom = Config::get_option("custom-rendezvous-server");
    let target = if !custom.is_empty() { custom.as_str() } else { server.as_str() };

    match target {
        "rs-ny.rustdesk.com" | "" => "209.250.254.15:21116".to_string(),
        s if s.contains(':') => s.to_string(),
        s => format!("{}:{}", s, RENDEZVOUS_PORT),
    }
}

/// Resolve a hostname:port string to an IP:port string using OS DNS.
async fn resolve_addr(hostport: &str) -> Result<String, String> {
    use std::net::ToSocketAddrs;
    let mut addrs = hostport.to_socket_addrs()
        .map_err(|e| format!("dns resolve '{}': {}", hostport, e))?;
    let addr = addrs
        .next()
        .ok_or_else(|| format!("no address for '{}'", hostport))?;
    Ok(addr.to_string())
}

async fn establish_connection(peer_id: &str) -> Result<Stream, String> {
    let addr = get_rendezvous_addr();
    let mut stream = socket_client::connect_tcp(addr, 30000)
        .await
        .map_err(|e| format!("rendezvous: {}", e))?;

    // Try OAuth token (GUI stores it in RustDesk_local.toml)
    let access_token = hbb_common::config::LocalConfig::get_option("access_token");

    // Register PK — skip if already registered (UUID conflict with existing peer)
    // Single attempt, don't fail if server rejects (some hbbs builds don't support re-registration)
    let my_id = Config::get_id();
    let uuid_bytes: bytes::Bytes = hbb_common::get_uuid().into();
    let (_sk, pk) = Config::get_key_pair();
    let mut rp = RegisterPk::new();
    rp.id = my_id;
    rp.pk = pk.into();
    rp.uuid = uuid_bytes;
    let mut reg_msg = RendezvousMessage::new();
    reg_msg.set_register_pk(rp);
    if stream.send(&reg_msg).await.is_ok() {
        if let Some(reg_resp) = crate::get_next_nonkeyexchange_msg(&mut stream, Some(10000)).await {
            if reg_resp.has_register_pk_response() {
                let rpr = reg_resp.register_pk_response();
                if rpr.result.enum_value() == Ok(register_pk_response::Result::OK) {
                    log::info!("Register PK confirmed");
                } else {
                    log::info!("Register PK not needed, proceeding");
                }
            }
        }
    }

    // Now send PunchHoleRequest (matching client.rs::_start_inner)
    use hbb_common::protobuf::Enum;
    let mut msg = RendezvousMessage::new();
    let licence_key = crate::get_key(true).await;
    msg.set_punch_hole_request(PunchHoleRequest {
        id: peer_id.to_owned(),
        token: access_token,
        licence_key,
        conn_type: ConnType::FILE_TRANSFER.into(),
        version: crate::VERSION.to_owned(),
        nat_type: hbb_common::rendezvous_proto::NatType::UNKNOWN_NAT.into(),
        ..Default::default()
    });

    for i in 1..=3 {
        stream.send(&msg).await.map_err(|e| format!("punch #{}: {}", i, e))?;
        let timeout_secs = if i == 1 { 30 } else { i * 3 };
        log::info!("Punch attempt {} waiting {}s for response", i, timeout_secs);
        if let Some(resp) = crate::get_next_nonkeyexchange_msg(&mut stream, Some(timeout_secs * 1000)).await {
            log::info!("Received response type: punch_hole_response={} relay_response={} fetch_local_addr={}", resp.has_punch_hole_response(), resp.has_relay_response(), resp.has_fetch_local_addr());
            if resp.has_punch_hole_response() {
                let phr = resp.punch_hole_response();
                if !phr.relay_server.is_empty() {
                    let relay_addr = phr.relay_server.clone();
                    log::info!("relay_server='{}'", relay_addr);
                    let relay_target = if relay_addr.contains(':') {
                        relay_addr
                    } else {
                        format!("{}:21117", relay_addr)
                    };
                    // Resolve to SocketAddr via OS DNS, pass as resolved address
                    use std::net::ToSocketAddrs;
                    let socket_addrs: Vec<std::net::SocketAddr> = relay_target
                        .to_socket_addrs()
                        .map_err(|e| format!("relay dns: {}", e))?
                        .collect();
                    let remote_addr = *socket_addrs.first()
                        .ok_or_else(|| "no relay address".to_string())?;
                    let mut relay = socket_client::connect_tcp_local(
                        remote_addr, None, 30000
                    ).await.map_err(|e| format!("relay: {}", e))?;

                    let mut rr = RendezvousMessage::new();
                    rr.set_request_relay(RequestRelay {
                        id: peer_id.to_owned(),
        conn_type: ConnType::DEFAULT_CONN.into(),
                        ..Default::default()
                    });
                    relay.send(&rr).await.map_err(|e| format!("relay send: {}", e))?;

                    let _relay_resp = crate::get_next_nonkeyexchange_msg(&mut relay, Some(30000))
                        .await.ok_or("relay resp timeout")?;
                    return Ok(relay);

                } else if !phr.socket_addr.is_empty() {
                    let addr = hbb_common::AddrMangle::decode(&phr.socket_addr);
                    let p2p = socket_client::connect_tcp_local(addr.to_string(), None, 10000)
                        .await.map_err(|e| format!("p2p: {}", e))?;
                    return Ok(p2p);
                } else if !phr.other_failure.is_empty() {
                    return Err(phr.other_failure.clone());
                } else {
                    match phr.failure.enum_value() {
                        Ok(punch_hole_response::Failure::ID_NOT_EXIST) =>
                            return Err("ID does not exist".into()),
                        Ok(punch_hole_response::Failure::OFFLINE) =>
                            return Err("Remote desktop is offline".into()),
                        Ok(punch_hole_response::Failure::LICENSE_MISMATCH) =>
                            return Err("License key mismatch".into()),
                        Ok(punch_hole_response::Failure::LICENSE_OVERUSE) =>
                            return Err("License key overuse".into()),
                        _ => {
                            let v = phr.failure.value();
                            log::info!("Punch failure={} (no relay or direct addr)", v);
                        }
                    }
                }
            } else if resp.has_relay_response() {
                let relay_resp = resp.relay_response();
                let relay_str = relay_resp.relay_server.clone();
                let relay_uuid = relay_resp.uuid.clone();
                log::info!("relay_response server='{}'", relay_str);
                let relay_target = if relay_str.contains(':') { relay_str } else { format!("{}:21117", relay_str) };
                use std::net::ToSocketAddrs;
                let addrs: Vec<std::net::SocketAddr> = relay_target.to_socket_addrs()
                    .map_err(|e| format!("relay dns: {}", e))?.collect();
                let relay_addr = *addrs.first().ok_or("no relay addr")?;
                let mut relay = socket_client::connect_tcp_local(relay_addr, None, 30000)
                    .await.map_err(|e| format!("relay: {}", e))?;

                let mut rmsg = RendezvousMessage::new();
                rmsg.set_request_relay(RequestRelay {
                    id: peer_id.to_owned(),
        conn_type: ConnType::DEFAULT_CONN.into(),
                    uuid: relay_uuid.into(),
                    ..Default::default()
                });
                relay.send(&rmsg).await.map_err(|e| format!("relay send: {}", e))?;

                            // Read the raw pairing response. It could be a RendezvousMessage
                // (KeyExchange) or a Message (SignedId). If it's SignedId, respond
                // with empty PublicKey to bypass encryption and let the peer proceed
                // to Connection::start.
                loop {
                    let buf = match relay.next_timeout(30000).await {
                        Some(Ok(b)) => b,
                        Some(Err(e)) => return Err(format!("relay pairing: {}", e)),
                        None => return Err("relay: timeout after pairing".into()),
                    };
                    // Try as Message first (application layer)
                    if let Ok(app_msg) = Message::parse_from_bytes(&buf) {
                        if app_msg.has_signed_id() {
                            log::info!("relay: got SignedId, responding with empty PublicKey");
                            let mut pk_msg = Message::new();
                            pk_msg.set_public_key(PublicKey {
                                asymmetric_value: bytes::Bytes::new(),
                                symmetric_value: bytes::Bytes::new(),
                                ..Default::default()
                            });
                            relay.send(&pk_msg).await.map_err(|e| format!("pk send: {}", e))?;
                            break;
                        }
                        if app_msg.has_hash() || app_msg.has_test_delay() {
                            log::info!("relay: already at Connection::start (Hash/TestDelay)");
                            break;
                        }
                    }
                    // Try as RendezvousMessage and skip KeyExchange
                    if let Ok(rz_msg) = hbb_common::rendezvous_proto::RendezvousMessage::parse_from_bytes(&buf) {
                        if rz_msg.has_key_exchange() {
                            log::info!("relay: skipped KeyExchange");
                            continue;
                        }
                    }
                    log::info!("relay: unexpected msg after pairing ({} bytes)", buf.len());
                    break;
                }
                return Ok(relay);
            } else {
                log::info!("Unexpected response type from server");
            }
        } else {
            log::info!("No response from server (timeout)");
        }
        log::info!("Punch attempt {} failed, retrying...", i);
    }
    Err("all punch attempts failed — peer may be behind NAT. Ensure hbbr is running on 21117.".into())
}

async fn send_msg(stream: &mut Stream, action: FileAction) -> Result<(), String> {
    let mut msg = Message::new();
    msg.set_file_action(action);
    stream.send(&msg).await.map_err(|e| format!("send: {}", e))
}

async fn send_file_resp(stream: &mut Stream, resp: FileResponse) -> Result<(), String> {
    let mut msg = Message::new();
    msg.set_file_response(resp);
    stream.send(&msg).await.map_err(|e| format!("send: {}", e))
}

async fn recv_msg(stream: &mut Stream) -> Result<FileResponse, String> {
    loop {
        let buf = stream.next_timeout(TIMEOUT_SECS * 1000).await
            .ok_or("timeout")?
            .map_err(|e| format!("recv: {}", e))?;
        let msg: Message = Message::parse_from_bytes(&buf)
            .map_err(|e| format!("parse: {}", e))?;
        if msg.has_file_response() {
            return Ok(msg.file_response().clone());
        } else if msg.has_hash() || msg.has_test_delay() || msg.has_signed_id() || msg.has_login_request() || msg.has_login_response() {
            log::info!("recv_msg: skipping handshake msg type ({} bytes), waiting for FileResponse", buf.len());
            continue;
        } else {
            log::info!("recv_msg: unexpected msg type bytes_len={} raw_hex_first32={:02x?}", buf.len(), &buf[..buf.len().min(32)]);
            continue;
        }
    }
}
