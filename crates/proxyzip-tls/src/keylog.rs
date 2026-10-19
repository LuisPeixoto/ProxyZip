use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use parking_lot::Mutex;

/// Writes TLS session secrets in NSS key log format so captures can be
/// decrypted in Wireshark (the `SSLKEYLOGFILE` convention).
#[derive(Debug)]
pub struct FileKeyLog {
    file: Mutex<File>,
}

impl FileKeyLog {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = OpenOptions::new().append(true).create(true).open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl rustls::KeyLog for FileKeyLog {
    fn log(&self, label: &str, client_random: &[u8], secret: &[u8]) {
        let line = format!("{label} {} {}\n", hex(client_random), hex(secret));
        let _ = self.file.lock().write_all(line.as_bytes());
    }
}
