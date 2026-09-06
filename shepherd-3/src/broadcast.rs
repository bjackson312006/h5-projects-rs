//! Contains the `Broadcast` type, which is basically just a wrapper around `embassy_sync::pubsub::PubSubChannel` but specifically meant for allowing tasks to subscribe to an awaitable trigger signal.
//! 
//! u_TODO - if actually keep using this then this should probably be a standalone crate (maybe in firmware-rs repo) so it can be reused

use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    pubsub::PubSubChannel,
};

/// Errors that can occur when trying to wait for a broadcast signal.
pub enum WaitError {
    /// The maximum number of waiters has been reached, so the task cannot wait for the signal right now. This means you should probably increase the `N` of the `Broadcast`.
    MaxWaitersReached,
}

/// N is the max number of waiters this broadcast should allow.
pub struct Broadcast<M: RawMutex, const N: usize> { 
    inner: PubSubChannel<M, (), 1, N, 0> 
}
impl<M: RawMutex, const N: usize> Broadcast<M, N> {
    pub const fn new() -> Self { Self { inner: PubSubChannel::new() } }

    /// Waits until the broadcast is signaled.
    /// 
    /// If the `Broadcast` has reached its maximum number of waiters, this will return `WaitError` immediately without waiting. Otherwise, this function will wait
    /// until the broadcast is signaled. Note that this will yield the calling thread until the signal is recieved, so it is up to the caller to ensure the signal will
    /// actually get sent sometime. There is no timeout.
    pub async fn wait(&self) -> Result<(), WaitError> {
        let mut subscriber = self.inner.subscriber().map_err(|_| WaitError::MaxWaitersReached)?;
        subscriber.next_message_pure().await;

        Ok(())
    }

    /// Dispatch a broadcast signal. This will wake up all tasks currently waiting on this broadcast.
    pub fn signal(&self) {
        self.inner.immediate_publisher().publish_immediate(());
    }
}