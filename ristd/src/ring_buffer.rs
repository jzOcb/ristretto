//! In-memory PTY scrollback storage.

/// Simple byte ring buffer with fixed capacity.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    capacity: usize,
    buffer: Vec<u8>,
}

impl RingBuffer {
    /// Creates a new ring buffer with the provided capacity in bytes.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: Vec::with_capacity(capacity.min(4096)),
        }
    }

    /// Appends bytes to the buffer, dropping the oldest bytes when required.
    pub fn push(&mut self, data: &[u8]) {
        if data.len() >= self.capacity {
            self.buffer.clear();
            self.buffer.extend_from_slice(&data[data.len() - self.capacity..]);
            return;
        }

        let overflow = self
            .buffer
            .len()
            .saturating_add(data.len())
            .saturating_sub(self.capacity);
        if overflow > 0 {
            self.buffer.drain(..overflow);
        }
        self.buffer.extend_from_slice(data);
    }

    /// Returns the last `n_bytes` bytes as a contiguous slice.
    #[must_use]
    pub fn tail(&self, n_bytes: usize) -> &[u8] {
        let start = self.buffer.len().saturating_sub(n_bytes);
        &self.buffer[start..]
    }

    /// Returns the last `n` UTF-8-decoded lines.
    #[must_use]
    pub fn tail_lines(&self, n: usize) -> Vec<String> {
        let text = String::from_utf8_lossy(&self.buffer);
        let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
        let keep_from = lines.len().saturating_sub(n);
        lines.drain(..keep_from);
        lines
    }

    /// Returns the full buffered contents.
    #[must_use]
    pub fn drain_all(&self) -> Vec<u8> {
        self.buffer.clone()
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::RingBuffer;

    #[test]
    fn push_and_tail() {
        let mut buffer = RingBuffer::new(8);
        buffer.push(b"abcd");
        buffer.push(b"ef");
        assert_eq!(buffer.tail(4), b"cdef");
    }

    #[test]
    fn wrap_around_discards_oldest_bytes() {
        let mut buffer = RingBuffer::new(5);
        buffer.push(b"abc");
        buffer.push(b"def");
        assert_eq!(buffer.drain_all(), b"bcdef");
    }

    #[test]
    fn tail_lines_returns_latest_lines() {
        let mut buffer = RingBuffer::new(64);
        buffer.push(b"one\ntwo\nthree\nfour\n");
        assert_eq!(buffer.tail_lines(2), vec!["three".to_owned(), "four".to_owned()]);
    }
}

