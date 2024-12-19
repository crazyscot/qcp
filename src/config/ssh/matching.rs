//! Host matching
// (c) 2024 Ross Younger

pub(super) fn evaluate_host_match(host: &str, args: &Vec<String>) -> bool {
    for arg in args {
        if wildmatch::WildMatch::new(arg).matches(host) {
            return true;
        }
    }
    false
}

///////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use super::evaluate_host_match;
    use anyhow::{anyhow, Context, Result};
    use assertables::assert_eq_as_result;

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
            let vec = args
                .clone()
                .into_iter()
                .map(std::convert::Into::into)
                .collect();
            assert_eq_as_result!(evaluate_host_match(host, &vec), result)
                .map_err(|e| anyhow!(e))
                .with_context(|| format!("host {host}, args {args:?}"))?;
        }
        Ok(())
    }
}
