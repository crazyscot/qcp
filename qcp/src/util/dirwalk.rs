//! Directory recursion
// (c) 2025 Ross Younger

use std::{
    ffi::{OsStr, OsString},
    io::ErrorKind,
    path::{MAIN_SEPARATOR, Path},
};

use crate::{CopyJobSpec, FileSpec, util::path};

use enumflags2::{BitFlags, bitflags};
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

/// Option flags for a resolve operation.
#[bitflags]
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Options {
    // TODO: Move these to protocol
    /// Do not error out if we are unable to open a directory
    IgnoreUnreadableDirectories = 1 << 0,
    /// Include file/directory names beginning with `.` (except for `.` and `..`) in the output
    IncludeHiddenItems = 1 << 1,
}

impl Options {
    pub(crate) const EMPTY: BitFlags<Self> = BitFlags::EMPTY;
}

fn is_suppressed<P: AsRef<Path>>(path: P, options: BitFlags<Options>) -> bool {
    if let Some(name) = path.as_ref().file_name() {
        !options.contains(Options::IncludeHiddenItems)
            && name.to_str().is_some_and(|s| s.starts_with('.'))
    } else {
        false
    }
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
pub(crate) fn recurse_local_source(
    source: &FileSpec,
    destination: &FileSpec,
    options: BitFlags<Options>,
    preserve: bool,
    output: &mut Vec<CopyJobSpec>,
) -> Result<(), Error> {
    if destination.user_at_host.is_none() {
        return Err(anyhow::anyhow!("destination must be remote").into());
    }

    let bare_host = destination.filename.is_empty();

    // Join a remote path using forward slashes, independent of the client's OS.
    // This avoids emitting `\` on Windows clients when the remote host is Unix-like.
    // TODO: swap MAIN_SEPARATOR_STR and & if remote source, local dest.
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

    let listing = contents_of(&source.filename, options, bare_host, &dest_separator_str)?;
    for (entry, leaf_str) in listing {
        let file_type = entry.file_type();
        let path = entry.path();
        let Some(src_str) = path.to_str() else {
            return Err(anyhow::anyhow!(
                "Path name {} could not be converted into Unicode string",
                path.display()
            )
            .into());
        };

        let src_fs = FileSpec {
            user_at_host: source.user_at_host.clone(),
            filename: src_str.to_string(),
        };
        let dest_fs = FileSpec {
            user_at_host: destination.user_at_host.clone(),
            // TODO: use join_local instead of join_remote for remote source / local dest
            filename: path::join_remote(&dest_stem, &leaf_str),
        };
        output.push(
            CopyJobSpec::try_new(src_fs, dest_fs, preserve, file_type.is_dir())
                .map_err(Error::from)?,
        );
    }
    Ok(())
}

fn contents_of(
    path: &str,
    options: BitFlags<Options>,
    skip_root: bool,
    separator: &str,
) -> Result<Vec<(walkdir::DirEntry, String)>, Error> {
    let mut output = vec![];
    for entry in WalkDir::new(path)
        // skip_root true => min_depth 1; false => min_depth 0
        .min_depth(usize::from(skip_root))
        .into_iter()
        .filter_entry(|f| !is_suppressed(f.path(), options))
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
                let leaf_str = leaf.to_string_checked()?;
                output.push((entry, leaf_str));
            }

            Err(wderr) => {
                if let Some(ioe) = wderr.io_error()
                    && ioe.kind() == ErrorKind::PermissionDenied
                    && options.contains(Options::IgnoreUnreadableDirectories)
                {
                    continue;
                }
                return Err(wderr.into());
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use core::iter::Iterator;
    use std::{path::PathBuf, str::FromStr};

    use crate::{CopyJobSpec, FileSpec, util::dirwalk::LocalToString as _};

    use super::Options;
    use anyhow::Result;
    use enumflags2::{BitFlags, make_bitflags};
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
        options: BitFlags<Options>,
        source: &str,
        dest: Option<&str>,
        expected_sources: &[&str],
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
            super::recurse_local_source(&source_fs, &destination, options, false, &mut out)?;
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
            Options::EMPTY,
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
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
        );
    }
    #[test]
    fn recurse_to_bare_host() {
        test_case(
            setup_fs,
            Options::EMPTY,
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
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
        );
    }
    #[test]
    fn recurse_to_abs_path() {
        test_case(
            setup_fs,
            Options::EMPTY,
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
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
        );
    }
    #[test]
    fn recurse_files_only() {
        test_case(
            setup_fs,
            Options::EMPTY,
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
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
        );
    }
    #[test]
    fn recurse_files_inc_dotfiles() {
        test_case(
            setup_fs,
            make_bitflags!(Options::IncludeHiddenItems),
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
        );
    }

    #[test]
    fn recurse_trailing_slash() {
        test_case(
            setup_fs,
            Options::EMPTY,
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
                "dir1/b/afile",
                "dir1/midlevelfile",
            ],
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
            make_bitflags!(Options::IgnoreUnreadableDirectories),
            ".",
            None,
            &[
                ".",
                "./okdir",
                "./okdir/okfile",
                "./unreadable_dir",
                "./unreadable_file",
            ],
        );
    }
}
