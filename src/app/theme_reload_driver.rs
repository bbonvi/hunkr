//! Theme reload trigger driver: filesystem events first, interval polling as fallback.

use std::{
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher, recommended_watcher};

/// Runtime trigger source for theme file reload checks.
pub(super) struct ThemeReloadDriver {
    source: ThemeReloadSource,
    fallback_poll_every: Duration,
    last_fallback_poll: Instant,
}

enum ThemeReloadSource {
    Watch {
        _watcher: RecommendedWatcher,
        rx: Receiver<notify::Result<notify::Event>>,
    },
    Fallback,
}

impl ThemeReloadDriver {
    /// Create a theme reload trigger watching the config directory, with polling fallback.
    pub(super) fn new(watch_dir: &Path, fallback_poll_every: Duration, now: Instant) -> Self {
        let source = Self::watch_source(watch_dir).unwrap_or(ThemeReloadSource::Fallback);
        Self {
            source,
            fallback_poll_every,
            last_fallback_poll: now,
        }
    }

    /// Returns when the next fallback poll is due, if running in fallback mode.
    pub(super) fn fallback_poll_in(&self, now: Instant) -> Option<Duration> {
        if !matches!(self.source, ThemeReloadSource::Fallback) {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.last_fallback_poll);
        Some(self.fallback_poll_every.saturating_sub(elapsed))
    }

    /// Returns true when theme metadata should be checked and reloaded if changed.
    pub(super) fn should_reload(&mut self, now: Instant) -> bool {
        let mut saw_event = false;
        let mut disconnected = false;
        if let ThemeReloadSource::Watch { rx, .. } = &mut self.source {
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
                return true;
            }
        }

        if disconnected {
            self.source = ThemeReloadSource::Fallback;
        }
        self.fallback_poll_due(now)
    }

    fn watch_source(watch_dir: &Path) -> notify::Result<ThemeReloadSource> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = recommended_watcher(move |result| {
            let _ = tx.send(result);
        })?;
        watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;
        Ok(ThemeReloadSource::Watch {
            _watcher: watcher,
            rx,
        })
    }

    fn fallback_poll_due(&mut self, now: Instant) -> bool {
        if !matches!(self.source, ThemeReloadSource::Fallback) {
            return false;
        }
        if now.saturating_duration_since(self.last_fallback_poll) < self.fallback_poll_every {
            return false;
        }
        self.last_fallback_poll = now;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polling_driver(interval: Duration, now: Instant) -> ThemeReloadDriver {
        ThemeReloadDriver {
            source: ThemeReloadSource::Fallback,
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
    fn fallback_should_reload_only_when_interval_elapsed() {
        let now = Instant::now();
        let mut driver = polling_driver(Duration::from_secs(1), now);
        assert!(!driver.should_reload(now + Duration::from_millis(900)));
        assert!(driver.should_reload(now + Duration::from_secs(1)));
        assert!(!driver.should_reload(now + Duration::from_secs(1) + Duration::from_millis(10)));
    }
}
