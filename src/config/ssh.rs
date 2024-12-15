//! Config file parsing, openssh-style
// (c) 2024 Ross Younger

use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use glob::{glob_with, MatchOptions};
use tracing::warn;

#[derive(Debug, Clone, PartialEq)]
/// A parsed line we read from an ssh config file
enum Line {
    Empty,
    Host {
        line_number: usize,
        args: Vec<String>,
    },
    Match {
        line_number: usize,
        args: Vec<String>,
    },
    Include {
        line_number: usize,
        args: Vec<String>,
    },
    Generic {
        line_number: usize,
        keyword: String, /*lowercase!*/
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
/// A setting we read from a config file
pub(crate) struct Setting {
    /// where the value came from
    pub source: String,
    /// line number within the value
    pub line_number: usize,
    /// the setting data itself (not parsed; we assert nothing beyond the parser has applied the ssh quoting logic)
    pub args: Vec<String>,
}

impl Setting {
    pub(crate) fn first_arg(&self) -> String {
        self.args.first().cloned().unwrap_or_else(String::new)
    }
}

/// Splits a string into a list of arguments.
/// Arguments are delimited by whitespace, subject to quoting (single or double quotes), and simple escapes (\\, \", \').
fn split_args(input: &str) -> Result<Vec<String>> {
    // We need to index over the characters of the input, but also need to be able to peek at the next token in case of escapes.
    let mut i = 0;
    let input: Vec<char> = input.chars().collect();
    let mut output = Vec::<String>::new();
    while i < input.len() {
        // Strip any leading whitespace
        if input[i] == ' ' || input[i] == '\t' {
            i += 1;
            continue;
        }
        if input[i] == '#' {
            break; // it's a comment, we're done
        }

        // We're at the start of a real token
        let mut current_arg = String::new();
        let mut quote_state: char = '\0';

        while i < input.len() {
            let ch = input[i];
            match (ch, quote_state) {
                ('\\', _) => {
                    // It might be an escape
                    let next = input.get(i + 1);
                    match next {
                        Some(nn @ ('\'' | '\"' | '\\')) => {
                            // It is an escape
                            current_arg.push(*nn);
                            i += 1;
                        }
                        Some(_) | None => current_arg.push(ch), // Ignore unrecognised escape
                    }
                }
                (' ' | '\t', '\0') => break, // end of token
                (q @ ('\'' | '\"'), '\0') => quote_state = q, // start of quote
                (q1, q2) if q1 == q2 => quote_state = '\0', // end of quote
                (c, _) => current_arg.push(c), // nothing special
            }
            i += 1;
        }

        // end of token
        anyhow::ensure!(quote_state == '\0', "unterminated quote");
        output.push(current_arg);
        i += 1;
    }
    Ok(output)
}

fn evaluate_host_match(host: &str, args: &Vec<String>) -> bool {
    for arg in args {
        if wildmatch::WildMatch::new(arg).matches(host) {
            return true;
        }
    }
    false
}

/// Wildcard matching and ~ expansion for Include directives
fn find_include_files(arg: &str, is_user: bool) -> Result<Vec<String>> {
    let mut path = if arg.starts_with('~') {
        anyhow::ensure!(
            is_user,
            "include paths may not start with ~ in a system configuration file"
        );
        expanduser::expanduser(arg)
            .with_context(|| format!("expanding include expression {arg}"))?
    } else {
        PathBuf::from(arg)
    };
    if !path.is_absolute() {
        if is_user {
            let Some(home) = dirs::home_dir() else {
                anyhow::bail!("could not determine home directory");
            };
            let mut buf = home;
            buf.push(".ssh");
            buf.push(path);
            path = buf;
        } else {
            let mut buf = PathBuf::from("/etc/ssh/");
            buf.push(path);
            path = buf;
        }
    }

    let mut result = Vec::new();
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_leading_dot: true,
        require_literal_separator: true,
    };
    for entry in (glob_with(path.to_string_lossy().as_ref(), options)?).flatten() {
        if let Some(s) = entry.to_str() {
            result.push(s.into());
        }
    }
    Ok(result)
}

pub(crate) struct Parser<R>
where
    R: Read,
{
    line_number: usize,
    reader: BufReader<R>,
    source: String,
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
            is_user,
        ))
    }
}

impl<R: Read> Parser<R> {
    fn for_reader(reader: BufReader<R>, source: String, is_user: bool) -> Self {
        Self {
            line_number: 0,
            reader,
            source,
            is_user,
        }
    }
}

impl<'a> Parser<&'a [u8]> {
    fn for_str(s: &'a str, is_user: bool) -> Self {
        Self::for_reader(BufReader::new(s.as_bytes()), "<string>".into(), is_user)
    }
}

impl Default for Parser<&[u8]> {
    fn default() -> Self {
        Parser::for_str("", false)
    }
}

