// Instant progress read-out
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
    pub(crate) fn new(source: &ProgressBar, destination: ProgressBar) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InstaMeterInner::new(source, destination))),
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
}

impl InstaMeterInner {
    pub(crate) fn new(source: &ProgressBar, destination: ProgressBar) -> Self {
        Self {
            previous_position: 0u64,
            source: source.clone(),
            destination,
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
        let msg = format!(
            "Transferring data, instant rate: {}",
            rate.human_throughput_bytes()
        );
        self.destination.set_message(msg.clone());
        msg
    }
}
