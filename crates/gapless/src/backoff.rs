use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

use crate::classify::DisconnectReason;
use crate::config::BackoffConfig;

/// Jittered exponential backoff that also caps reconnects per window, so a reconnect loop
/// can't trip Solami's per-IP limit.
#[derive(Debug)]
pub struct Backoff {
    config: BackoffConfig,
    attempt: u32,
    recent: VecDeque<Instant>,
}

impl Backoff {
    pub fn new(config: BackoffConfig) -> Self {
        Self {
            config,
            attempt: 0,
            recent: VecDeque::new(),
        }
    }

    /// How long to wait before the next connect.
    pub fn next_delay(&mut self, reason: &DisconnectReason, now: Instant) -> Duration {
        let exp = self
            .config
            .initial
            .saturating_mul(1u32 << self.attempt.min(16))
            .min(self.config.max);
        self.attempt += 1;
        let mut delay = exp.mul_f64(0.5 + fastrand::f64() * 0.5);
        match reason {
            DisconnectReason::ReconnectLimit => delay = delay.max(self.config.window),
            DisconnectReason::StreamLimit => delay = delay.max(Duration::from_secs(2)),
            _ => {}
        }

        while let Some(&t) = self.recent.front() {
            if now.saturating_duration_since(t) >= self.config.window {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        if self.recent.len() >= self.config.max_per_window
            && let Some(&oldest) = self.recent.front()
        {
            let free_at = oldest + self.config.window;
            delay = delay.max(free_at.saturating_duration_since(now));
        }
        self.recent.push_back(now + delay);
        delay
    }

    /// Called once a stream is healthy again.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BackoffConfig {
        BackoffConfig {
            initial: Duration::from_millis(100),
            max: Duration::from_secs(2),
            max_per_window: 5,
            window: Duration::from_secs(10),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn grows_and_caps() {
        let mut b = Backoff::new(BackoffConfig {
            max_per_window: 1000,
            ..config()
        });
        let now = Instant::now();
        let delays: Vec<Duration> = (0..8)
            .map(|_| b.next_delay(&DisconnectReason::ServerClosed, now))
            .collect();
        assert!(delays[0] >= Duration::from_millis(50) && delays[0] <= Duration::from_millis(100));
        assert!(delays[3] >= Duration::from_millis(400));
        assert!(delays.iter().all(|d| *d <= Duration::from_secs(2)));
    }

    #[tokio::test(start_paused = true)]
    async fn reset_starts_over() {
        let mut b = Backoff::new(config());
        let now = Instant::now();
        for _ in 0..4 {
            b.next_delay(&DisconnectReason::Killed, now);
        }
        b.reset();
        assert!(b.next_delay(&DisconnectReason::Killed, now) <= Duration::from_millis(100));
    }

    #[tokio::test(start_paused = true)]
    async fn caps_reconnects_per_window() {
        let mut b = Backoff::new(config());
        let now = Instant::now();
        for _ in 0..5 {
            b.next_delay(&DisconnectReason::Killed, now);
            b.reset();
        }
        let sixth = b.next_delay(&DisconnectReason::Killed, now);
        assert!(
            sixth >= Duration::from_secs(9),
            "sixth reconnect in the window waits: {sixth:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_limit_waits_a_full_window() {
        let mut b = Backoff::new(config());
        let d = b.next_delay(&DisconnectReason::ReconnectLimit, Instant::now());
        assert!(d >= Duration::from_secs(10));
    }
}
