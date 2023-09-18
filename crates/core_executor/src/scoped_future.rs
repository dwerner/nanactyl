// lifted from https://github.com/rmanoka/async-scoped

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_lite::{Future, Stream};
use futures_util::future::{AbortHandle, Abortable};
use futures_util::stream::FuturesUnordered;
use pin_project::{pin_project, pinned_drop};

use crate::enrich::CoreFuture;
use crate::ThreadPoolExecutor;

/// A scope to allow controlled spawning of non 'static
/// futures. Futures can be spawned using `spawn` or
/// `spawn_cancellable` methods.
///
/// # Safety
///
/// This type uses `Drop` implementation to guarantee
/// safety. It is not safe to forget this object unless it
/// is driven to completion.
#[pin_project(PinnedDrop)]
pub struct Scope<'env, T> {
    done: bool,
    len: usize,
    remaining: usize,
    #[pin]
    futs: FuturesUnordered<
        CoreFuture<Pin<Box<dyn futures_lite::Future<Output = Result<T, async_oneshot::Closed>>>>>,
    >,
    abort_handles: Vec<AbortHandle>,

    // Future proof against variance changes
    _marker: PhantomData<fn(&'env ()) -> &'env ()>,
    executor: &'env mut ThreadPoolExecutor,
}

impl<'env, T> Scope<'env, T>
where
    T: Send + Sync + 'static,
{
    /// Create a Scope object.
    ///
    /// This function is unsafe as `futs` may hold futures
    /// which have to be manually driven to completion.
    pub unsafe fn create(executor: &'env mut ThreadPoolExecutor) -> Self {
        Scope {
            done: false,
            len: 0,
            remaining: 0,
            futs: FuturesUnordered::new(),
            abort_handles: vec![],
            _marker: PhantomData,
            executor,
        }
    }

    /// Spawn a future with the executor's `task::spawn` functionality. The
    /// future is expected to be driven to completion before 'a expires.
    pub fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = T> + Send + 'env,
    {
        let handle = self.executor.spawn_on_any_boxed(unsafe {
            let boxed = Box::pin(f) as Pin<Box<dyn Future<Output = T>>>;
            let trans = std::mem::transmute::<_, Pin<Box<dyn Future<Output = T> + Send>>>(boxed);
            trans
        });
        self.futs.push(handle);
        self.len += 1;
        self.remaining += 1;
    }

    /// Spawn a cancellable future with the executor's `task::spawn`
    /// functionality.
    ///
    /// The future is cancelled if the `Scope` is dropped
    /// pre-maturely. It can also be cancelled by explicitly
    /// calling (and awaiting) the `cancel` method.
    #[inline]
    pub fn spawn_cancellable<F, Fu>(&mut self, f: F, default: Fu)
    where
        F: Future<Output = T> + Send + 'env,
        Fu: FnOnce() -> T + Send + 'env,
    {
        let (h, reg) = AbortHandle::new_pair();
        self.abort_handles.push(h);
        let fut = Abortable::new(f, reg);
        self.spawn(async { fut.await.unwrap_or_else(|_| default()) })
    }
}

impl<'a, T> Scope<'a, T> {
    /// Cancel all futures spawned with cancellation.
    pub fn cancel(&mut self) {
        for h in self.abort_handles.drain(..) {
            h.abort();
        }
    }

    /// Total number of futures spawned in this scope.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Number of futures remaining in this scope.
    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// A slighly optimized `collect` on the stream. Also
    /// useful when we can not move out of self.
    pub async fn collect(&mut self) -> Vec<Result<T, async_oneshot::Closed>> {
        let mut proc_outputs = Vec::with_capacity(self.remaining);

        use futures_util::StreamExt;
        while let Some(item) = self.next().await {
            proc_outputs.push(item);
        }

        proc_outputs
    }
}

impl<'a, T> Stream for Scope<'a, T> {
    type Item = Result<T, async_oneshot::Closed>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let poll = this.futs.poll_next(cx);
        if let Poll::Ready(None) = poll {
            *this.done = true;
        } else if poll.is_ready() {
            *this.remaining -= 1;
        }
        poll
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

#[pinned_drop]
impl<'a, T> PinnedDrop for Scope<'a, T> {
    fn drop(mut self: Pin<&mut Self>) {
        if !self.done {
            futures_lite::future::block_on(async {
                self.cancel();
                self.collect().await;
            });
        }
    }
}
