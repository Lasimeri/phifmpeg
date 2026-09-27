//! `pins.toml`: every download, pinned.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// The whole file.
#[derive(Deserialize)]
pub struct Pins {
    pub ffmpeg: GitPin,
    pub nasm: TarPin,
}

/// A git repository pinned to a tag and the commit that tag must peel to.
#[derive(Deserialize)]
pub struct GitPin {
    pub repo: String,
    pub tag: String,
    pub commit: String,
}

/// A tarball pinned by SHA-256.
#[derive(Deserialize)]
pub struct TarPin {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

impl Pins {
    /// Read `<repo>/pins.toml`.
    pub fn load(repo: &Path) -> Result<Pins> {
        let p = repo.join("pins.toml");
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))
    }
}
