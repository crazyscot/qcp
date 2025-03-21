//! Host matching
// (c) 2024 Ross Younger

fn match_one_pattern(host: &str, pattern: &str) -> bool {
    if let Some(negative_pattern) = pattern.strip_prefix('!') {
        !wildmatch::WildMatch::new(negative_pattern).matches(host)
    } else {
        wildmatch::WildMatch::new(pattern).matches(host)
    }
}

pub(super) fn evaluate_host_match(host: Option<&str>, args: &[String]) -> bool {
    if let Some(host) = host {
        args.iter().any(|arg| match_one_pattern(host, arg))
    } else {
        // host is None i.e. unspecified; match only on '*'
        args.iter().any(|arg| arg == "*")
    }
}

///////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::evaluate_host_match;
    use anyhow::{Context, Result, anyhow};
    use assertables::assert_eq_as_result;

    /// helper macro: concise notation to create a Vec<String>
    ///
    /// # Example
    /// ```
    /// use vec_of_strings as sv;
    /// assert_eq!(sv["a","b"], vec![String::from("a"), String::from("b")]);
    /// ```
    macro_rules! vec_of_strings {
        ($($x:expr),*) => (vec![$($x.to_string()),*]);
    }

    use vec_of_strings as sv;

    #[test]
    fn host_matching() -> Result<()> {
        for (host, args, result) in [
            ("foo", sv!["foo"], true),
            ("foo", sv![""], false),
            ("foo", sv!["bar"], false),
            ("foo", sv!["bar", "foo"], true),
            ("foo", sv!["f?o"], true),
            ("fooo", sv!["f?o"], false),
            ("foo", sv!["f*"], true),
            ("oof", sv!["*of"], true),
            ("192.168.1.42", sv!["192.168.?.42"], true),
            ("192.168.10.42", sv!["192.168.?.42"], false),
            ("xyzy", sv!["!xyzzy"], true),
            ("xyzy", sv!["!xyzy"], false),
        ] {
            assert_eq_as_result!(evaluate_host_match(Some(host), &args), result)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("host {host}, args {args:?}"))?;
        }
        Ok(())
    }
    #[test]
    fn unspecified_host() -> Result<()> {
        for (args, result) in [
            (sv!["foo", "bar", "baz"], false),
            (sv!["*"], true),
            (sv!["foo", "bar", "*", "baz"], true), // silly case but we ought to get it right
        ] {
            assert_eq_as_result!(evaluate_host_match(None, &args), result)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("host <unspecified>, args {args:?}"))?;
        }
        Ok(())
    }
}
