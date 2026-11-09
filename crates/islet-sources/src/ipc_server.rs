//! Async Unix domain socket server at `$XDG_RUNTIME_DIR/islet.sock` for `islet-ctl`.

use islet_core::{socket_path, EventSender, IpcCommand};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, info, warn};

/// Listens for `islet-ctl` commands on `$XDG_RUNTIME_DIR/islet.sock`.
pub async fn run_ipc_server(sender: EventSender) {
    let path = socket_path();
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

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let tx = sender.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            if let Ok(Some(line)) = lines.next_line().await {
                if let Some(cmd) = IpcCommand::from_wire_line(&line) {
                    debug!(?cmd, "Received IPC command");
                    if let Some(event) = cmd.into_event() {
                        tx.send(event);
                    }
                    let _ = writer.write_all(b"OK
").await;
                } else {
                    let _ = writer.write_all(b"ERR invalid command
").await;
                }
            }
        });
    }
}
