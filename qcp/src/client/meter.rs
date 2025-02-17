//! Instant progress read-out
// (c) 2024 Ross Younger

//! # Rationale
//! `indicatif` has a smoothed, weighted moving-average estimator.
//! It is good for estimating the ETA, but conceals the full picture when bandwidth is spiky.
//! This struct computes the near-instant progress rate and updates the message on another progress bar.
//! Sorry (not sorry)...

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use human_repr::HumanThroughput as _;
use indicatif::ProgressBar;
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{debug, warn};

/// Convenience wrapper for `InstaMeter` that takes care of starting & stopping
#[derive(Debug)]
pub(crate) struct InstaMeterRunner {
    inner: Arc<Mutex<InstaMeterInner>>,
    task: Option<JoinHandle<()>>,
    stopper: Option<oneshot::Sender<()>>,
}

impl InstaMeterRunner {
    pub(crate) fn new(source: &ProgressBar, destination: ProgressBar, max_throughput: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InstaMeterInner::new(
                source,
                destination,
                max_throughput,
            ))),
            task: None,
            stopper: None,
        }
    }
    pub(crate) async fn start(&mut self) {
        self.stop().await;
        let (tx, mut rx) = oneshot::channel();
        self.stopper = Some(tx);
        self.task = Some(tokio::spawn({
            let inner = self.inner.clone();
            async move {
                let interval = Duration::from_secs(1);
                let mut earlier = SystemTime::now();
                loop {
                    let sleep = tokio::time::sleep(interval);
                    tokio::pin!(sleep);
                    tokio::select! {
                        () = &mut sleep => (), // we woke up, continue
                        _ = &mut rx => break, // we've been signalled to stop
                    }

                    let now = SystemTime::now();
                    let delta = now.duration_since(earlier).unwrap_or(Duration::ZERO);
                    let msg = inner.lock().unwrap().update(delta);
                    debug!("{msg}");
                    earlier = now;
                }
            }
        }));
    }
    pub(crate) async fn stop(&mut self) {
        let stopper = self.stopper.take();
        if let Some(tx) = stopper {
            if tx.send(()).is_err() {
                warn!("failed to notify meter to stop");
                return;
            } // else we sent OK.
        } else {
            return; // nothing to do
        }
        if let Some(task) = self.task.take() {
            let _ = task.await.inspect_err(|e| warn!("meter task paniced: {e}"));
        } else {
            warn!("logic error: stop called with a stopper but no task");
        }
    }
}

impl Drop for InstaMeterRunner {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

/// Near-instant progress meter wrapper for `ProgressBar`.
/// This struct holds the inner persistent data that is updated for the life of the struct.
#[derive(Clone, Debug)]
pub(crate) struct InstaMeterInner {
    previous_position: u64,
    source: ProgressBar,
    destination: ProgressBar,
    tick_calc: TickRateCalculator,
}

impl InstaMeterInner {
    pub(crate) fn new(source: &ProgressBar, destination: ProgressBar, max_throughput: u64) -> Self {
        #[allow(clippy::cast_precision_loss)]
        Self {
            previous_position: 0u64,
            source: source.clone(),
            destination,
            tick_calc: TickRateCalculator::new(max_throughput as f64),
        }
    }

    #[must_use]
    fn update(&mut self, elapsed: Duration) -> String {
        let current = self.source.position();
        #[allow(clippy::cast_precision_loss)]
        let progress = (current - self.previous_position) as f64;
        let elapsed = elapsed.as_secs_f64();
        let rate = progress / elapsed;
        self.previous_position = current;
        let msg = format!("{} (last 1s)", rate.human_throughput_bytes());
        self.destination.set_prefix(msg.clone());
        self.destination
            .enable_steady_tick(self.tick_calc.tick_time(progress));
        msg
    }
}

/// This is a Rust implementation of the calibration algorithm from
/// `https://github.com/rsalmei/alive-progress/blob/main/alive_progress/core/calibration.py`
#[derive(Clone, Copy, Debug)]
struct TickRateCalculator {
    calibration: f64,
    adjust: f64,
    factor: f64,
}

const MIN_FPS: f64 = 0.2;
const MAX_FPS: f64 = super::progress::MAX_UPDATE_FPS as f64;

impl TickRateCalculator {
    fn new(max_throughput: f64) -> Self {
        let calibration = f64::max(max_throughput, 0.000_001);
        let adjust = 100. / f64::min(calibration, 100.);
        #[allow(clippy::cast_lossless)]
        let factor = (MAX_FPS - MIN_FPS) / ((calibration * adjust) + 1.).log10();

        Self {
            calibration,
            adjust,
            factor,
        }
    }
    fn tick_rate(&self, rate: f64) -> f64 {
        if rate <= 0. {
            10. // Initial rate
        } else if rate <= self.calibration {
            ((rate * self.adjust) + 1.).log10() * self.factor + MIN_FPS
        } else {
            MAX_FPS
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn tick_time(&self, rate: f64) -> Duration {
        Duration::from_millis((1000. / self.tick_rate(rate)) as u64)
    }
}

#[cfg(test)]
mod test {
    use super::TickRateCalculator;

    fn rate(tput: f64) {
        let trc = TickRateCalculator::new(5. * 37_500_000.0);
        let hz = trc.tick_rate(tput);
        let dura = trc.tick_time(tput);
        println!("tput {tput} -> rate {hz} -> {dura:?}");
    }

    #[test]
    fn rates() {
        rate(1.);
        rate(10.);
        rate(100.);
        rate(1_000.);
        rate(10_000.);
        rate(100_000.);
        rate(1_000_000.);
        rate(10_000_000.);
        rate(37_500_000.);
    }
}
