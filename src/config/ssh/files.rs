//! File parsing internals
// (c) 2024 Ross Younger

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use figment::Figment;
use tracing::warn;

use super::{evaluate_host_match, find_include_files, split_args, Line, Setting, ValueProvider};

/// The result of parsing an ssh-style configuration file, with a particular host in mind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostConfiguration {
    /// The host we were interested in. If None, this is "unspecified", i.e. we return data in `Host *` sections or in an unqualified section at the top of the file.
    host: Option<String>,
    /// If present, this is the file we read
    source: Option<PathBuf>,
    /// Output data
    data: BTreeMap<String, Setting>,
}

impl HostConfiguration {
    fn new(host: Option<&str>, source: Option<PathBuf>) -> Self {
        Self {
            host: host.map(std::borrow::ToOwned::to_owned),
            source,
            data: BTreeMap::default(),
        }
    }
    pub(crate) fn get(&self, key: &str) -> Option<&Setting> {
        self.data.get(key)
    }

    pub(crate) fn as_figment(&self) -> Figment {
        let mut figment = Figment::new();
        let profile = self
            .host
            .as_deref()
            .map_or(figment::Profile::Default, figment::Profile::new);
        for (k, v) in &self.data {
            figment = figment.merge(ValueProvider::new(k, v, &profile));
        }
        figment
    }
}

///////////////////////////////////////////////////////////////////////////////////////

/// The business end of reading a config file.
///
/// # Note
/// You can only use this struct once. If for some reason you want to re-parse a file,
/// you must create a fresh `Parser` to do so.
pub(crate) struct Parser<R>
where
    R: Read,
{
    line_number: usize,
    reader: BufReader<R>,
    source: String,
    path: Option<PathBuf>,
    is_user: bool,
}

impl Parser<File> {
    pub(crate) fn for_path<P>(path: P, is_user: bool) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::for_reader(
            reader,
            path.to_string_lossy().to_string(),
            Some(path.to_path_buf()),
            is_user,
        ))
    }
}

impl<'a> Parser<&'a [u8]> {
    fn for_str(s: &'a str, is_user: bool) -> Self {
        Self::for_reader(
            BufReader::new(s.as_bytes()),
            "<string>".into(),
            None,
            is_user,
        )
    }
}

impl Default for Parser<&[u8]> {
    fn default() -> Self {
        Parser::for_str("", false)
    }
}

impl<R: Read> Parser<R> {
    fn for_reader(
        reader: BufReader<R>,
        source: String,
        path: Option<PathBuf>,
        is_user: bool,
    ) -> Self {
        Self {
            line_number: 0,
            reader,
            source,
            path,
            is_user,
        }
    }

    fn parse_line(&self, line: &str) -> Result<Line> {
        let line = line.trim();
        let line_number = self.line_number;
        // extract keyword, which may be delimited by whitespace (Key Value) OR equals (Key=Value)
        let (keyword, rest) = {
            let mut splitter = line.splitn(2, &[' ', '\t', '=']);
            let keyword = match splitter.next() {
                None | Some("") => return Ok(Line::Empty),
                Some(kw) => kw.to_lowercase(),
            };
            (keyword, splitter.next().unwrap_or_default())
        };
        if keyword.starts_with('#') {
            return Ok(Line::Empty);
        }
        let args = split_args(rest).with_context(|| format!("at line {line_number}"))?;
        anyhow::ensure!(!args.is_empty(), "missing argument at line {line_number}");

        Ok(match keyword.as_str() {
            "host" => Line::Host { line_number, args },
            "match" => Line::Match { line_number, args },
            "include" => Line::Include { line_number, args },
            _ => Line::Generic {
                line_number,
                keyword,
                args,
            },
        })
    }

    const INCLUDE_DEPTH_LIMIT: u8 = 16;

