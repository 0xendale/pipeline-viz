//! Shutdown signalling for every background task the tracker owns.
//!
//! The tracker's tasks are detached, so nothing but an explicit signal can stop
//! them. A `watch` channel is the right shape: the signal is a latched flag, a
//! late subscriber still observes it, and setting it never blocks — which
//! matters because it is set from `Drop`.

use tokio::sync::watch;

/// Held by the tracker. Dropping the tracker fires it.
#[derive(Debug)]
pub(crate) struct CancelSignal {
    sender: watch::Sender<bool>,
}

impl CancelSignal {
    pub(crate) fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    /// A token for one task. Subscribe *before* spawning, so a task that is
    /// dropped before its first poll still carries the signal.
    pub(crate) fn token(&self) -> CancelToken {
        CancelToken {
            receiver: self.sender.subscribe(),
        }
    }

    /// Latch the flag. Infallible and non-blocking, including with no receivers.
    pub(crate) fn cancel(&self) {
        self.sender.send_replace(true);
    }

    /// How many tasks still hold a token. Used by tests to prove they exited.
    #[cfg(test)]
    pub(crate) fn token_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// One task's view of the shutdown signal.
#[derive(Clone, Debug)]
pub(crate) struct CancelToken {
    receiver: watch::Receiver<bool>,
}

impl CancelToken {
    /// Resolves once shutdown has been requested.
    ///
    /// Three cases count as cancelled: the flag is already set, it is set
    /// later, or the signal itself was dropped without the flag being read.
    /// The last one cannot happen through the tracker — `Drop` sets the flag
    /// first — but treating a closed channel as "the owner is gone" is the only
    /// answer that cannot leak a task.
    pub(crate) async fn cancelled(&mut self) {
        if *self.receiver.borrow() {
            return;
        }
        let _ = self.receiver.wait_for(|cancelled| *cancelled).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_already_cancelled_token_resolves_immediately() {
        let signal = CancelSignal::new();
        let mut token = signal.token();
        signal.cancel();

        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("a latched flag needs no further change");
    }

    #[tokio::test]
    async fn a_token_subscribed_after_cancellation_still_sees_it() {
        let signal = CancelSignal::new();
        signal.cancel();
        let mut token = signal.token();

        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("the flag is latched, not an edge");
    }

    #[tokio::test]
    async fn a_dropped_signal_cancels_its_tokens() {
        let signal = CancelSignal::new();
        let mut token = signal.token();
        drop(signal);

        tokio::time::timeout(std::time::Duration::from_secs(1), token.cancelled())
            .await
            .expect("a closed channel means the owner is gone");
    }

    #[tokio::test]
    async fn every_waiting_task_exits_and_releases_its_token() {
        let signal = CancelSignal::new();
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let mut token = signal.token();
                tokio::spawn(async move { token.cancelled().await })
            })
            .collect();
        assert_eq!(signal.token_count(), 8);

        signal.cancel();
        for task in tasks {
            task.await.expect("no task panics on shutdown");
        }

        assert_eq!(signal.token_count(), 0, "no task outlives cancellation");
    }
}