impl<R: Read> Parser<R> {
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
        host: &str,
        accepting: &mut bool,
        depth: u8,
        output: &mut BTreeMap<String, Setting>,
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
                    *accepting = evaluate_host_match(host, &args);
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
                            subparser.parse_file_inner(host, accepting, depth + 1, output)?;
                        }
                    }
                }
                Line::Generic { keyword, args, .. } => {
                    if *accepting {
                        // per ssh_config(5), the first matching entry for a given key wins.
                        let _ = output.entry(keyword).or_insert_with(|| Setting {
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

    pub(crate) fn parse_file_for(&mut self, host: &str) -> Result<BTreeMap<String, Setting>> {
        let mut output = BTreeMap::<String, Setting>::new();
        let mut accepting = true;
        self.parse_file_inner(host, &mut accepting, 0, &mut output)?;
        Ok(output)
    }
}

#[cfg(test)]
mod test {
    use anyhow::{anyhow, Context, Result};
    use assertables::{assert_contains, assert_contains_as_result, assert_eq_as_result};

    use crate::{
        os::{AbstractPlatform, Platform},
        util::make_test_tempfile,
    };

    use super::{evaluate_host_match, find_include_files, split_args, Line, Parser};
    #[test]
    fn arg_splitting() -> Result<()> {
        for (input, expected) in [
            ("", vec![]),
            ("a", vec!["a"]),
            ("   a    b   ", vec!["a", "b"]),
            (" a b # c d", vec!["a", "b"]),
            (r#"a\ \' \"b"#, vec!["a\\", "'", "\"b"]),
            (r#""a b" 'c d'"#, vec!["a b", "c d"]),
            (r#""a \"b" '\'c d'"#, vec!["a \"b", "'c d"]),
        ] {
            let msg = || format!("input \"{input}\" failed");
            assert_eq_as_result!(split_args(input).with_context(msg)?, expected)
                .map_err(|e| anyhow!(e))
                .with_context(msg)?;
        }
        for (input, expected_msg) in [
            ("aaa\"bbb", "unterminated quote"),
            ("'", "unterminated quote"),
        ] {
            let err = split_args(input).unwrap_err();
            assert_contains_as_result!(err.to_string(), expected_msg)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("input \"{input}\" failed"))?;
        }
        Ok(())
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
    fn host_matching() -> Result<()> {
        for (host, args, result) in [
            ("foo", vec!["foo"], true),
            ("foo", vec![""], false),
            ("foo", vec!["bar"], false),
            ("foo", vec!["bar", "foo"], true),
            ("foo", vec!["f?o"], true),
            ("fooo", vec!["f?o"], false),
            ("foo", vec!["f*"], true),
            ("oof", vec!["*of"], true),
            ("192.168.1.42", vec!["192.168.?.42"], true),
            ("192.168.10.42", vec!["192.168.?.42"], false),
        ] {
            assert_eq_as_result!(evaluate_host_match(host, &make_vec!(args.clone())), result)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("host {host}, args {args:?}"))?;
        }
        Ok(())
    }

    macro_rules! assert_1_arg {
        ($left:expr, $right:expr) => {
            assert_eq!(($left).unwrap().args.first().unwrap(), $right);
        };
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
        .parse_file_for("any host")
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
        .parse_file_for("Fred")
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
        .parse_file_for("Fred")
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
        .parse_file_for("Fred")
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
            .parse_file_for("any")
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
            .parse_file_for("any")
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
            .parse_file_for("any")
            .unwrap();
        assert_1_arg!(output.get("hi"), "there");
        assert_1_arg!(output.get("green"), "cheese");
    }

    #[test]
    #[ignore] // this test is dependent on the current user filespace
    fn tilde_expansion_current_user() {
        let a = find_include_files("~/*.conf", true).expect("~ should expand to home directory");
        assert!(!a.is_empty());
        let _ = find_include_files("~/*", false)
            .expect_err("~ should not be allowed in system configurations");
    }

    #[test]
    #[ignore] // obviously this won't run on CI. TODO: figure out a way to make it CIable.
    fn tilde_expansion_arbitrary_user() {
        let a =
            find_include_files("~wry/*.conf", true).expect("~ should expand to a home directory");
        println!("{a:?}");
        assert!(!a.is_empty());
        let _ = find_include_files("~/*", false)
            .expect_err("~ should not be allowed in system configurations");
    }

    #[test]
    #[ignore] // TODO: Make this runnable on CI
    fn relative_path_expansion() {
        let a = find_include_files("config", true).unwrap();
        println!("{a:?}");
        assert!(!a.is_empty());

        let a = find_include_files("sshd_config", false).unwrap();
        println!("{a:?}");
        assert!(!a.is_empty());

        // but the user does not have an sshd_config:
        let a = find_include_files("sshd_config", true).unwrap();
        println!("{a:?}");
        assert!(a.is_empty());
    }

    #[test]
    #[ignore]
    fn dump_local_config() {
        let path = Platform::user_ssh_config().unwrap();
        let mut parser = Parser::for_path(path, true).unwrap();
        let data = parser.parse_file_for("lapis").unwrap();
        println!("{data:#?}");
    }
}
