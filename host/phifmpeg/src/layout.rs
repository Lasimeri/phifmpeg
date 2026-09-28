//! Where phifmpeg keeps sources, tools and build trees.
//!
//! Everything lives under one build root: `PHIFMPEG_BUILD` if set, else
//! `<repo>/build` (git-ignored; a symlink to a larger disk works). The root
//! must not contain whitespace: FFmpeg's configure and make expand paths
//! unquoted.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// Resolved build-root layout.
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    /// Resolve the build root and refuse a path with whitespace.
    pub fn new(repo: &Path) -> Result<Layout> {
        let root = match std::env::var_os("PHIFMPEG_BUILD") {
            Some(p) => PathBuf::from(p),
            None => repo.join("build"),
        };
        if root.to_string_lossy().chars().any(char::is_whitespace) {
            bail!(
                "build root {} contains whitespace; set PHIFMPEG_BUILD to a path without any",
                root.display()
            );
        }
        Ok(Layout { root })
    }

    /// The pristine FFmpeg checkout.
    pub fn ffmpeg_src(&self) -> PathBuf {
        self.root.join("src/ffmpeg")
    }

    /// The pristine x265 checkout.
    pub fn x265_src(&self) -> PathBuf {
        self.root.join("src/x265")
    }

    /// x265 installed for one variant (`lib/libx265.a`, `include/`, `lib/pkgconfig`).
    pub fn x265_prefix(&self, variant: &str) -> PathBuf {
        self.root.join("prefix").join(variant)
    }

    /// Downloaded tarballs.
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// Unpacked tarball sources (nasm); the git checkouts have their own.
    pub fn src(&self, name: &str) -> PathBuf {
        self.root.join("src").join(name)
    }

    /// Host tools built here (nasm), installed as `<tools>/bin/...`.
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// An out-of-tree build directory: `c`, `host` for FFmpeg,
    /// `x265-<variant>` for x265.
    pub fn variant(&self, name: &str) -> PathBuf {
        self.root.join("build").join(name)
    }
}
