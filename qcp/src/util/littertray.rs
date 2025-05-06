//! Filesystem helper for tests
// Copyright (c) 2020 Sergio Benitez, (c) 2025 Ross Younger
// MIT license applies to this file.

use std::fs::{self, File};
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use tempfile::TempDir;

/// This is a sort of "lightweight jail".
/// The process changes directory into the litter tray during execution, but is not well constrained.
/// On drop, the litter tray is automatically cleaned up.
///
/// This is a derivative work of `figment::Jail` but simpler (no environment variables) and supports async closures.
#[derive(Debug)]
pub(crate) struct LitterTray {
    canonical_dir: PathBuf,
    _dir: TempDir,
    saved_cwd: PathBuf,
}

/// This mutex ensures that only one test can use a litter tray at once.
/// Necessary because it changes the process working directory.
static G_LOCK: Mutex<()> = Mutex::new(());

impl LitterTray {
    /// Runs a closure in a new litter tray, passing the tray to the closure.
    /// The closure must return a Result<()>.
    ///
    /// ```
    /// use qcp::util::littertray::LitterTray;
    ///
    /// let result = LitterTray::try_with(|tray| {
    ///   let _ = tray.create_text("test.txt", "Hello, world!")?;
    ///   assert_eq!(std::fs::read_to_string("test.txt")?, "Hello, world!");
    ///   Ok(())
    /// }).unwrap();
    /// ```
    #[allow(dead_code)]
    pub(crate) fn try_with<F: FnOnce(&mut LitterTray) -> Result<()>>(f: F) -> Result<()> {
        let _guard = G_LOCK.lock();
        let dir = TempDir::new()?;
        let mut tray = LitterTray {
            canonical_dir: dir.path().canonicalize()?,
            _dir: dir,
            saved_cwd: std::env::current_dir()?,
        };
        std::env::set_current_dir(tray.directory())?;
        f(&mut tray)
    }

    /// Runs a closure in a new litter tray, passing the tray to the closure.
    ///
    /// This is a convenience function that does not return a Result.
    ///
    /// ```
    /// use qcp::util::littertray::LitterTray;
    ///
    /// let result = LitterTray::run(|tray| {
    ///   tray.create_text("test.txt", "Hello, world!").unwrap();
    ///   assert_eq!(std::fs::read_to_string("test.txt").unwrap(), "Hello, world!");
    /// });
    /// ```
    pub(crate) fn run<F: FnOnce(&mut LitterTray)>(f: F) {
        let _ = Self::try_with(|tray| {
            f(tray);
            Ok(())
        });
    }

    /// Runs an async closure in a new litter tray, passing the tray to the closure.
    /// The closure must return a Result<()>.
    pub(crate) async fn try_with_async<F: AsyncFnOnce(&mut LitterTray) -> Result<()>>(
        f: F,
    ) -> Result<()> {
        let _guard = G_LOCK.lock();
        let dir = TempDir::new()?;
        let mut tray = LitterTray {
            canonical_dir: dir.path().canonicalize()?,
            _dir: dir,
            saved_cwd: std::env::current_dir()?,
        };
        std::env::set_current_dir(tray.directory())?;
        f(&mut tray).await
    }

    /// Returns the temporary directory that is this litter tray.
    /// This directory will be removed on drop.
    #[must_use]
    pub(crate) fn directory(&self) -> &Path {
        &self.canonical_dir
    }

    fn safe_path_within_tray(&self, path: &Path) -> Result<PathBuf> {
        let path = dedot(path);
        if path.is_absolute() && path.starts_with(self.directory()) {
            return Ok(path);
        }
        anyhow::ensure!(
            path.is_relative(),
            "LitterTray: input path is outside of tray directory"
        );
        Ok(path)
    }

    /// Creates a binary file within the tray
    pub(crate) fn create_binary<P: AsRef<Path>>(&self, path: P, bytes: &[u8]) -> Result<File> {
        let path = self.safe_path_within_tray(path.as_ref())?;
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(bytes)?;
        Ok(writer.into_inner()?)
    }

    /// Creates a text file within the tray
    pub(crate) fn create_text<P: AsRef<Path>>(&self, path: P, contents: &str) -> Result<File> {
        self.create_binary(path, contents.as_bytes())
    }

    /// Creates a directory within the tray
    pub(crate) fn make_dir<P: AsRef<Path>>(&self, path: P) -> Result<PathBuf> {
        let path = self.safe_path_within_tray(path.as_ref())?;
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    #[cfg(unix)]
    /// Creates a symbolic link within the tray.
    /// Returns the path to the new symlink.
    pub(crate) fn make_symlink<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        original: P,
        link: Q,
    ) -> Result<PathBuf> {
        let path_orig = self.safe_path_within_tray(original.as_ref())?;
        let path_link = self.safe_path_within_tray(link.as_ref())?;
        std::os::unix::fs::symlink(path_orig, &path_link)?;
        Ok(path_link)
    }
}

impl Drop for LitterTray {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.saved_cwd);
    }
}

/// Remove any dots from the path by popping as needed.
fn dedot(path: &Path) -> PathBuf {
    use std::path::Component::*;

    let mut comps = vec![];
    for component in path.components() {
        match component {
            p @ Prefix(_) => comps = vec![p],
            r @ RootDir if comps.iter().all(|c| matches!(c, Prefix(_))) => comps.push(r),
            r @ RootDir => comps = vec![r],
            CurDir => {}
            ParentDir if comps.iter().all(|c| matches!(c, Prefix(_) | RootDir)) => {}
            ParentDir => {
                let _ = comps.pop();
            }
            c @ Normal(_) => comps.push(c),
        }
    }

    comps.iter().map(|c| c.as_os_str()).collect()
}
