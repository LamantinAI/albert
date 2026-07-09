//! The agent's persona + operating instructions, loaded from files and **held in
//! RAM** (not re-read from disk each turn — that's OpenClaw's slow anti-pattern).
//! Hot-reloaded by mtime: each turn we `stat` the files and re-read a body only
//! when it changed. A missing file falls back to a terse embedded default, so the
//! bot always runs; the shipped `soul.md` / `system.md` carry the real content.

use std::{
    fs::{metadata, read_to_string},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

/// Terse fallback if `soul.md` is missing.
const DEFAULT_SOUL: &str = "You are Albert, a concise, helpful personal assistant. \
Reply in the user's language; keep it short and warm. Never invent a tool result — \
if a tool errors, say so honestly.";

/// One cached file: its last-seen mtime + body.
#[derive(Default)]
struct Slot {
    mtime: Option<SystemTime>,
    body: String,
}

#[derive(Default)]
struct Cache {
    soul: Slot,
    system: Slot,
}

/// Persona (`soul`) + operating instructions (`system`), RAM-cached, mtime-reloaded.
pub struct PromptFiles {
    soul: PathBuf,
    system: PathBuf,
    cache: Mutex<Cache>,
}

impl PromptFiles {
    pub fn new(soul: PathBuf, system: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            soul,
            system,
            cache: Mutex::new(Cache::default()),
        })
    }

    /// The composed base preamble (soul + system), reloading any changed file.
    pub fn base(&self) -> String {
        let mut c = self.cache.lock().unwrap();
        let soul = read_cached(&self.soul, &mut c.soul, DEFAULT_SOUL);
        let system = read_cached(&self.system, &mut c.system, "");
        if system.trim().is_empty() {
            soul
        } else {
            format!("{soul}\n\n{system}")
        }
    }
}

/// Return the file's body, re-reading only when its mtime changed; a missing or
/// unreadable file yields `default`.
fn read_cached(path: &Path, slot: &mut Slot, default: &str) -> String {
    match metadata(path).and_then(|m| m.modified()).ok() {
        Some(mt) if slot.mtime == Some(mt) => slot.body.clone(),
        Some(mt) => match read_to_string(path) {
            Ok(body) => {
                slot.mtime = Some(mt);
                slot.body = body;
                slot.body.clone()
            }
            Err(_) => default.to_string(),
        },
        None => default.to_string(),
    }
}
