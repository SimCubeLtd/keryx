//! Coalesced dashboard invalidation for connected browsers.
//!
//! The channel carries only a revision. Browsers fetch a fresh server-rendered
//! snapshot after each change, so skipped intermediate revisions never leave a
//! dashboard stale.

use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct DashboardUpdates {
    sender: watch::Sender<u64>,
    closing: watch::Sender<bool>,
}

impl DashboardUpdates {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(0);
        let (closing, _) = watch::channel(false);
        Self { sender, closing }
    }

    /// Mark the current dashboard snapshot as stale.
    pub fn changed(&self) {
        self.sender
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// End every live-update stream. They never finish on their own, so a
    /// graceful shutdown that waits for open connections would wait forever
    /// while a dashboard tab is open. Browsers reconnect when the server is back.
    pub fn close(&self) {
        self.closing.send_replace(true);
    }

    /// Resolves once [`close`](Self::close) has been called, including when it
    /// was called before this future was created.
    pub async fn closed(&self) {
        let mut closing = self.closing.subscribe();
        // Err means the sender is gone, which is just as final.
        let _ = closing.wait_for(|closing| *closing).await;
    }

    /// Observe the latest revision. A new receiver starts with the current
    /// revision, which makes reconnects recover changes missed while offline.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_receive_the_latest_revision() {
        let updates = DashboardUpdates::new();
        let mut first = updates.subscribe();

        updates.changed();
        updates.changed();

        first.changed().await.expect("sender remains alive");
        assert_eq!(*first.borrow_and_update(), 2);
        assert_eq!(*updates.subscribe().borrow(), 2);
    }

    #[tokio::test]
    async fn closed_resolves_for_streams_opened_before_and_after_close() {
        let updates = DashboardUpdates::new();
        let early = tokio::spawn({
            let updates = updates.clone();
            async move { updates.closed().await }
        });
        tokio::task::yield_now().await;
        assert!(!early.is_finished());

        updates.close();
        early.await.unwrap();
        updates.closed().await;
    }
}
