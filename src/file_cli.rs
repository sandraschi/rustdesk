/// rustdesk++ headless file transfer.
/// Uses rendezvous + relay protocol to transfer files without GUI.

use hbb_common::{
    allow_err,
    config::Config,
    fs::{self, TransferJob},
    futures,
    log,
    message_proto::*,
    protobuf::Message as _,
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
    let path = std::path::Path::new(local_path);
    let file_name = fs::get_file_name(path);
    let file_size = std::fs::metadata(local_path)
        .map_err(|e| format!("cannot stat {}: {}", local_path, e))?
        .len();

    let job_id = fs::get_next_job_id();
    let mut entry = FileEntry::new();
    entry.set_name(file_name);
    entry.set_entry_type(FileType::File.into());
    entry.set_size(file_size);

    let mut send = file_transfer_send_request::FileTransferSendRequest::new();
    send.set_id(job_id);
    send.set_file_num(1);
    send.set_file_type(file_transfer_send_request::FileType::Generic);
    send.set_file_entry(entry);

    let mut action = FileAction::new();
    action.set_send(send);
    send_file_action(&mut stream, action).await?;

    let _ = recv_file_response(&mut stream).await?;

    let remote = if remote_path.ends_with('/') || remote_path.ends_with('\\') {
        format!("{}{}", remote_path, fs::get_file_name(std::path::Path::new(local_path)))
    } else {
        remote_path.to_string()
    };

    let mut job = TransferJob::new_read(
        job_id,
        fs::JobType::Generic,
        remote,
        fs::DataSource::FilePath(std::path::PathBuf::from(local_path)),
        1,
        false, false, false,
    ).map_err(|e| format!("job: {}", e))?;

    loop {
        match job.read().await.map_err(|e| format!("read: {}", e))? {
            Some(block) => {
                let mut fr = FileResponse::new();
                fr.set_block(block);
                send_file_response(&mut stream, fr).await?;
            }
            None => {
                let mut done = FileResponse::new();
                done.set_done(FileTransferDone::new());
                send_file_response(&mut stream, done).await?;
                break;
            }
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

    let mut recv = FileTransferReceiveRequest::new();
    recv.set_id(fs::get_next_job_id());
    recv.set_dir(remote_path.to_string());
    recv.set_include_hidden(false);
    recv.set_recursive(false);
    recv.set_with_empty_dirs(false);

    let mut action = FileAction::new();
    action.set_receive(recv);
    send_file_action(&mut stream, action).await?;

    let parent = std::path::Path::new(local_path).parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;

    let mut file = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| format!("create: {}", e))?;

    loop {
        let resp = recv_file_response(&mut stream).await?;
        if resp.has_block() {
            use tokio::io::AsyncWriteExt;
            file.write_all(resp.get_block().get_data()).await
                .map_err(|e| format!("write: {}", e))?;
        } else if resp.has_done() {
            break;
        } else if resp.has_error() {
            return Err(format!("remote error: {}", resp.get_error().get_msg()));
        }
    }
    Ok(())
}

pub async fn list_dir(peer_id: &str, remote_path: &str) -> Result<Vec<String>, String> {
    let mut stream = establish_connection(peer_id).await?;

    let mut rd = ReadDir::new();
    rd.set_id(fs::get_next_job_id());
    rd.set_dir(remote_path.to_string());
    rd.set_include_hidden(false);

    let mut action = FileAction::new();
    action.set_read_dir(rd);
    send_file_action(&mut stream, action).await?;

    let resp = recv_file_response(&mut stream).await?;
    if resp.has_dir() {
        let dir = resp.get_dir();
        Ok(dir.get_entries().iter().map(|e| {
            let kind = match e.get_entry_type().enum_value().unwrap_or(FileType::File) {
                FileType::Dir | FileType::DirDrive => "DIR",
                _ => "FILE",
            };
            format!("[{}] {} {}b", kind, e.get_name(), e.get_size())
        }).collect())
    } else if resp.has_error() {
        Err(format!("remote: {}", resp.get_error().get_msg()))
    } else {
        Err("unexpected response".into())
    }
}

// --- Connection ---

fn get_rendezvous_addr() -> String {
    let id = Config::get_id_server();
    format!("{}:{}", id.1, id.2)
}

async fn establish_connection(peer_id: &str) -> Result<Stream, String> {
    let addr = get_rendezvous_addr();
    let mut stream = socket_client::connect_tcp(&addr, 10000)
        .await
        .map_err(|e| format!("rendezvous: {}", e))?;

    let mut msg = RendezvousMessage::new();
    let mut phr = PunchHoleRequest::new();
    phr.set_id(peer_id.to_string());
    phr.set_conn_type(ConnType::FILE_TRANSFER);
    msg.set_punch_hole_request(phr);

    let payload = msg.write_to_bytes().map_err(|e| format!("encode: {}", e))?;
    stream.send_raw(payload).await.map_err(|e| format!("send: {}", e))?;

    let buf = stream.next_timeout(TIMEOUT_SECS * 1000).await
        .ok_or("timeout waiting for rendezvous")?
        .map_err(|e| format!("recv: {}", e))?;

    let resp: RendezvousMessage = protobuf::parse_from_bytes(&buf)
        .map_err(|e| format!("parse: {}", e))?;

    if !resp.has_punch_hole_response() {
        return Err("unexpected rendezvous response".into());
    }

    let phr = resp.get_punch_hole_response();
    if phr.has_relay_server() {
        let rs = phr.get_relay_server();
        let relay_addr = format!("{}:{}", rs.get_host(), rs.get_port());
        let mut relay = socket_client::connect_tcp(&relay_addr, 10000)
            .await
            .map_err(|e| format!("relay: {}", e))?;

        let mut rmsg = RendezvousMessage::new();
        let mut rr = RequestRelay::new();
        rr.set_id(peer_id.to_string());
        rr.set_conn_type(ConnType::FILE_TRANSFER);
        rmsg.set_request_relay(rr);

        let p = rmsg.write_to_bytes().map_err(|e| format!("encode: {}", e))?;
        relay.send_raw(p).await.map_err(|e| format!("send relay: {}", e))?;

        let b = relay.next_timeout(TIMEOUT_SECS * 1000).await
            .ok_or("timeout waiting for relay")?
            .map_err(|e| format!("recv relay: {}", e))?;
        let _relay_resp: RendezvousMessage = protobuf::parse_from_bytes(&b)
            .map_err(|e| format!("parse relay: {}", e))?;

        Ok(relay)
    } else if phr.has_socket_addr() {
        Err("direct connections not yet supported in headless mode".into())
    } else {
        Err("no relay or direct address".into())
    }
}

async fn send_file_action(stream: &mut Stream, action: FileAction) -> Result<(), String> {
    let mut msg = Message::new();
    msg.set_file_action(action);
    let p = msg.write_to_bytes().map_err(|e| format!("encode: {}", e))?;
    stream.send_raw(p).await.map_err(|e| format!("send: {}", e))
}

async fn recv_file_response(stream: &mut Stream) -> Result<FileResponse, String> {
    let buf = stream.next_timeout(TIMEOUT_SECS * 1000).await
        .ok_or("timeout")?
        .map_err(|e| format!("recv: {}", e))?;
    let msg: Message = protobuf::parse_from_bytes(&buf)
        .map_err(|e| format!("parse: {}", e))?;
    if msg.has_file_response() {
        Ok(msg.get_file_response().clone())
    } else {
        Err("unexpected message (not file_response)".into())
    }
}
