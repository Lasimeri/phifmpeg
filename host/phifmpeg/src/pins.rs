//! `pins.toml`: every download, pinned.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// The whole file.
#[derive(Deserialize)]
pub struct Pins {
    pub ffmpeg: GitPin,
    pub x265: GitPin,
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

#[cfg(test)]
mod tests {
    use super::Pins;
    use std::path::Path;

    /// The repository's own pins parse, and every pin is complete.
    #[test]
    fn repository_pins() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let p = Pins::load(&repo).unwrap();
        for g in [&p.ffmpeg, &p.x265] {
            assert_eq!(g.commit.len(), 40, "{} commit", g.repo);
            assert!(g.commit.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(!g.tag.is_empty());
        }
        assert_eq!(p.nasm.sha256.len(), 64);
        assert!(p.nasm.url.ends_with(".tar.xz"));
    }
}
