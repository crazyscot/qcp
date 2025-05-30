//! CLI helper - Address family
// (c) 2024 Ross Younger

/// Representation of an IP address family
///
/// This is a local type that takes part in the config/CLI system.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
    strum::VariantNames,
)]
#[serde(rename_all = "kebab-case")] // to match clap::ValueEnum
pub enum AddressFamily {
    /// IPv4
    #[value(alias("4"), alias("inet4"))]
    Inet,
    /// IPv6
    #[value(alias("6"))]
    Inet6,
    /// Unspecified. qcp will use whatever seems suitable given the target address or the result of DNS lookup.
    Any,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::AddressFamily;
    use clap::ValueEnum as _;
    use pretty_assertions::assert_eq;

    #[test]
    fn serialize() {
        let a = AddressFamily::Inet;
        let b = AddressFamily::Inet6;
        let c = AddressFamily::Any;

        let aa = serde_json::to_string(&a);
        let bb = serde_json::to_string(&b);
        let cc = serde_json::to_string(&c);
        assert_eq!(aa.unwrap(), "\"inet\"");
        assert_eq!(bb.unwrap(), "\"inet6\"");
        assert_eq!(cc.unwrap(), "\"any\"");
    }

    #[test]
    fn deser_str() {
        use AddressFamily::*;
        for (str, expected) in &[("inet", Inet), ("inet6", Inet6), ("any", Any)] {
            let raw = AddressFamily::from_str(str, true).expect(str);
            let json = format!(r#""{str}""#);
            let output = serde_json::from_str::<AddressFamily>(&json).expect(str);
            assert_eq!(raw, *expected);
            assert_eq!(output, *expected);
        }
    }

    #[test]
    fn deser_invalid() {
        for s in &["true", "5", r#""5""#, "-1", r#""42"#, r#""string"#] {
            let _ = serde_json::from_str::<AddressFamily>(s).expect_err(s);
        }
    }
}
