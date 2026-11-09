//! Album art resolver, `$XDG_CACHE_HOME/islet/` cacher, and dominant color extractor.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use color_thief::{ColorFormat, get_palette};
use tracing::{debug, warn};

/// Resolves an `mpris:artUrl` (`file://` or `http(s)://`) to a local cached file path
/// in `$XDG_CACHE_HOME/islet/` and extracts its dominant accent RGB color.
pub async fn resolve_art_and_accent(art_url: &str) -> (Option<String>, Option<(u8, u8, u8)>) {
    let cache_dir = cache_directory();
    if let Err(err) = tokio::fs::create_dir_all(&cache_dir).await {
        warn!(%err, "Failed to create album art cache directory");
        return (None, None);
    }

    let local_path = if let Some(stripped) = art_url.strip_prefix("file://") {
        let decoded = percent_decode(stripped);
        let path = PathBuf::from(decoded);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    } else if art_url.starts_with("http://") || art_url.starts_with("https://") {
        fetch_remote_art(art_url, &cache_dir).await
    } else {
        None
    };

    let Some(path) = local_path else {
        return (None, None);
    };

    let path_clone = path.clone();
    let accent = tokio::task::spawn_blocking(move || extract_dominant_rgb(&path_clone))
        .await
        .ok()
        .flatten();

    let path_str = path.to_string_lossy().into_owned();
    debug!(path = %path_str, ?accent, "Resolved album art and dominant accent color");
    (Some(path_str), accent)
}

fn cache_directory() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("islet")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("islet")
    } else {
        PathBuf::from("/tmp/islet-cache")
    }
}

async fn fetch_remote_art(url: &str, cache_dir: &Path) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let target = cache_dir.join(format!("art_{:016x}.img", hasher.finish()));

    if target.exists() {
        return Some(target);
    }

    // Use curl CLI asynchronously so we avoid heavy TLS crates inside the daemon binary.
    let status = tokio::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "5", "-o"])
        .arg(&target)
        .arg(url)
        .status()
        .await
        .ok()?;

    if status.success() && target.exists() {
        Some(target)
    } else {
        None
    }
}

/// Extracts a vibrant dominant RGB tuple using `color-thief` on downsampled image pixels.
pub fn extract_dominant_rgb(path: &Path) -> Option<(u8, u8, u8)> {
    let img = image::open(path).ok()?;
    let rgb = img.thumbnail(64, 64).to_rgb8();
    let palette = get_palette(rgb.as_raw(), ColorFormat::Rgb, 10, 5).ok()?;
    let best = palette.first()?;
    Some((best.r, best.g, best.b))
}

/// Minimal percent-decoder for `file://` URIs containing spaces or UTF-8 characters.
#[must_use]
pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let (Some(hi), Some(lo)) = (from_hex(raw[i + 1]), from_hex(raw[i + 2])) {
                bytes.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

const fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
