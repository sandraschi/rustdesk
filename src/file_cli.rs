/// rustdesk++ headless file transfer.
/// Uses rendezvous + relay protocol to transfer files without GUI.

use hbb_common::{
    anyhow,
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
    let _server = Config::get_rendezvous_server();
    "209.250.254.15:21116".to_string()
}

async fn establish_connection(peer_id: &str) -> Result<Stream, String> {
    let addr = get_rendezvous_addr();
    let mut stream = socket_client::connect_tcp(addr, 30000)
        .await
        .map_err(|e| format!("rendezvous: {}", e))?;

    // Send PunchHoleRequest
    let mut msg = RendezvousMessage::new();
    msg.set_punch_hole_request(PunchHoleRequest {
        id: peer_id.to_owned(),
        conn_type: ConnType::FILE_TRANSFER.into(),
        version: crate::VERSION.to_owned(),
        ..Default::default()
    });
    stream.send(&msg).await.map_err(|e| format!("send: {}", e))?;

    // Wait for response (PunchHoleResponse or RelayResponse)
    let buf = stream.next_timeout(TIMEOUT_SECS * 1000).await
        .ok_or("timeout waiting for rendezvous")?
        .map_err(|e| format!("recv: {}", e))?;

    let resp: RendezvousMessage = RendezvousMessage::parse_from_bytes(&buf)
        .map_err(|e| format!("parse: {}", e))?;

    // Handle PunchHoleResponse
    if resp.has_punch_hole_response() {
        let phr = resp.punch_hole_response();
        if !phr.relay_server.is_empty() {
            let relay_addr = phr.relay_server.clone();
            let mut relay = socket_client::connect_tcp(relay_addr.as_str(), 30000)
                .await
                .map_err(|e| format!("relay: {}", e))?;

            let mut rmsg = RendezvousMessage::new();
            rmsg.set_request_relay(RequestRelay {
                id: peer_id.to_owned(),
                conn_type: ConnType::FILE_TRANSFER.into(),
                ..Default::default()
            });
            relay.send(&rmsg).await.map_err(|e| format!("send relay: {}", e))?;

            let b = relay.next_timeout(TIMEOUT_SECS * 1000).await
                .ok_or("timeout waiting for relay")?
                .map_err(|e| format!("recv relay: {}", e))?;
            let _relay_resp: RendezvousMessage = RendezvousMessage::parse_from_bytes(&b)
                .map_err(|e| format!("parse relay: {}", e))?;

            Ok(relay)
        } else if !phr.socket_addr.is_empty() {
            // Direct P2P connection
            let addr_bytes = &phr.socket_addr;
            let peer_addr = hbb_common::AddrMangle::decode(addr_bytes);
            let p2p_stream = socket_client::connect_tcp_local(peer_addr.to_string().as_str(), None, 10000)
                .await
                .map_err(|e| format!("p2p: {}", e))?;
            Ok(p2p_stream)
        } else {
            let err = if !phr.other_failure.is_empty() {
                phr.other_failure.clone()
            } else {
                format!("punch hole failed: {:?}", phr.failure)
            };
            Err(err)
        }
    } else if resp.has_relay_response() {
        // Server already has a relay connection
        let rr = resp.relay_response();
        let relay_addr = rr.relay_server.clone();
        let mut relay = socket_client::connect_tcp(relay_addr.as_str(), 30000)
            .await
            .map_err(|e| format!("relay: {}", e))?;

        let mut rmsg = RendezvousMessage::new();
        rmsg.set_request_relay(RequestRelay {
            id: peer_id.to_owned(),
            conn_type: ConnType::FILE_TRANSFER.into(),
            ..Default::default()
        });
        let p = rmsg.write_to_bytes().map_err(|e| format!("encode: {}", e))?;
        relay.send_raw(p).await.map_err(|e| format!("send relay: {}", e))?;

        Ok(relay)
    } else if let Some(ref u) = resp.union {
        Err(format!("unexpected rendezvous response variant"))
    } else {
        Err("empty rendezvous response".into())
    }
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