    fn parse_file_inner(
        &mut self,
        accepting: &mut bool,
        depth: u8,
        output: &mut HostConfiguration,
    ) -> Result<()> {
        let mut line = String::new();
        anyhow::ensure!(
            depth < Self::INCLUDE_DEPTH_LIMIT,
            "too many nested includes"
        );

        loop {
            line.clear();
            self.line_number += 1;
            let mut line = String::new();
            if 0 == self.reader.read_line(&mut line)? {
                break; // EOF
            }
            match self.parse_line(&line)? {
                Line::Empty => (),
                Line::Host { args, .. } => {
                    *accepting = evaluate_host_match(output.host.as_deref(), &args);
                }
                Line::Match { .. } => {
                    warn!("match expressions in ssh_config files are not yet supported");
                }
                Line::Include { args, .. } => {
                    for arg in args {
                        let files = find_include_files(&arg, self.is_user)?;
                        for f in files {
                            let mut subparser =
                                Parser::for_path(f, self.is_user).with_context(|| {
                                    format!(
                                        "Include directive at {} line {}",
                                        self.source, self.line_number
                                    )
                                })?;
                            subparser.parse_file_inner(accepting, depth + 1, output)?;
                        }
                    }
                }
                Line::Generic { keyword, args, .. } => {
                    if *accepting {
                        // per ssh_config(5), the first matching entry for a given key wins.
                        let _ = output.data.entry(keyword).or_insert_with(|| Setting {
                            source: self.source.clone(),
                            line_number: self.line_number,
                            args,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Interprets the source with a given hostname in mind.
    /// This consumes the `Parser`.
    pub(crate) fn parse_file_for(mut self, host: Option<&str>) -> Result<HostConfiguration> {
        let mut output = HostConfiguration::new(host, self.path.take());
        let mut accepting = true;
        self.parse_file_inner(&mut accepting, 0, &mut output)?;
        Ok(output)
    }
}

///////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use anyhow::{anyhow, Context, Result};
    use assertables::{assert_contains, assert_contains_as_result, assert_eq_as_result};

    use super::super::Line;
    use super::Parser;

    use crate::{
        os::{AbstractPlatform, Platform},
        util::make_test_tempfile,
    };

    macro_rules! assert_1_arg {
        ($left:expr, $right:expr) => {
            assert_eq!(($left).unwrap().args.first().unwrap(), $right);
        };
    }

    macro_rules! make_vec {
        ($v:expr) => {
            $v.into_iter().map(|s| s.into()).collect()
        };
    }

    fn host_(args: Vec<&str>) -> Line {
        Line::Host {
            line_number: 0,
            args: make_vec!(args),
        }
    }
    fn match_(args: Vec<&str>) -> Line {
        Line::Match {
            line_number: 0,
            args: make_vec!(args),
        }
    }
    fn include_(args: Vec<&str>) -> Line {
        Line::Include {
            line_number: 0,
            args: make_vec!(args),
        }
    }
    fn generic_(kw: &str, args: Vec<&str>) -> Line {
        Line::Generic {
            line_number: 0,
            keyword: kw.into(),
            args: make_vec!(args),
        }
    }

    #[test]
    fn line_parsing() -> Result<()> {
        let p = Parser::default();
        for (input, expected) in [
            ("", Line::Empty),
            (" # foo", Line::Empty),
            ("Foo Bar", generic_("foo", vec!["Bar"])),
            ("Foo Bar baz", generic_("foo", vec!["Bar", "baz"])),
            ("Foo \"Bar baz\"", generic_("foo", vec!["Bar baz"])),
            ("Foo=bar", generic_("foo", vec!["bar"])),
            ("Host a b", host_(vec!["a", "b"])),
            ("Match a b", match_(vec!["a", "b"])),
            ("iNcluDe c d", include_(vec!["c", "d"])),
            (
                "QUOTED \"abc def\" ghi",
                generic_("quoted", vec!["abc def", "ghi"]),
            ),
        ] {
            let msg = || format!("input \"{input}\" failed");
            assert_eq_as_result!(p.parse_line(input).with_context(msg)?, expected)
                .map_err(|e| anyhow!(e))
                .with_context(msg)?;
        }
        for (input, expected_msg) in [
            ("aaa bbb \" ccc", "unterminated quote"),
            ("aaa", "missing argument"),
        ] {
            let err = p.parse_line(input).unwrap_err();
            assert_contains_as_result!(err.root_cause().to_string(), expected_msg)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("input \"{input}\" failed"))?;
        }
        Ok(())
    }

    #[test]
    fn defaults_without_host_block() {
        let output = Parser::for_str(
            r"
            Foo Bar
            Baz Qux
            # foop is a comment
        ",
            true,
        )
        .parse_file_for(None)
        .unwrap();
        //println!("{output:?}");
        assert_1_arg!(output.get("foo"), "Bar");
        assert_1_arg!(output.get("baz"), "Qux");
        assert_eq!(output.get("foop"), None);
    }

    #[test]
    fn host_block_simple() {
        let output = Parser::for_str(
            r"
            Host Fred
            Foo Bar
            Host Barney
            Foo Baz
        ",
            true,
        )
        .parse_file_for(Some("Fred"))
        .unwrap();
        assert_1_arg!(output.get("foo"), "Bar");
    }

    #[test]
    fn earlier_match_wins() {
        let output = Parser::for_str(
            r"
            Host Fred
            Foo Bar
            Host Barney
            Foo Baz
            Host Fred
            Foo Qux
            Host *
            Foo Qix
        ",
            true,
        )
        .parse_file_for(Some("Fred"))
        .unwrap();
        assert_1_arg!(output.get("foo"), "Bar");
    }

    #[test]
    fn later_default_works() {
        let output = Parser::for_str(
            r"
            Host Fred
            Foo Bar
            Host Barney
            Foo Baz
            Host *
            Qux Qix
        ",
            true,
        )
        .parse_file_for(Some("Fred"))
        .unwrap();
        assert_1_arg!(output.get("qux"), "Qix");
    }

    #[test]
    fn read_real_file() {
        let (path, _dir) = make_test_tempfile(
            r"
            hi there
        ",
            "test.conf",
        );
        let output = Parser::for_path(path, true)
            .unwrap()
            .parse_file_for(None)
            .unwrap();
        assert_1_arg!(output.get("hi"), "there");
    }

    #[test]
    fn recursion_limit() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test-recursion");
        let contents = format!(
            "
            include {path:?}
        "
        );
        std::fs::write(&path, contents).unwrap();
        let err = Parser::for_path(path, true)
            .unwrap()
            .parse_file_for(None)
            .unwrap_err();
        assert_contains!(err.to_string(), "too many nested includes");
    }

    #[test]
    fn expand_globs() {
        let tempdir = tempfile::tempdir().unwrap();
        let path1 = tempdir.path().join("test1");
        let path2 = tempdir.path().join("other2");
        let path3 = tempdir.path().join("other3");
        let glob = tempdir.path().join("oth*");
        std::fs::write(&path1, format!("include {glob:?}")).unwrap();
        std::fs::write(&path2, "hi there").unwrap();
        std::fs::write(&path3, "green cheese").unwrap();
        let output = Parser::for_path(path1, true)
            .unwrap()
            .parse_file_for(None)
            .unwrap();
        assert_1_arg!(output.get("hi"), "there");
        assert_1_arg!(output.get("green"), "cheese");
    }

    #[test]
    #[ignore]
    fn dump_local_config() {
        let path = Platform::user_ssh_config().unwrap();
        let parser = Parser::for_path(path, true).unwrap();
        let data = parser.parse_file_for(Some("lapis")).unwrap();
        println!("{data:#?}");
    }
}
