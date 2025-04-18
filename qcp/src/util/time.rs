//! Time utilities
// (c) 2024 Ross Younger

use std::{
    cmp::max,
    time::{Duration, Instant},
};

use human_repr::HumanDuration;

#[derive(Debug, Default, Clone)]
/// A simple named stopwatch.
/// This stopwatch does not currently support resuming or splits.
pub(crate) struct Stopwatch {
    /// Descriptive name
    pub name: String,
    start_: Option<Instant>,
    stop_: Option<Instant>,
}

impl Stopwatch {
    /// Creates a running stopwatch.
    /// If you wanted to create a stopped stopwatch, use `::default()` or `::new_stopped()`
    #[must_use]
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start_: Some(Instant::now()),
            stop_: None,
        }
    }

    /// Starts this stopwatch
    /// # Panics
    /// It is a logic error to call start more than once.
    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn start(&mut self) {
        assert!(self.start_.is_none(), "Stopwatch already started");
        self.start_ = Some(Instant::now());
    }

    /// Starts this stopwatch
    /// # Panics
    /// It is a logic error to call stop more than once.
    #[must_use]
    pub(crate) fn stop(&mut self) -> Option<Duration> {
        assert!(self.stop_.is_none(), "Stopwatch already stopped");
        self.stop_ = Some(Instant::now());
        self.elapsed()
    }

    /// Returns the elapsed duration so far
    #[must_use]
    pub(crate) fn elapsed(&self) -> Option<Duration> {
        if let Some(start) = self.start_ {
            if let Some(stop) = self.stop_ {
                return Some(stop - start);
            }
        }
        None
    }

    /// Stops this stopwatch, starts a new one where it left off
    #[must_use]
    pub(crate) fn chain(&mut self, new_name: &str) -> Self {
        let _ = self.stop();
        Self {
            name: new_name.to_string(),
            start_: self.stop_,
            stop_: None,
        }
    }

    /// Formatter for --profile mode
    fn fmt_ln(&self, f: &mut std::fmt::Formatter<'_>, width: usize) -> std::fmt::Result {
        let t = self.elapsed();
        if let Some(t) = t {
            writeln!(f, "  {:width$}: {}", self.name, t.human_duration())
        } else {
            writeln!(f, "  {:width$}: None", self.name)
        }
    }
}

/// A chain of stopwatches, intended for instrumenting program elapsed time.
#[derive(Debug, Default, Clone)]
pub(crate) struct StopwatchChain {
    watches: Vec<Stopwatch>,
}

impl StopwatchChain {
    /// Convenience method: constructs and starts a stopwatch chain
    #[must_use]
    pub(crate) fn new_running(name: &str) -> Self {
        let mut r = Self::default();
        r.next(name);
        r
    }
    /// Stops the current stopwatch (if there is one), adds a new stopwatch to the chain and starts it.
    pub(crate) fn next(&mut self, name: &str) {
        let new1 = match self.watches.last_mut() {
            None => Stopwatch::new(name),
            Some(latest) => latest.chain(name),
        };
        self.watches.push(new1);
    }
    /// Stops the chain. This is final, you cannot restart or call `next()`.
    pub(crate) fn stop(&mut self) {
        let _ = self.watches.last_mut().map(Stopwatch::stop);
    }

    /// Extracts a single stopwatch by name, if it was present
    #[must_use]
    pub(crate) fn find(&self, name: &str) -> Option<&Stopwatch> {
        self.watches.iter().find(|&sw| sw.name == name)
    }
}

/// Simple display formatting
impl std::fmt::Display for StopwatchChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut largest = 0usize;
        for sw in &self.watches {
            largest = max(largest, sw.name.len());
        }

        for sw in &self.watches {
            sw.fmt_ln(f, largest)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Stopwatch, StopwatchChain};
    #[test]
    fn new_stopwatch_is_running() {
        let mut a = Stopwatch::new("");
        assert!(a.stop().is_some());
    }
    #[test]
    fn default_stopwatch_is_not_running() {
        let mut a = Stopwatch::default();
        assert!(a.stop().is_none());
    }
    #[test]
    #[should_panic(expected = "Stopwatch already started")]
    fn cannot_start_twice() {
        let mut a = Stopwatch::new("a");
        a.start();
    }
    #[test]
    #[should_panic(expected = "Stopwatch already stopped")]
    fn cannot_stop() {
        let mut a = Stopwatch::new("a");
        let _ = a.stop();
        let _ = a.stop();
    }

    #[test]
    fn empty_chain() {
        let c = StopwatchChain::default();
        println!("{c}");
    }
    #[test]
    fn running_chain() {
        let mut c = StopwatchChain::default();
        c.next("a");
        c.next("b");
        c.next("c");
        println!("{c}");
    }
    #[test]
    fn finished_chain() {
        let mut c = StopwatchChain::default();
        c.next("a");
        c.next("b");
        c.next("c");
        c.stop();
        println!("{c}");
    }
    #[test]
    #[should_panic(expected = "Stopwatch already stopped")]
    fn cannot_restart_stopped_chain() {
        let mut c = StopwatchChain::default();
        c.next("a");
        c.stop();
        c.next("b");
    }

    #[test]
    fn new_running() {
        let c = StopwatchChain::new_running("timer1");
        assert!(c.find("timer1").is_some());
    }
}
