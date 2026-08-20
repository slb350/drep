//! Private file-backed capture for Codex child output.

use std::io::{Read, Seek, SeekFrom};
use std::process::Stdio;

/// A private temporary file that can be handed to a child and read back with a
/// caller-selected bound.
pub(super) struct BoundedCapture {
    file: std::fs::File,
}

impl BoundedCapture {
    pub(super) fn new() -> std::io::Result<Self> {
        tempfile::tempfile().map(|file| Self { file })
    }

    pub(super) fn child_stdio(&self) -> std::io::Result<Stdio> {
        self.file.try_clone().map(Stdio::from)
    }

    pub(super) fn exceeds(&self, limit: usize) -> std::io::Result<bool> {
        self.file.metadata().map(|meta| meta.len() > limit as u64)
    }

    pub(super) fn read_bounded(&mut self, limit: usize) -> std::io::Result<Vec<u8>> {
        let bytes = self.read_prefix(limit.saturating_add(1))?;
        if bytes.len() > limit {
            return Err(std::io::Error::other(format!(
                "Codex output exceeded {limit} bytes"
            )));
        }
        Ok(bytes)
    }

    pub(super) fn read_prefix(&mut self, limit: usize) -> std::io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file
            .by_ref()
            .take(limit as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
