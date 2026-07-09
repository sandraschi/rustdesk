/// rustdesk++ headless file transfer.
/// Uses rendezvous + relay protocol to transfer files without GUI.

use hbb_common::{
    anyhow,
    bytes,
    config::{self, Config, RENDEZVOUS_PORT},
    fs::{self, TransferJob},
    futures,
    log,
    message_proto::*,
    protobuf::{self, Message as _},
    rendezvous_proto::*,
    socket_client,
    timeout,
    tokio,
    Stream,
};
use std::sync::Arc;

const TIMEOUT_SECS: u64 = 30;

pub async fn send_file(
    peer_id: &str,
    local_path: &str,
    remote_path: &str,
) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;
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
) -> Result<(), String> {
    let mut stream = establish_connection(peer_id).await?;

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

    let mut file = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| format!("create: {}", e))?;

    loop {
        let resp = recv_msg(&mut stream).await?;
        if resp.has_block() {
            use tokio::io::AsyncWriteExt;
                file.write_all(&resp.block().data[..]).await
                .map_err(|e| format!("write: {}", e))?;
        } else if resp.has_done() {
            break;
        } else if resp.has_error() {
            return Err(format!("remote error: {}", resp.error().error));
        }
    }
    Ok(())
}

pub async fn list_dir(peer_id: &str, remote_path: &str) -> Result<Vec<String>, String> {
    let mut stream = establish_connection(peer_id).await?;

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

fn get_rendezvous_addr() -> String {
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

    // Register PK (required even for local servers — PunchHoleRequest won't be answered otherwise)
    let uuid_bytes: bytes::Bytes = hbb_common::get_uuid().into();
    let (_sk, pk) = Config::get_key_pair();
    let mut rp = RegisterPk::new();
    rp.id = Config::get_id();
    rp.pk = pk.into();
    rp.uuid = uuid_bytes;
    let mut reg_msg = RendezvousMessage::new();
    reg_msg.set_register_pk(rp);
    stream.send(&reg_msg).await.map_err(|e| format!("register: {}", e))?;

    let reg_resp = crate::get_next_nonkeyexchange_msg(&mut stream, Some(10000))
        .await.ok_or("register timeout")?;

    if reg_resp.has_register_pk_response() {
        let rpr = reg_resp.register_pk_response();
        if rpr.result.enum_value() != Ok(register_pk_response::Result::OK) {
            log::warn!("Register PK not OK, continuing anyway");
        } else {
            log::info!("Register PK confirmed");
        }
    } else {
        log::warn!("Unexpected register response, continuing");
    }

    // Now send PunchHoleRequest (matching client.rs::_start_inner)
    use hbb_common::protobuf::Enum;
    let mut msg = RendezvousMessage::new();
    msg.set_punch_hole_request(PunchHoleRequest {
        id: peer_id.to_owned(),
        token: access_token,
        conn_type: ConnType::FILE_TRANSFER.into(),
        version: crate::VERSION.to_owned(),
        nat_type: hbb_common::rendezvous_proto::NatType::UNKNOWN_NAT.into(),
        ..Default::default()
    });

    for i in 1..=3 {
        stream.send(&msg).await.map_err(|e| format!("punch #{}: {}", i, e))?;
        if let Some(resp) = crate::get_next_nonkeyexchange_msg(&mut stream, Some(i * 3000)).await {
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

                // Wait for relay response with extended timeout (peer may be offline)
                match crate::get_next_nonkeyexchange_msg(&mut relay, Some(120000)).await {
                    Some(_resp) => return Ok(relay),
                    None => return Err("relay: no response from relay server (peer may be offline or using different relay)".into()),
                }
            }
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
    let buf = stream.next_timeout(TIMEOUT_SECS * 1000).await
        .ok_or("timeout")?
        .map_err(|e| format!("recv: {}", e))?;
    let msg: Message = Message::parse_from_bytes(&buf)
        .map_err(|e| format!("parse: {}", e))?;
    if msg.has_file_response() {
        Ok(msg.file_response().clone())
    } else {
        Err("unexpected message (not file_response)".into())
    }
}
