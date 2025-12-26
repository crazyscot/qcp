//! Directory recursion
// (c) 2025 Ross Younger

use std::{io::ErrorKind, path::Path};

use crate::FileSpec;

use enumflags2::{BitFlags, bitflags};
use walkdir::WalkDir;

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error(transparent)]
    StdIo(#[from] std::io::Error),
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
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

fn add_unless_filtered(path: &Path, options: BitFlags<Options>, output: &mut Vec<FileSpec>) {
    if !options.contains(Options::IncludeHiddenItems)
        && let Some(name) = path.file_name()
        && name.to_str().is_some_and(|s| s.starts_with('.'))
    {
        return;
    }
    output.push(FileSpec::from(path));
}

/// Resolves a single `FileSpec`.
/// If we did nothing, the `FileSpec` is added to the output vector as-is.
pub(crate) fn resolve_one(
    spec: &FileSpec,
    options: BitFlags<Options>,
    output: &mut Vec<FileSpec>,
) -> Result<(), Error> {
    for entry in WalkDir::new(&spec.filename) {
        match entry {
            Ok(it) => add_unless_filtered(it.path(), options, output),
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
    Ok(())
}

/// Walks a list of `FileSpec` and resolves all of them.
pub(crate) fn resolve_all(
    specs: &[FileSpec],
    options: BitFlags<Options>,
    output: &mut Vec<FileSpec>,
) -> Result<(), Error> {
    // maybe someday: deduplicate
    for spec in specs {
        resolve_one(spec, options, output)?;
    }
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use core::iter::Iterator;

    use crate::FileSpec;

    use super::{Options, resolve_all, resolve_one};
    use anyhow::Result;
    use enumflags2::{BitFlags, make_bitflags};
    use littertray::LitterTray;

    fn filespec_local<S: AsRef<str>>(f: S) -> FileSpec {
        FileSpec {
            user_at_host: None,
            filename: f.as_ref().to_string(),
        }
    }
    fn resolve_one_collect(
        spec: &FileSpec,
        options: BitFlags<Options>,
    ) -> anyhow::Result<Vec<FileSpec>> {
        let mut out = Vec::new();
        resolve_one(spec, options, &mut out)?;
        Ok(out)
    }
    fn resolve_all_collect(
        specs: &[FileSpec],
        options: BitFlags<Options>,
    ) -> anyhow::Result<Vec<FileSpec>> {
        let mut out = Vec::new();
        resolve_all(specs, options, &mut out)?;
        Ok(out)
    }

    fn flatten(v: &[FileSpec]) -> Vec<String> {
        let mut r: Vec<_> = v.iter().map(|fs| fs.filename.clone()).collect();
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

    // canonicaliser, so we can write Unix-style expectations and have them work on Windows
    fn check_output(a: Vec<String>, b: &[&str]) {
        let aa = a
            .into_iter()
            .map(|it| it.replace('\\', "/"))
            .collect::<Vec<_>>();
        assert_eq!(aa, b);
    }

    fn test_case(options: BitFlags<Options>, pattern: &str, expected: &[&str]) {
        let res = LitterTray::try_with(|tray| {
            setup_fs(tray)?;
            resolve_one_collect(&filespec_local(pattern), options)
        })
        .unwrap();
        let res = flatten(&res);
        check_output(res, expected);
    }
    fn recurse_test_case(pattern: &str, expected: &[&str]) {
        test_case(BitFlags::default(), pattern, expected);
    }

    #[test]
    fn recurse_all() {
        recurse_test_case(
            "dir1",
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
        recurse_test_case(
            "dir1",
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
            make_bitflags!(Options::IncludeHiddenItems),
            "dir1",
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
        let res = LitterTray::try_with(|tray| {
            setup_fs_with_inaccessibles(tray)?;
            let e = resolve_one_collect(&filespec_local("."), BitFlags::default())
                .expect_err("should have failed with permission denied");
            assert!(e.to_string().contains("Permission denied"));
            resolve_one_collect(
                &filespec_local("."),
                make_bitflags!(Options::IgnoreUnreadableDirectories),
            )
        })
        .unwrap();
        let res = flatten(&res);
        check_output(
            res,
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
