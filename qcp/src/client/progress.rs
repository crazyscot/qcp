//! Progress bar styling
// (c) 2024 Ross Younger

/// Maximum update frequency we will use for the progress display
pub(crate) const MAX_UPDATE_FPS: u8 = 20;

/// A single-line style format for Indicatif which should cover most situations.
///
/// ```text
/// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
/// filename [==========================            ] 2m30s @ 123.4MB/s [70%/1.24GB]
/// fairly-long-filename [====================      ] 2m30s @ 123.4MB/s [70%/1.24GB]
/// extremely-long-filename-no-really-very-long [== ] 2m30s @ 123.4MB/s [70%/1.24GB]
/// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
///
const PROGRESS_STYLE_COMPACT: &str =
    "{msg:.dim} {wide_bar:.cyan} {eta} @ {decimal_bytes_per_sec} [{decimal_total_bytes:.dim}]";

/// Space to allow for the filename
///
/// We need about 35 characters for the data readout.
/// A useful progress bar needs maybe 20 characters.
/// This informs how much space we can allow for the filename.
const DATA_AND_PROGRESS: usize = 55;

/// A double-line style format for Indicatif for use when the filename is too long.
///
/// ```text
/// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
/// extremely-long-filename-no-really-very-long                         [70%/1.24GB]
/// [==========================                                  ] 2m30s @ 123.4MB/s
/// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
/// ```
const PROGRESS_STYLE_OVERLONG: &str = "{wide_msg:.dim} [{decimal_total_bytes:.dim}]\n{wide_bar:.cyan} {eta} @ {decimal_bytes_per_sec}";

/// Determine and retrieve the appropriate progress style to use
pub(crate) fn style_for(msg_size: usize) -> &'static str {
    let term_width = console::Term::stderr().size().1 as usize; // this returns a reasonable default if it can't detect
    if msg_size + DATA_AND_PROGRESS > term_width {
        PROGRESS_STYLE_OVERLONG
    } else {
        PROGRESS_STYLE_COMPACT
    }
}

/// Indicatif template for spinner lines
pub(crate) const SPINNER_TEMPLATE: &str = "{spinner} {wide_msg} {prefix}";

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::client::progress::{PROGRESS_STYLE_COMPACT, PROGRESS_STYLE_OVERLONG, style_for};

    #[test]
    fn filename_sizes() {
        assert_eq!(style_for(10), PROGRESS_STYLE_COMPACT);
        assert_eq!(style_for(500), PROGRESS_STYLE_OVERLONG);
    }
}
