//! Filesystem watch trigger driver: watch events first, interval polling as fallback.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher, recommended_watcher};

/// Why a watched path should be checked on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathWatchTrigger {
    Event,
    FallbackPoll,
}

/// Runtime trigger source for checking whether a watched path may have changed.
pub(super) struct PathWatchDriver {
    watch_path: PathBuf,
    source: PathWatchSource,
    fallback_poll_every: Duration,
    last_fallback_poll: Instant,
}

enum PathWatchSource {
    Watch {
        _watcher: RecommendedWatcher,
        rx: Receiver<notify::Result<notify::Event>>,
    },
    Fallback,
}

impl PathWatchDriver {
    /// Creates a path watch driver backed by filesystem events, with interval polling fallback.
    pub(super) fn new(watch_path: &Path, fallback_poll_every: Duration, now: Instant) -> Self {
        let watch_path = watch_path.to_path_buf();
        let source = Self::watch_source(&watch_path).unwrap_or(PathWatchSource::Fallback);
        Self {
            watch_path,
            source,
            fallback_poll_every,
            last_fallback_poll: now,
        }
    }

    /// Returns when the next fallback poll is due, if the driver is not currently watched.
    pub(super) fn fallback_poll_in(&self, now: Instant) -> Option<Duration> {
        if !matches!(self.source, PathWatchSource::Fallback) {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.last_fallback_poll);
        Some(self.fallback_poll_every.saturating_sub(elapsed))
    }

    /// Returns why the caller should check on-disk state for updates on this tick.
    pub(super) fn poll_trigger(&mut self, now: Instant) -> Option<PathWatchTrigger> {
        let mut saw_event = false;
        let mut disconnected = false;
        if let PathWatchSource::Watch { rx, .. } = &mut self.source {
            loop {
                match rx.try_recv() {
                    Ok(Ok(_)) => saw_event = true,
                    Ok(Err(_)) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if saw_event {
                return Some(PathWatchTrigger::Event);
            }
        }

        if disconnected {
            self.source = PathWatchSource::Fallback;
        }
        self.fallback_poll_due(now)
    }

    fn watch_source(watch_path: &Path) -> notify::Result<PathWatchSource> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = recommended_watcher(move |result| {
            let _ = tx.send(result);
        })?;
        watcher.watch(watch_path, RecursiveMode::NonRecursive)?;
        Ok(PathWatchSource::Watch {
            _watcher: watcher,
            rx,
        })
    }

    fn fallback_poll_due(&mut self, now: Instant) -> Option<PathWatchTrigger> {
        if !matches!(self.source, PathWatchSource::Fallback) {
            return None;
        }
        if now.saturating_duration_since(self.last_fallback_poll) < self.fallback_poll_every {
            return None;
        }
        self.last_fallback_poll = now;
        self.source = Self::watch_source(&self.watch_path).unwrap_or(PathWatchSource::Fallback);
        Some(PathWatchTrigger::FallbackPoll)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn polling_driver(interval: Duration, now: Instant) -> PathWatchDriver {
        PathWatchDriver {
            watch_path: PathBuf::from("."),
            source: PathWatchSource::Fallback,
            fallback_poll_every: interval,
            last_fallback_poll: now,
        }
    }

    #[test]
    fn fallback_poll_in_reports_remaining_interval() {
        let now = Instant::now();
        let driver = polling_driver(Duration::from_secs(1), now);
        let next = driver
            .fallback_poll_in(now + Duration::from_millis(200))
            .expect("fallback mode");
        assert!(next <= Duration::from_millis(800));
    }

    #[test]
    fn fallback_should_check_only_when_interval_elapsed() {
        let now = Instant::now();
        let mut driver = polling_driver(Duration::from_secs(1), now);
        assert_eq!(driver.poll_trigger(now + Duration::from_millis(900)), None);
        assert_eq!(
            driver.poll_trigger(now + Duration::from_secs(1)),
            Some(PathWatchTrigger::FallbackPoll)
        );
        assert_eq!(
            driver.poll_trigger(now + Duration::from_secs(1) + Duration::from_millis(10)),
            None
        );
    }

    #[test]
    fn fallback_poll_retries_watch_registration_after_path_appears() {
        let tempdir = TempDir::new().expect("tempdir");
        let missing = tempdir.path().join("watched");
        let now = Instant::now();
        let mut driver = PathWatchDriver::new(&missing, Duration::from_millis(1), now);
        assert!(
            driver.fallback_poll_in(now).is_some(),
            "starts in fallback mode"
        );

        std::fs::create_dir_all(&missing).expect("create watched dir");

        assert_eq!(
            driver.poll_trigger(now + Duration::from_millis(1)),
            Some(PathWatchTrigger::FallbackPoll)
        );
        assert!(
            driver
                .fallback_poll_in(now + Duration::from_millis(1))
                .is_none(),
            "driver should attach a watcher once the path exists"
        );
    }
}
