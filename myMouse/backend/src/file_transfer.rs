use mymouse_protocol::{FileChunk, FileOffer, Message, TransferId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_TRANSFER_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB
pub const DEFAULT_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Offered,
    Accepted,
    Rejected,
    InProgress,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferRecord {
    pub transfer_id: TransferId,
    pub direction: TransferDirection,
    pub peer_device_id: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub state: TransferState,
    pub bytes_transferred: u64,
    pub path: Option<String>,
    pub message: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferSettings {
    pub auto_accept: bool,
    pub save_dir: String,
    pub max_transfer_bytes: u64,
}

impl Default for FileTransferSettings {
    fn default() -> Self {
        let save_dir = default_save_dir()
            .to_string_lossy()
            .to_string();
        Self {
            auto_accept: false,
            save_dir,
            max_transfer_bytes: DEFAULT_MAX_TRANSFER_BYTES,
        }
    }
}

pub fn default_save_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let mut p = PathBuf::from(home);
        p.push("Downloads");
        p.push("myMouse");
        return p;
    }
    std::env::temp_dir().join("myMouse")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_file_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("transfer.bin");
    let sanitized: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "transfer.bin".to_string()
    } else {
        trimmed
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug)]
struct IncomingTransfer {
    record: TransferRecord,
    part_path: PathBuf,
}

#[derive(Debug)]
struct OutgoingTransfer {
    record: TransferRecord,
    path: PathBuf,
    offset: u64,
}

pub struct FileTransferManager {
    device_id: String,
    settings: FileTransferSettings,
    incoming: HashMap<TransferId, IncomingTransfer>,
    outgoing: HashMap<TransferId, OutgoingTransfer>,
    history: Vec<TransferRecord>,
}

impl FileTransferManager {
    pub fn new(device_id: impl Into<String>, settings: FileTransferSettings) -> Self {
        Self {
            device_id: device_id.into(),
            settings,
            incoming: HashMap::new(),
            outgoing: HashMap::new(),
            history: Vec::new(),
        }
    }

