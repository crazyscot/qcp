// Console related functions and styling
// (c) 2024 Ross Younger

use console::Term;

const PROGRESS_STYLE_COMPACT: &str =
    "{msg:.dim} {wide_bar:.cyan} {eta} @ {decimal_bytes_per_sec} [{decimal_total_bytes:.dim}]";

// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
// filename [==============================            ] 2m30s @ 123.4MB/s [1.24GB]
// fairly-long-filename [========================      ] 2m30s @ 123.4MB/s [1.24GB]
// extremely-long-filename-no-really-very-long [====   ] 2m30s @ 123.4MB/s [1.24GB]
// 11111111111111111111111111111111111111111111111111111111111111111111111111111111

// We need about 30 characters for the data readout.
// A useful progress bar needs maybe 20 characters.
// This informs how much space we can allow for the filename.
const DATA_AND_PROGRESS: usize = 50;

// 11111111111111111111111111111111111111111111111111111111111111111111111111111111
// extremely-long-filename-no-really-very-long                             [1.24GB]
// [==========================                                  ] 2m30s @ 123.4MB/s
// 11111111111111111111111111111111111111111111111111111111111111111111111111111111

const PROGRESS_STYLE_OVERLONG: &str =
    "{wide_msg:.dim} [{decimal_total_bytes:.dim}]\n{wide_bar:.cyan} {eta} @ {decimal_bytes_per_sec}";

fn use_long_style(terminal: &Term, msg_size: usize) -> bool {
    let term_width = terminal.size().1 as usize; // this returns a reasonable default if it can't detect
    msg_size + DATA_AND_PROGRESS > term_width
}

pub(crate) fn progress_style_for(terminal: &Term, msg_size: usize) -> &str {
    match use_long_style(terminal, msg_size) {
        true => PROGRESS_STYLE_OVERLONG,
        false => PROGRESS_STYLE_COMPACT,
    }
}
