//! Bandwidth throttling for response bodies.

use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use tokio::time::{Sleep, sleep};

/// Releases a body at a fixed rate by splitting it into small chunks and pausing between them.
pub struct ThrottledBody<B> {
    inner: B,
    bytes_per_sec: u64,
    chunk: usize,
    pending: Option<Bytes>,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl<B> ThrottledBody<B> {
    pub fn new(inner: B, bytes_per_sec: u64) -> Self {
        let bytes_per_sec = bytes_per_sec.max(1);
        // About twenty pauses per second keeps the stream smooth without busy timers.
        let chunk = (bytes_per_sec / 20).clamp(256, 16 * 1024) as usize;
        Self { inner, bytes_per_sec, chunk, pending: None, sleep: None }
    }
}

impl<B> Body for ThrottledBody<B>
where
    B: Body<Data = Bytes> + Unpin,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let this = &mut *self;
        loop {
            if let Some(timer) = this.sleep.as_mut() {
                ready!(timer.as_mut().poll(cx));
                this.sleep = None;
            }
            if let Some(rest) = this.pending.as_mut() {
                let n = rest.len().min(this.chunk);
                let part = rest.split_to(n);
                if rest.is_empty() {
                    this.pending = None;
                }
                let pause = Duration::from_secs_f64(n as f64 / this.bytes_per_sec as f64);
                this.sleep = Some(Box::pin(sleep(pause)));
                return Poll::Ready(Some(Ok(Frame::data(part))));
            }
            match ready!(Pin::new(&mut this.inner).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => this.pending = Some(data),
                    Err(other) => return Poll::Ready(Some(Ok(other))),
                },
                other => return Poll::Ready(other),
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.pending.is_none() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use http_body_util::{BodyExt, Full};

    use super::*;

    #[tokio::test]
    async fn paces_the_body_and_keeps_every_byte() {
        let payload = Bytes::from(vec![7u8; 10_000]);
        let body = ThrottledBody::new(Full::new(payload.clone()), 20_000); // 0.5 s for 10 kB
        let started = Instant::now();
        let collected = body.collect().await.unwrap().to_bytes();
        let elapsed = started.elapsed();
        assert_eq!(collected, payload);
        assert!(elapsed >= Duration::from_millis(400), "finished too fast: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(3), "finished too slowly: {elapsed:?}");
    }

    #[tokio::test]
    async fn empty_bodies_end_immediately() {
        let body = ThrottledBody::new(Full::new(Bytes::new()), 10);
        assert!(body.collect().await.unwrap().to_bytes().is_empty());
    }
}