    pub fn settings(&self) -> &FileTransferSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: FileTransferSettings) {
        self.settings = settings;
    }

    pub fn history(&self) -> &[TransferRecord] {
        &self.history
    }

    fn push_history(&mut self, record: TransferRecord) {
        if let Some(existing) = self.history.iter_mut().find(|r| r.transfer_id == record.transfer_id) {
            *existing = record;
        } else {
            self.history.push(record);
        }
        if self.history.len() > 200 {
            let drain = self.history.len() - 200;
            self.history.drain(0..drain);
        }
    }

    pub fn create_offer(&mut self, path: &Path, peer_device_id: &str) -> std::io::Result<(FileOffer, Vec<Message>)> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a file"));
        }
        let size = metadata.len();
        if size > self.settings.max_transfer_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "file exceeds configured size limit",
            ));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("transfer.bin")
            .to_string();
        let sha256 = sha256_file(path)?;
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let offer = FileOffer {
            transfer_id: transfer_id.clone(),
            from_device_id: self.device_id.clone(),
            name: name.clone(),
            size,
            sha256: sha256.clone(),
            mime: None,
        };
        let record = TransferRecord {
            transfer_id: transfer_id.clone(),
            direction: TransferDirection::Outgoing,
            peer_device_id: peer_device_id.to_string(),
            name,
            size,
            sha256,
            state: TransferState::Offered,
            bytes_transferred: 0,
            path: Some(path.to_string_lossy().to_string()),
            message: None,
            updated_at_ms: now_ms(),
        };
        self.outgoing.insert(
            transfer_id.clone(),
            OutgoingTransfer {
                record: record.clone(),
                path: path.to_path_buf(),
                offset: 0,
            },
        );
        self.push_history(record);
        Ok((offer.clone(), vec![Message::FileTransferOffer(offer)]))
    }

    /// Handle an incoming offer. Returns protocol messages to send (accept/reject).
    pub fn handle_offer(&mut self, offer: FileOffer, peer_device_id: &str) -> Vec<Message> {
        if offer.size > self.settings.max_transfer_bytes {
            let record = TransferRecord {
                transfer_id: offer.transfer_id.clone(),
                direction: TransferDirection::Incoming,
                peer_device_id: peer_device_id.to_string(),
                name: offer.name,
                size: offer.size,
                sha256: offer.sha256,
                state: TransferState::Rejected,
                bytes_transferred: 0,
                path: None,
                message: Some("size limit exceeded".into()),
                updated_at_ms: now_ms(),
            };
            self.push_history(record);
            return vec![Message::FileTransferReject {
                transfer_id: offer.transfer_id,
                reason: Some("size limit exceeded".into()),
            }];
        }

        let save_dir = PathBuf::from(&self.settings.save_dir);
        let safe_name = sanitize_file_name(&offer.name);
        let final_path = save_dir.join(&safe_name);
        let part_path = save_dir.join(format!("{safe_name}.{}.part", offer.transfer_id));

        let record = TransferRecord {
            transfer_id: offer.transfer_id.clone(),
            direction: TransferDirection::Incoming,
            peer_device_id: peer_device_id.to_string(),
            name: safe_name,
            size: offer.size,
            sha256: offer.sha256,
            state: if self.settings.auto_accept { TransferState::Accepted } else { TransferState::Offered },
            bytes_transferred: 0,
            path: Some(final_path.to_string_lossy().to_string()),
            message: None,
            updated_at_ms: now_ms(),
        };

        self.incoming.insert(
            offer.transfer_id.clone(),
            IncomingTransfer {
                record: record.clone(),
                part_path,
            },
        );
        self.push_history(record);

        if self.settings.auto_accept {
            vec![Message::FileTransferAccept {
                transfer_id: offer.transfer_id,
            }]
        } else {
            Vec::new()
        }
    }

    pub fn accept(&mut self, transfer_id: &str) -> std::io::Result<Vec<Message>> {
        let record = {
            let incoming = self.incoming.get_mut(transfer_id).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "unknown transfer")
            })?;
            if let Some(parent) = incoming.part_path.parent() {
                fs::create_dir_all(parent)?;
            }
            File::create(&incoming.part_path)?;
            incoming.record.state = TransferState::Accepted;
            incoming.record.updated_at_ms = now_ms();
            incoming.record.clone()
        };
        self.push_history(record);
        Ok(vec![Message::FileTransferAccept {
            transfer_id: transfer_id.to_string(),
        }])
    }

    pub fn reject(&mut self, transfer_id: &str, reason: Option<String>) -> Vec<Message> {
        if let Some(mut incoming) = self.incoming.remove(transfer_id) {
            incoming.record.state = TransferState::Rejected;
            incoming.record.message = reason.clone();
            incoming.record.updated_at_ms = now_ms();
            self.push_history(incoming.record);
        }
        vec![Message::FileTransferReject {
            transfer_id: transfer_id.to_string(),
            reason,
        }]
    }

    pub fn handle_accept(&mut self, transfer_id: &str) -> std::io::Result<Vec<Message>> {
        let record = {
            let outgoing = self.outgoing.get_mut(transfer_id).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "unknown transfer")
            })?;
            outgoing.record.state = TransferState::InProgress;
            outgoing.record.updated_at_ms = now_ms();
            outgoing.record.clone()
        };
        self.push_history(record);
        self.next_chunks(transfer_id, DEFAULT_CHUNK_BYTES)
    }

    pub fn next_chunks(&mut self, transfer_id: &str, chunk_size: usize) -> std::io::Result<Vec<Message>> {
        let (record, chunk, last) = {
            let outgoing = self.outgoing.get_mut(transfer_id).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "unknown transfer")
            })?;
            let total = outgoing.record.size;
            // Send one chunk per call to keep IPC simple; caller can loop.
            if outgoing.offset >= total {
                return Ok(vec![Message::FileTransferComplete {
                    transfer_id: transfer_id.to_string(),
                }]);
            }
            let mut file = File::open(&outgoing.path)?;
            file.seek(SeekFrom::Start(outgoing.offset))?;
            let mut buf = vec![0u8; chunk_size.min((total - outgoing.offset) as usize)];
            let n = file.read(&mut buf)?;
            buf.truncate(n);
            let offset = outgoing.offset;
            outgoing.offset += n as u64;
            outgoing.record.bytes_transferred = outgoing.offset;
            outgoing.record.updated_at_ms = now_ms();
            let last = outgoing.offset >= total;
            if last {
                outgoing.record.state = TransferState::InProgress;
            }
            (
                outgoing.record.clone(),
                FileChunk {
                    transfer_id: transfer_id.to_string(),
                    offset,
                    data: buf,
                    last,
                },
                last,
            )
        };
        self.push_history(record);
        let mut messages = vec![Message::FileTransferChunk(chunk)];
        if last {
            messages.push(Message::FileTransferComplete {
                transfer_id: transfer_id.to_string(),
            });
        }
        Ok(messages)
    }

    pub fn handle_chunk(&mut self, chunk: FileChunk) -> std::io::Result<Option<TransferRecord>> {
        let incoming = self.incoming.get_mut(&chunk.transfer_id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown transfer")
        })?;
        if let Some(parent) = incoming.part_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&incoming.part_path)?;
        file.seek(SeekFrom::Start(chunk.offset))?;
        file.write_all(&chunk.data)?;
        incoming.record.bytes_transferred = incoming.record.bytes_transferred.max(chunk.offset + chunk.data.len() as u64);
        incoming.record.state = TransferState::InProgress;
        incoming.record.updated_at_ms = now_ms();
        let record = incoming.record.clone();
        self.push_history(record.clone());
        if chunk.last {
            return Ok(Some(record));
        }
        Ok(None)
    }

    pub fn handle_complete(&mut self, transfer_id: &str) -> std::io::Result<TransferRecord> {
        let mut incoming = self.incoming.remove(transfer_id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "unknown transfer")
        })?;
        let actual = sha256_file(&incoming.part_path)?;
        if actual != incoming.record.sha256 {
            incoming.record.state = TransferState::Failed;
            incoming.record.message = Some("sha256 mismatch".into());
            incoming.record.updated_at_ms = now_ms();
            let _ = fs::remove_file(&incoming.part_path);
            self.push_history(incoming.record.clone());
            return Ok(incoming.record);
        }
        let final_path = PathBuf::from(incoming.record.path.clone().unwrap_or_else(|| {
            default_save_dir().join(&incoming.record.name).to_string_lossy().to_string()
        }));
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Avoid overwriting by adding suffix.
        let mut dest = final_path.clone();
        if dest.exists() {
            let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("file").to_string();
            let ext = dest.extension().and_then(|s| s.to_str()).map(|s| format!(".{s}")).unwrap_or_default();
            for i in 1..1000 {
                let candidate = dest.with_file_name(format!("{stem} ({i}){ext}"));
                if !candidate.exists() {
                    dest = candidate;
                    break;
                }
            }
        }
        fs::rename(&incoming.part_path, &dest)?;
        incoming.record.state = TransferState::Completed;
        incoming.record.path = Some(dest.to_string_lossy().to_string());
        incoming.record.bytes_transferred = incoming.record.size;
        incoming.record.updated_at_ms = now_ms();
        self.push_history(incoming.record.clone());
        Ok(incoming.record)
    }

    /// Apply a rejection received from the peer.
    pub fn handle_reject(&mut self, transfer_id: &str, reason: Option<String>) -> Option<TransferRecord> {
        if let Some(mut outgoing) = self.outgoing.remove(transfer_id) {
            outgoing.record.state = TransferState::Rejected;
            outgoing.record.message = reason;
            outgoing.record.updated_at_ms = now_ms();
            self.push_history(outgoing.record.clone());
            return Some(outgoing.record);
        }
        if let Some(mut incoming) = self.incoming.remove(transfer_id) {
            let _ = fs::remove_file(&incoming.part_path);
            incoming.record.state = TransferState::Rejected;
            incoming.record.message = reason;
            incoming.record.updated_at_ms = now_ms();
            self.push_history(incoming.record.clone());
            return Some(incoming.record);
        }
        None
    }

    pub fn handle_cancel(&mut self, transfer_id: &str, reason: Option<String>) -> Option<TransferRecord> {
        if let Some(mut incoming) = self.incoming.remove(transfer_id) {
            let _ = fs::remove_file(&incoming.part_path);
            incoming.record.state = TransferState::Cancelled;
            incoming.record.message = reason;
            incoming.record.updated_at_ms = now_ms();
            self.push_history(incoming.record.clone());
            return Some(incoming.record);
        }
        if let Some(mut outgoing) = self.outgoing.remove(transfer_id) {
            outgoing.record.state = TransferState::Cancelled;
            outgoing.record.message = reason;
            outgoing.record.updated_at_ms = now_ms();
            self.push_history(outgoing.record.clone());
            return Some(outgoing.record);
        }
        None
    }

    pub fn mark_outgoing_complete(&mut self, transfer_id: &str) -> Option<TransferRecord> {
        if let Some(mut outgoing) = self.outgoing.remove(transfer_id) {
            outgoing.record.state = TransferState::Completed;
            outgoing.record.bytes_transferred = outgoing.record.size;
            outgoing.record.updated_at_ms = now_ms();
            self.push_history(outgoing.record.clone());
            return Some(outgoing.record);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn offer_rejected_when_too_large() {
        let dir = tempdir().unwrap();
        let mut mgr = FileTransferManager::new(
            "pc-a",
            FileTransferSettings { auto_accept: true, save_dir: dir.path().to_string_lossy().to_string(), max_transfer_bytes: 4 },
        );
        let offer = FileOffer {
            transfer_id: "t1".into(),
            from_device_id: "pc-b".into(),
            name: "big.bin".into(),
            size: 10,
            sha256: "x".into(),
            mime: None,
        };
        let msgs = mgr.handle_offer(offer, "pc-b");
        assert!(matches!(msgs[0], Message::FileTransferReject { .. }));
    }

    #[test]
    fn manual_accept_updates_state_and_history() {
        let dir = tempdir().unwrap();
        let mut mgr = FileTransferManager::new(
            "pc-a",
            FileTransferSettings {
                auto_accept: false,
                save_dir: dir.path().to_string_lossy().to_string(),
                max_transfer_bytes: 1024,
            },
        );
        let offer = FileOffer {
            transfer_id: "manual-1".into(),
            from_device_id: "pc-b".into(),
            name: "note.txt".into(),
            size: 4,
            sha256: "x".into(),
            mime: None,
        };
        assert!(mgr.handle_offer(offer, "pc-b").is_empty());

        let messages = mgr.accept("manual-1").unwrap();
        assert!(matches!(messages[0], Message::FileTransferAccept { .. }));

        let record = mgr.history().first().expect("history should contain the transfer");
        assert_eq!(record.state, TransferState::Accepted);
        assert!(dir.path().join("note.txt.manual-1.part").exists());
    }

    #[test]
    fn chunked_transfer_round_trip_with_hash() {
        let dir = tempdir().unwrap();
        let src_dir = tempdir().unwrap();
        let src = src_dir.path().join("hello.txt");
        fs::write(&src, b"hello world").unwrap();

        let mut sender = FileTransferManager::new("pc-a", FileTransferSettings { auto_accept: true, save_dir: dir.path().to_string_lossy().to_string(), max_transfer_bytes: 1024 * 1024 });
        let mut receiver = FileTransferManager::new("pc-b", FileTransferSettings { auto_accept: true, save_dir: dir.path().to_string_lossy().to_string(), max_transfer_bytes: 1024 * 1024 });

        let (offer, msgs) = sender.create_offer(&src, "pc-b").unwrap();
        assert!(matches!(msgs[0], Message::FileTransferOffer(_)));

        let recv_msgs = receiver.handle_offer(offer, "pc-a");
        assert!(matches!(recv_msgs[0], Message::FileTransferAccept { .. }));

        let mut all_done = false;
        let mut transfer_id = "t1".to_string();
        // simulate accept on sender
        if let Message::FileTransferAccept { transfer_id: id } = &recv_msgs[0] {
            transfer_id = id.clone();
            let mut chunks = sender.handle_accept(&id).unwrap();
            while !chunks.is_empty() {
                let mut next = Vec::new();
                for msg in chunks {
                    match msg {
                        Message::FileTransferChunk(chunk) => {
                            let last = chunk.last;
                            let _ = receiver.handle_chunk(chunk).unwrap();
                            if last {
                                // wait for complete message
                            }
                        }
                        Message::FileTransferComplete { transfer_id: id2 } => {
                            let rec = receiver.handle_complete(&id2).unwrap();
                            assert_eq!(rec.state, TransferState::Completed);
                            let saved = fs::read(rec.path.unwrap()).unwrap();
                            assert_eq!(saved, b"hello world");
                            sender.mark_outgoing_complete(&id2);
                            all_done = true;
                        }
                        _ => {}
                    }
                }
                if all_done { break; }
                next.extend(sender.next_chunks(&transfer_id, 4).unwrap());
                chunks = next;
                if chunks.is_empty() { break; }
            }
        }
        assert!(all_done, "transfer should complete");
    }
}