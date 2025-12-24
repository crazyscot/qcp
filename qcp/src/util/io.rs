//! File & async I/O helpers
// (c) 2024-5 Ross Younger

use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) async fn read_available_non_blocking<R: AsyncRead + Unpin>(
    mut reader: R,
    buffer: &mut tokio::io::ReadBuf<'_>,
) -> Result<(), std::io::Error> {
    std::future::poll_fn(|cx| {
        // Attempt to read data. If no data is available, poll_read returns Poll::Pending.
        // The Waker in 'cx' will be registered to wake this task when data is ready.
        Pin::new(&mut reader).poll_read(cx, buffer)
    })
    .await
}

/// File transfer buffer size (bytes).
///
/// `tokio::io::copy` uses an internal 8KiB buffer, which can become CPU-bound at
/// higher throughputs; qcp's typical use-case is large transfers, so we use a
/// larger buffer.
pub(crate) const DEFAULT_COPY_BUFFER_SIZE: u64 = 1024 * 1024;

/// `tokio::io::copy` uses an internal 8KiB buffer, which can become CPU-bound at
/// higher throughputs; qcp's typical use-case is large transfers, so we use a
/// larger buffer.
pub(crate) async fn copy_large<R, W, Z>(
    reader: &mut R,
    writer: &mut W,
    buffer_size: Z,
) -> Result<u64, std::io::Error>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
    Z: num_traits::cast::AsPrimitive<usize>,
{
    let mut reader = tokio::io::BufReader::with_capacity(buffer_size.as_(), reader);
    tokio::io::copy_buf(&mut reader, writer).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod microbench {
    use std::path::Path;
    use std::time::Duration;

    use anyhow::Context as _;
    use human_repr::HumanCount as _;
    use tokio::io::AsyncReadExt as _;

    use super::{DEFAULT_COPY_BUFFER_SIZE, copy_large};

    const DEFAULT_BYTES: u64 = 256 * 1024 * 1024;
    const DEFAULT_WARMUP_BYTES: u64 = 64 * 1024 * 1024;
    const DEFAULT_ITERS: usize = 5;

    fn env_u64(name: &str) -> Option<u64> {
        let value = std::env::var(name).ok()?;
        value.parse().ok()
    }

    fn env_usize(name: &str) -> Option<usize> {
        let value = std::env::var(name).ok()?;
        value.parse().ok()
    }

    fn bytes_per_second(bytes: u64, duration: Duration) -> u64 {
        let nanos = duration.as_nanos();
        if nanos == 0 {
            return u64::MAX;
        }
        let bytes_per_sec = u128::from(bytes).saturating_mul(1_000_000_000u128) / nanos;
        u64::try_from(bytes_per_sec).unwrap_or(u64::MAX)
    }

    fn median(durations: &mut [Duration]) -> Duration {
        durations.sort_unstable();
        durations[durations.len() / 2]
    }

    async fn run_tokio_copy_file(path: &Path, bytes: u64) -> anyhow::Result<Duration> {
        let file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("open {path:?}"))?;
        let mut reader = file.take(bytes);
        let mut writer = tokio::io::sink();
        let start = std::time::Instant::now();
        let copied = tokio::io::copy(&mut reader, &mut writer)
            .await
            .context("tokio::io::copy failed")?;
        anyhow::ensure!(
            copied == bytes,
            "tokio::io::copy copied {copied}B, expected {bytes}B"
        );
        Ok(start.elapsed())
    }

    async fn run_copy_large_file(path: &Path, bytes: u64) -> anyhow::Result<Duration> {
        let file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("open {path:?}"))?;
        let mut reader = file.take(bytes);
        let mut writer = tokio::io::sink();
        let start = std::time::Instant::now();
        let copied = copy_large(&mut reader, &mut writer, DEFAULT_COPY_BUFFER_SIZE)
            .await
            .context("copy_large failed")?;
        anyhow::ensure!(
            copied == bytes,
            "copy_large copied {copied}B, expected {bytes}B"
        );
        Ok(start.elapsed())
    }

    /// Microbenchmark for the file payload copy path.
    ///
    /// Runs in release mode via:
    /// `cargo test -p qcp --release microbench_copy_large_vs_tokio_copy -- --ignored --nocapture`
    ///
    /// Tune with env vars:
    /// - `QCP_COPY_BENCH_BYTES` (default: 256MiB)
    /// - `QCP_COPY_BENCH_WARMUP_BYTES` (default: 64MiB)
    /// - `QCP_COPY_BENCH_ITERS` (default: 5)
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "microbenchmark; run manually"]
    async fn microbench_copy_large_vs_tokio_copy() -> anyhow::Result<()> {
        let bytes = env_u64("QCP_COPY_BENCH_BYTES").unwrap_or(DEFAULT_BYTES);
        let warmup_bytes = env_u64("QCP_COPY_BENCH_WARMUP_BYTES").unwrap_or(DEFAULT_WARMUP_BYTES);
        let iters = env_usize("QCP_COPY_BENCH_ITERS").unwrap_or(DEFAULT_ITERS);
        anyhow::ensure!(iters > 0, "QCP_COPY_BENCH_ITERS must be >0");

        let tempdir = tempfile::tempdir().context("creating tempdir")?;
        let file_path = tempdir.path().join("copybench.dat");
        let file = tokio::fs::File::create(&file_path)
            .await
            .context("creating temp file")?;
        file.set_len(bytes).await.context("setting file size")?;
        drop(file);

        eprintln!(
            "qcp copy microbench: bytes {}, warmup {}, iters {}, copy_large buffer {}, source file {file_path:?}",
            bytes.human_count_bytes(),
            warmup_bytes.human_count_bytes(),
            iters,
            DEFAULT_COPY_BUFFER_SIZE.human_count_bytes()
        );

        // Warm up caches/jit/etc.
        let warmup_bytes = std::cmp::min(bytes, warmup_bytes);
        let _ = run_tokio_copy_file(&file_path, warmup_bytes).await?;
        let _ = run_copy_large_file(&file_path, warmup_bytes).await?;

        let mut tokio_durations = Vec::with_capacity(iters);
        let mut large_durations = Vec::with_capacity(iters);
        for _ in 0..iters {
            tokio_durations.push(run_tokio_copy_file(&file_path, bytes).await?);
            large_durations.push(run_copy_large_file(&file_path, bytes).await?);
        }

        let tokio_median = median(&mut tokio_durations);
        let large_median = median(&mut large_durations);

        let tokio_bps = bytes_per_second(bytes, tokio_median);
        let large_bps = bytes_per_second(bytes, large_median);

        let improvement_pct = if tokio_bps == 0 {
            None
        } else {
            let tokio_bps = i128::from(tokio_bps);
            let large_bps = i128::from(large_bps);
            Some((large_bps - tokio_bps) * 100 / tokio_bps)
        };

        eprintln!(
            "tokio::io::copy      median {tokio_median:?}, {tokio_bps}/s",
            tokio_bps = tokio_bps.human_count_bytes()
        );
        eprintln!(
            "crate::io::copy_large median {large_median:?}, {large_bps}/s",
            large_bps = large_bps.human_count_bytes()
        );
        if let Some(improvement_pct) = improvement_pct {
            eprintln!("throughput change: {improvement_pct:+}%");
        }

        Ok(())
    }
}
