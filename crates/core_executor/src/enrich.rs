use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct CoreFuture<F> {
    core_id: usize,
    future: F,
}

impl<F> CoreFuture<F>
where
    F: std::future::Future,
{
    /// Create a new CoreFuture. Should only be created as a read-only metadata
    /// wrapper over a future created by this library.
    pub(crate) fn new(core_id: usize, future: F) -> Self {
        Self { core_id, future }
    }

    /// Get the core id that this future is running on.
    pub fn core_id(&self) -> usize {
        self.core_id
    }
}

impl<F> Future for CoreFuture<F>
where
    F: Future + Unpin,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.future).poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreadPoolExecutor;

    #[smol_potat::test]
    async fn test_future_wrapper() {
        let mut executor = ThreadPoolExecutor::new(2);
        let future = executor.spawn_on_any(async { 1 });
        assert_eq!(future.await, Ok(1));
    }

    #[smol_potat::test]
    async fn should_be_on_consistent_cores() {
        let mut executor = ThreadPoolExecutor::new(2);
        for _ in 0..10 {
            let core_future = executor.spawn_on_any(async {
                let actual_core = wat_cpu::get_current_cpu();
                println!("actual core is {}", actual_core);
                actual_core
            });
            println!("should be spawned on core {}", core_future.core_id());
            assert_eq!(core_future.core_id(), core_future.await.unwrap());
        }
    }
}
