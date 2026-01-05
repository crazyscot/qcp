//! Directory recursion
// (c) 2025 Ross Younger

use std::{
    ffi::{OsStr, OsString},
    io::ErrorKind,
    path::MAIN_SEPARATOR,
};

use crate::{CopyJobSpec, FileSpec, util::path};

use tracing::error;
use walkdir::WalkDir;

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

trait OsStrJoin {
    fn join_os_str<Sep>(&self, separator: Sep) -> OsString
    where
        Sep: AsRef<OsStr>;
}

impl<S> OsStrJoin for [S]
where
    S: AsRef<OsStr>,
{
    fn join_os_str<Sep>(&self, separator: Sep) -> OsString
    where
        Sep: AsRef<OsStr>,
    {
        let mut buffer = OsString::new();
        let separator = separator.as_ref();

        for (i, item) in self.iter().enumerate() {
            if i > 0 {
                buffer.push(separator);
            }
            buffer.push(item.as_ref());
        }
        buffer
    }
}

trait LocalToString {
    fn to_string_checked(&self) -> anyhow::Result<String>;
}

impl<S> LocalToString for S
where
    S: AsRef<OsStr>,
{
    fn to_string_checked(&self) -> anyhow::Result<String> {
        let s1 = self.as_ref().to_str();
        let Some(s2) = s1 else {
            anyhow::bail!("Item {s1:?} could not be converted into Unicode string");
        };
        Ok(s2.to_string())
    }
}

/// Resolves a single `FileSpec`
///
/// Returns:
/// - Ok(true) on success
/// - Ok(false) on partial success
/// - Err(...) on fatal error
pub(crate) fn recurse_local_source(
    source: &FileSpec,
    destination: &FileSpec,
    preserve: bool,
    output: &mut Vec<CopyJobSpec>,
) -> Result<bool, Error> {
    if destination.user_at_host.is_none() {
        return Err(anyhow::anyhow!("destination must be remote").into());
    }

    let bare_host = destination.filename.is_empty();
    let mut success = true;

    // Join a remote path using forward slashes, independent of the client's OS.
    // This avoids emitting `\` on Windows clients when the remote host is Unix-like.
    let dest_separator_char = '/';
    let dest_separator_str = String::from(dest_separator_char);
    let local_separator_char = MAIN_SEPARATOR;

    let dest_stem = if destination.filename.ends_with('/') {
        // Destination is a directory. Therefore, we put the source directory _into_ the destination.
        let mut buf = destination.filename.clone();
        let mut iter = source.filename.split(local_separator_char);
        let mut source_dir = iter.next_back();
        while let Some(d) = source_dir
            && d.is_empty()
        {
            // Remove any trailing slash in the source directory
            source_dir = iter.next_back();
        }

        if let Some(s) = source_dir {
            buf.push_str(s);
        } else {
            // The destination filename is either the root ('/') or the bare host ('')
        }
        buf
    } else {
        destination.filename.clone()
    };

    let (success1, listing) = contents_of(&source.filename, bare_host, &dest_separator_str)?;
    success &= success1;
    for (entry, leaf_str) in listing {
        let file_type = entry.file_type();
        let path = entry.path();
        let Some(src_str) = path.to_str() else {
            error!(
                "Path name {} could not be converted into Unicode string",
                path.display()
            );
            success = false;
            continue;
        };

        let src_fs = FileSpec {
            user_at_host: source.user_at_host.clone(),
            filename: src_str.to_string(),
        };
        let dest_fs = FileSpec {
            user_at_host: destination.user_at_host.clone(),
            filename: path::join_remote(&dest_stem, &leaf_str),
        };
        output.push(
            CopyJobSpec::try_new(src_fs, dest_fs, preserve, file_type.is_dir())
                .map_err(Error::from)?,
        );
    }
    Ok(success)
}

