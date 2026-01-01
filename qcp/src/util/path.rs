//! Path-related

use std::path::{MAIN_SEPARATOR, Path, PathBuf};

pub(crate) fn basename_of(path: &str) -> anyhow::Result<String> {
    let path = Path::new(path);
    let Some(filename) = path.file_name() else {
        anyhow::bail!("Source path \"{}\" must contain a filename", path.display());
    };
    let filename = filename.to_string_lossy();
    anyhow::ensure!(
        !filename.is_empty(),
        "Source path \"{}\" must contain a filename",
        path.display()
    );
    Ok(filename.to_string())
}

pub(crate) fn join_local(base: &str, leaf: &str) -> String {
    if base.is_empty() {
        return leaf.to_string();
    }
    PathBuf::from(base).join(leaf).to_string_lossy().to_string()
}

/// Join a remote path using forward slashes, independent of the client's OS.
///
/// This avoids emitting `\` on Windows clients when the remote host is Unix-like.
pub(crate) fn join_remote(base: &str, leaf: &str) -> String {
    if base.is_empty() {
        return leaf.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{leaf}")
    } else {
        format!("{base}/{leaf}")
    }
}

pub(crate) fn add_pathsep_if_needed(path: &mut String, local: bool) {
    let sep = if local { MAIN_SEPARATOR } else { '/' };
    if !path.is_empty() && !path.ends_with(sep) {
        path.push(sep);
    }
}
