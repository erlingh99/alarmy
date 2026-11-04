use core::fmt;

/// Formats into `buf`, truncating on a char boundary. Nothing is appended after the first cut,
/// and `finish` ends the text with `marker` if truncated, then always with `suffix`.
pub struct TruncatingWriter<'a> {
    buf: &'a mut [u8],
    overflow_marker: Option<&'static str>,
    suffix: Option<&'static str>,
    len: usize,
    truncated: bool,
}

impl<'a> TruncatingWriter<'a> {
    /// `overflow_marker` and `suffix` must fit in `buf`
    pub fn new(
        buf: &'a mut [u8],
        overflow_marker: Option<&'static str>,
        suffix: Option<&'static str>,
    ) -> Self {
        assert!(
            overflow_marker.map_or(0, |m| m.len()) + suffix.map_or(0, |s| s.len()) <= buf.len(),
            "overflow_marker and suffix must fit in buf"
        );

        Self {
            buf,
            overflow_marker,
            suffix,
            len: 0,
            truncated: false,
        }
    }

    pub fn finish(mut self) -> &'a str {
        if self.truncated && self.overflow_marker.is_some() {
            self.append(self.overflow_marker.unwrap());
        }
        if let Some(sfx) = self.suffix {
            self.append(sfx);
        }
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or_default()
    }

    fn append(&mut self, s: &str) {
        self.buf[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        self.len += s.len();
    }
}

impl fmt::Write for TruncatingWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.truncated {
            return Ok(());
        }
        let limit = self.buf.len()
            - self.overflow_marker.map_or(0, |m| m.len())
            - self.suffix.map_or(0, |s| s.len());
        let mut n = s.len().min(limit - self.len);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.append(&s[..n]);
        self.truncated = n < s.len();
        Ok(())
    }
}
