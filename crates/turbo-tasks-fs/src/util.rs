use std::{
    borrow::Cow,
    io::Result as IoResult,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};

use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};

// https://github.com/rust-lang/rust/blob/13471d3b2046cce78181dde6cfc146c09f55e29e/library/std/src/sys_common/io.rs#L1-L3
const DEFAULT_BUF_SIZE: usize = if cfg!(target_os = "espidf") {
    512
} else {
    8 * 1024
};

/// Joins two /-separated paths into a normalized path.
/// Paths are concatenated with /.
///
/// see also [normalize_path] for normalization.
pub fn join_path(fs_path: &str, join: &str) -> Option<String> {
    // Paths that we join are written as source code (eg, `join_path(fs_path,
    // "foo/bar.js")`) and it's expected that they will never contain a
    // backslash.
    debug_assert!(
        !join.contains('\\'),
        "joined path {} must not contain a Windows directory '\\', it must be normalized to Unix \
         '/'",
        join
    );

    if fs_path.is_empty() {
        normalize_path(join)
    } else if join.is_empty() {
        normalize_path(fs_path)
    } else {
        normalize_path(&[fs_path, "/", join].concat())
    }
}

/// Converts System paths into Unix paths. This is a noop on Unix systems, and
/// replaces backslash directory separators with forward slashes on Windows.
#[inline]
pub fn sys_to_unix(path: &str) -> Cow<'_, str> {
    #[cfg(not(target_family = "windows"))]
    {
        Cow::from(path)
    }
    #[cfg(target_family = "windows")]
    {
        Cow::Owned(path.replace(std::path::MAIN_SEPARATOR_STR, "/"))
    }
}

/// Converts Unix paths into System paths. This is a noop on Unix systems, and
/// replaces forward slash directory separators with backslashes on Windows.
#[inline]
pub fn unix_to_sys(path: &str) -> Cow<'_, str> {
    #[cfg(not(target_family = "windows"))]
    {
        Cow::from(path)
    }
    #[cfg(target_family = "windows")]
    {
        Cow::Owned(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    }
}

/// Normalizes a /-separated path into a form that contains no leading /, no
/// double /, no "." seqment, no ".." seqment.
///
/// Returns None if the path would need to start with ".." to be equal.
pub fn normalize_path(str: &str) -> Option<String> {
    let mut seqments = Vec::new();
    for seqment in str.split('/') {
        match seqment {
            "." | "" => {}
            ".." => {
                seqments.pop()?;
            }
            seqment => {
                seqments.push(seqment);
            }
        }
    }
    Some(seqments.join("/"))
}

/// Normalizes a /-separated request into a form that contains no leading /, no
/// double /, and no "." or ".." seqments in the middle of the request. A
/// request might only start with a single "." seqment and no ".." segements, or
/// any positive number of ".." seqments but no "." seqment.
pub fn normalize_request(str: &str) -> String {
    let mut seqments = vec!["."];
    // Keeps track of our directory depth so that we can pop directories when
    // encountering a "..". If this is positive, then we're inside a directory
    // and we can pop that. If it's 0, then we can't pop the directory and we must
    // keep the ".." in our seqments. This is not the same as the seqments.len(),
    // because we cannot pop a kept ".." when encountering another "..".
    let mut depth = 0;
    let mut popped_dot = false;
    for seqment in str.split('/') {
        match seqment {
            "." => {}
            ".." => {
                if depth > 0 {
                    depth -= 1;
                    seqments.pop();
                } else {
                    // The first time we push a "..", we need to remove the "." we include by
                    // default.
                    if !popped_dot {
                        popped_dot = true;
                        seqments.pop();
                    }
                    seqments.push(seqment);
                }
            }
            seqment => {
                seqments.push(seqment);
                depth += 1;
            }
        }
    }
    seqments.join("/")
}

/// AsyncBufReader adds buffering to any [AsyncRead].
///
/// This essentially just implements [AsyncBufRead] over an [AsyncRead], using a
/// large buffer to store data. As the data is consumed, an offset buffer will
/// continue to be returned until the full buffer has been consumed. This allows
/// us to skip the overhead of, eg, repeated sys calls to read from disk as we
/// process a smaller number of bytes.
pub struct AsyncBufReader<'a, T: AsyncRead + Unpin + Sized> {
    inner: &'a mut T,
    offset: usize,
    capacity: usize,
    buffer: [u8; DEFAULT_BUF_SIZE],
}

impl<'a, T: AsyncRead + Unpin + Sized> AsyncBufReader<'a, T> {
    pub fn new(inner: &'a mut T) -> Self {
        AsyncBufReader {
            inner,
            offset: 0,
            capacity: 0,
            buffer: [0; DEFAULT_BUF_SIZE],
        }
    }
}

impl<'a, T: AsyncRead + Unpin + Sized> AsyncRead for AsyncBufReader<'a, T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        let inner = Pin::new(&mut self.get_mut().inner);
        inner.poll_read(cx, buf)
    }
}

impl<'a, T: AsyncRead + Unpin + Sized> AsyncBufRead for AsyncBufReader<'a, T> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<IoResult<&[u8]>> {
        let this = self.get_mut();
        if this.offset >= this.capacity {
            let inner = Pin::new(&mut this.inner);
            let mut buf = ReadBuf::new(&mut this.buffer);
            match inner.poll_read(cx, &mut buf) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            };

            this.capacity = buf.filled().len();
            this.offset = 0;
        }
        Poll::Ready(Ok(&this.buffer[this.offset..this.capacity]))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.get_mut().offset += amt;
    }
}
