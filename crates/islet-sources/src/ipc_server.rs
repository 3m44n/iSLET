//! Async Unix domain socket server at `$XDG_RUNTIME_DIR/islet.sock` for `islet-ctl`.

use islet_core::{IpcCommand, IpcResponse, EventSender, socket_path};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, info, warn};

/// Listens for `islet-ctl` commands on `$XDG_RUNTIME_DIR/islet.sock`.
pub async fn run_ipc_server(sender: EventSender) {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if path.exists() {
        let _ = tokio::fs::remove_file(&path).await;
    }

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(err) => {
            warn!(%err, path = %path.display(), "Failed to bind IPC socket; islet-ctl disabled");
            return;
        }
    };

    info!(socket = %path.display(), "IPC Unix socket server listening for islet-ctl");

    while let Ok((stream, _)) = listener.accept().await {
        let tx = sender.clone();
        tokio::spawn(async move { handle_connection(stream, tx).await });
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    tx: EventSender,
) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let response = match IpcCommand::from_wire_line(&line) {
            Some(cmd) => {
                debug!(?cmd, "Received IPC command");
                match cmd.into_event() {
                    Some(event) => {
                        if tx.send(event) {
                            IpcResponse::Ok
                        } else {
                            IpcResponse::ErrShutdown
                        }
                    }
                    None => IpcResponse::Pong,
                }
            }
            None => IpcResponse::ErrInvalidCommand,
        };

        if writer.write_all(response.to_wire_line().as_bytes()).await.is_err() {
            break;
        }
    }
}