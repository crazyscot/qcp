//! Build-time version information
// (c) 2024 Ross Younger

#[cfg_attr(coverage_nightly, coverage(off))]
/// Short version string
pub(crate) fn short() -> String {
    // this _should_ be provided by our build script; if not, something went wrong
    if let Some(v) = option_env!("QCP_VERSION_STRING") {
        return v.to_string();
    }
    let hash = option_env!("QCP_BUILD_GIT_HASH").unwrap_or("???");
    format!("{}+g{hash}", env!("CARGO_PKG_VERSION"))
}