/// Returns:
/// Ok(true, vec) on full success
/// Ok(false, vec) on partial success
/// Err(...) on fatal error
fn contents_of(
    path: &str,
    skip_root: bool,
    separator: &str,
) -> Result<(bool, Vec<(walkdir::DirEntry, String)>), Error> {
    let mut output = vec![];
    let mut success = true;
    for entry in WalkDir::new(path)
        // skip_root true => min_depth 1; false => min_depth 0
        .min_depth(usize::from(skip_root))
        .follow_links(true)
    {
        match entry {
            Ok(entry) => {
                // Walkdir gives us the path including the recursion root.
                // We need the path relative to the recursion root.
                let depth = entry.depth();
                let path = entry.path();

                let n_strip = path.iter().count() - depth;
                let leaf = path
                    .components()
                    .skip(n_strip)
                    .map(std::path::Component::as_os_str)
                    .collect::<Vec<_>>()
                    .join_os_str(OsStr::new(separator));
                match leaf.to_string_checked() {
                    Ok(leaf_str) => output.push((entry, leaf_str)),
                    Err(e) => {
                        error!("{e}");
                        success = false;
                    }
                }
            }

            Err(wderr) => {
                if let Some(ioe) = wderr.io_error()
                    && ioe.kind() == ErrorKind::PermissionDenied
                {
                    error!("{ioe}");
                    success = false;
                    continue;
                }
                return Err(wderr.into());
            }
        }
    }
    Ok((success, output))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use core::iter::Iterator;
    use std::{path::PathBuf, str::FromStr};

    use crate::{CopyJobSpec, FileSpec, util::dirwalk::LocalToString as _};

    use anyhow::Result;
    use littertray::LitterTray;

    fn filespec_local<S: AsRef<str>>(f: S) -> FileSpec {
        FileSpec {
            user_at_host: None,
            filename: f.as_ref().to_string(),
        }
    }

    fn check_flatten(
        v: &[CopyJobSpec],
        expected_user_at_host: Option<&String>,
        source: bool,
    ) -> Vec<String> {
        let mut r: Vec<_> = v
            .iter()
            .map(|js| {
                let uut = if source { &js.source } else { &js.destination };
                assert_eq!(
                    uut.user_at_host.as_deref(),
                    expected_user_at_host.map(std::string::String::as_str)
                );
                uut.filename.clone()
            })
            .collect();
        r.sort();
        r
    }

    fn setup_fs(tray: &mut LitterTray) -> Result<()> {
        let _ = tray.create_text("file1", "1")?;
        let _ = tray.create_text("file2", "2")?;
        let _ = tray.create_text("file22", "2")?;
        let _ = tray.create_text("file3", "3")?;
        let _ = tray.create_text("file333", "3")?;
        let _ = tray.create_text("FILE4", "4")?;
        let _ = tray.make_dir("dir1")?;
        let _ = tray.create_text("dir1/midlevelfile", "mid")?;
        let _ = tray.make_dir("dir1/a")?;
        let _ = tray.create_text("dir1/a/f", "f")?;
        let _ = tray.make_dir("dir1/a/z")?;
        let _ = tray.create_text("dir1/a/z/q1", "x")?;
        let _ = tray.create_text("dir1/a/z/q2", "y")?;
        let _ = tray.make_dir("dir1/b")?;
        let _ = tray.create_text("dir1/b/.dotfile", "sneaky")?;
        let _ = tray.create_text("dir1/b/afile", "hi")?;
        Ok(())
    }

    // canonicalises, so we can write Unix-style expectations and have them work on Windows
    fn check_output<S: AsRef<str>>(a: &[String], b: &[S]) {
        let aa = a.iter().map(|it| it.replace('\\', "/")).collect::<Vec<_>>();
        let bb = b
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>();
        assert_eq!(aa, bb);
    }

    fn test_case(
        setup: impl FnOnce(&mut LitterTray) -> Result<()>,
        source: &str,
        dest: Option<&str>,
        expected_sources: &[&str],
        expected_success: bool,
    ) {
        let source_fs = filespec_local(source);
        // note that the default destination does NOT have a trailing slash
        let destination = FileSpec::from_str(dest.unwrap_or("destuser@desthost:destdir")).unwrap();
        let bare_host = destination.filename.is_empty();
        let trailing_slash = dest.is_some_and(|d| d.ends_with('/'));
        let path_insert = if trailing_slash {
            // In trailing-slash mode, the source directory goes _into_ the destination directory.
            // (Without a trailing slash, the *contents* of the source dir go into the dest dir.)
            // Therefore we expect to see the last component of the source path in the output at the relevant place.
            let p = PathBuf::from(source);
            p.components()
                .next_back()
                .unwrap()
                .as_os_str()
                .to_string_checked()
                .unwrap()
        } else {
            String::new()
        };

        let res = LitterTray::try_with(|tray| {
            setup(tray)?;
            let mut out = Vec::new();
            let ok = super::recurse_local_source(&source_fs, &destination, false, &mut out)?;
            assert_eq!(expected_success, ok);
            Ok(out)
        })
        .unwrap();
        let sources = check_flatten(&res, source_fs.user_at_host.as_ref(), true);
        check_output(&sources, expected_sources);
        let dests = check_flatten(&res, destination.user_at_host.as_ref(), false);
        let source_strip = {
            let mut t = source.to_string();
            if bare_host && !t.ends_with('/') {
                t.push('/');
            }
            t
        };

        let expected_dests = expected_sources
            .iter()
            .map(|input| {
                let mut s = destination.filename.clone();
                s.push_str(&path_insert);
                let leaf = (*input).strip_prefix(&source_strip).unwrap();
                if !bare_host && !s.ends_with('/') && !leaf.starts_with('/') {
                    s.push('/');
                }
                s.push_str(leaf);
                s
            })
            .collect::<Vec<_>>();
        check_output(&dests, &expected_dests);
    }

    #[test]
    fn recurse() {
        test_case(
            setup_fs,
            "dir1",
            None,
            &[
                "dir1",
                "dir1/a",
                "dir1/a/f",
                "dir1/a/z",
                "dir1/a/z/q1",
                "dir1/a/z/q2",
                "dir1/b",
                "dir1/b/.dotfile",
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
            true,
        );
    }
    #[test]
    fn recurse_to_bare_host() {
        test_case(
            setup_fs,
            "dir1",
            Some("host:"),
            &[
                // no destination directory
                "dir1/a",
                "dir1/a/f",
                "dir1/a/z",
                "dir1/a/z/q1",
                "dir1/a/z/q2",
                "dir1/b",
                "dir1/b/.dotfile",
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
            true,
        );
    }
    #[test]
    fn recurse_to_abs_path() {
        test_case(
            setup_fs,
            "dir1",
            Some("host:/outdir"),
            &[
                "dir1",
                "dir1/a",
                "dir1/a/f",
                "dir1/a/z",
                "dir1/a/z/q1",
                "dir1/a/z/q2",
                "dir1/b",
                "dir1/b/.dotfile",
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
            true,
        );
    }
    #[test]
    fn recurse_files_only() {
        test_case(
            setup_fs,
            "dir1",
            None,
            &[
                "dir1",
                "dir1/a",
                "dir1/a/f",
                "dir1/a/z",
                "dir1/a/z/q1",
                "dir1/a/z/q2",
                "dir1/b",
                "dir1/b/.dotfile",
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
            true,
        );
    }

    #[test]
    fn recurse_trailing_slash() {
        test_case(
            setup_fs,
            "dir1",
            Some("dest:dir2/"),
            &[
                "dir1",
                "dir1/a",
                "dir1/a/f",
                "dir1/a/z",
                "dir1/a/z/q1",
                "dir1/a/z/q2",
                "dir1/b",
                "dir1/b/.dotfile",
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
            true,
        );
    }

    #[cfg(unix)]
    fn setup_fs_with_inaccessibles(tray: &mut LitterTray) -> Result<()> {
        use std::fs::{Permissions, set_permissions};
        use std::os::unix::fs::PermissionsExt as _;
        let _ = tray.make_dir("okdir")?;
        let _ = tray.create_text("okdir/okfile", "ok")?;

        let _ = tray.make_dir("unreadable_dir")?;
        let _ = tray.create_text("unreadable_dir/really", "nope")?;
        set_permissions("unreadable_dir", Permissions::from_mode(0o111))?;

        let _ = tray.create_text("unreadable_file", "nope")?;
        set_permissions("unreadable_file", Permissions::from_mode(0o0))?;
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn recurse_inaccessible_dir() {
        test_case(
            setup_fs_with_inaccessibles,
            ".",
            None,
            &[
                ".",
                "./okdir",
                "./okdir/okfile",
                "./unreadable_dir",
                "./unreadable_file",
            ],
            false,
        );
    }
}
