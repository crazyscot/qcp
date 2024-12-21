//! Line parsing internals
// (c) 2024 Ross Younger

use anyhow::Result;

#[derive(Debug, PartialEq)]
/// A parsed line we read from an ssh config file
pub(super) enum Line {
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

///////////////////////////////////////////////////////////////////////////////////////

/// Splits a string into a list of arguments.
/// Arguments are delimited by whitespace, subject to quoting (single or double quotes), and simple escapes (\\, \", \').
pub(super) fn split_args(input: &str) -> Result<Vec<String>> {
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

///////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use anyhow::{anyhow, Context, Result};
    use assertables::{assert_contains_as_result, assert_eq_as_result};

    use crate::config::ssh::split_args;
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
}
