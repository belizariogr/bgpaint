//! One BGPaint per user session: a second launch hands its files to the running instance, which
//! opens each in a new tab (a blank tab when there are none) and comes to the front, and exits.
//!
//! The instances talk over a Unix socket in the user's runtime directory. The message is the
//! absolute paths, each followed by a NUL byte. `BGPAINT_NEW_INSTANCE=1` starts an independent
//! instance; debug builds use their own socket so `cargo run` never hands files to an installed copy.

use crate::files::DialogResult;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// How this process should go on.
pub enum Start {
    /// Run the app; `Some` when it now answers later launches.
    Primary(Option<Listener>),
    /// Another instance took the files: exit.
    HandedOff,
}

/// The bound socket, not yet serving.
pub struct Listener {
    listener: UnixListener,
    socket: Socket,
}

/// The socket file this instance created (removed on exit if it is still ours).
pub struct Socket {
    path: PathBuf,
    id: (u64, u64),
}

fn socket_path() -> PathBuf {
    let name = if cfg!(debug_assertions) { "bgpaint-dev.sock" } else { "bgpaint.sock" };
    // XDG_RUNTIME_DIR is private to the user; macOS's temp dir is per user too.
    if let Some(dir) = dirs::runtime_dir() {
        return dir.join(name);
    }
    let user = std::env::var("USER").unwrap_or_default();
    std::env::temp_dir().join(format!("{user}-{name}"))
}

fn send(path: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut msg = Vec::new();
    for f in files {
        let f = std::path::absolute(f).unwrap_or_else(|_| f.clone());
        msg.extend_from_slice(f.as_os_str().as_bytes());
        msg.push(0);
    }
    stream.write_all(&msg)?;
    stream.shutdown(std::net::Shutdown::Write)
}

fn bind(path: &Path) -> Option<Listener> {
    let listener = UnixListener::bind(path).ok()?;
    let meta = std::fs::metadata(path).ok()?;
    Some(Listener { listener, socket: Socket { path: path.to_path_buf(), id: (meta.dev(), meta.ino()) } })
}

/// Hands `files` to a running instance, or claims the socket for this one.
pub fn start(files: &[PathBuf]) -> Start {
    if cfg!(feature = "automation") || std::env::var_os("BGPAINT_NEW_INSTANCE").is_some() {
        return Start::Primary(None);
    }
    let path = socket_path();
    if send(&path, files).is_ok() {
        return Start::HandedOff;
    }
    if let Some(l) = bind(&path) {
        return Start::Primary(Some(l));
    }
    // Another launch may have bound it just now; otherwise it is left over from a crash
    // (nobody answered above).
    if send(&path, files).is_ok() {
        return Start::HandedOff;
    }
    let _ = std::fs::remove_file(&path);
    Start::Primary(bind(&path))
}

impl Listener {
    /// Answers later launches on a worker thread: their paths arrive as [`DialogResult::Launched`].
    pub fn serve(self, ctx: &egui::Context, tx: Sender<DialogResult>) -> Socket {
        let ctx = ctx.clone();
        let listener = self.listener;
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut msg = Vec::new();
                if stream.read_to_end(&mut msg).is_err() {
                    continue;
                }
                let paths = msg
                    .split(|&b| b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| PathBuf::from(std::ffi::OsString::from_vec(p.to_vec())))
                    .collect();
                if tx.send(DialogResult::Launched(paths)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        self.socket
    }
}

impl Socket {
    /// Stops answering launches (before exiting, or relaunching after an update), unless another
    /// instance has replaced the socket meanwhile.
    pub fn release(self) {
        if std::fs::metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.id) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_files_to_the_running_instance() {
        let dir = std::env::temp_dir().join(format!("bgpaint-si-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = bind(&path).unwrap();
        let files = [PathBuf::from("/tmp/a b.png"), PathBuf::from("/tmp/c\nd.png")];
        send(&path, &files).unwrap();
        let (mut stream, _) = listener.listener.accept().unwrap();
        let mut msg = Vec::new();
        stream.read_to_end(&mut msg).unwrap();
        assert_eq!(msg, b"/tmp/a b.png\0/tmp/c\nd.png\0");
        listener.socket.release();
        assert!(!path.exists());
        // Nobody listens on a leftover socket file.
        let stale = bind(&path).unwrap();
        drop(stale.listener);
        assert!(send(&path, &files).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
