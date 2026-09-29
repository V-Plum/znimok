//! A bounds-checked cursor over a byte slice: every read returns `Truncated` instead of
//! panicking, and [`Cursor::need`] validates `count × size` before a table is allocated.

use super::{CheckError, CheckResult};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Cursor<'a> {
    pub fn new(b: &'a [u8], what: &'static str) -> Self {
        Self { b, pos: 0, what }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.b.len()
    }

    pub fn take(&mut self, n: usize) -> CheckResult<&'a [u8]> {
        if n > self.remaining() {
            return Err(CheckError::Truncated(self.what));
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> CheckResult<()> {
        self.take(n).map(|_| ())
    }

    /// There are at least `count` entries of `size` bytes left (no overflow, no allocation).
    pub fn need(&self, count: u64, size: u64) -> CheckResult<()> {
        match count.checked_mul(size) {
            Some(n) if n <= self.remaining() as u64 => Ok(()),
            _ => Err(CheckError::Truncated(self.what)),
        }
    }

    pub fn u8(&mut self) -> CheckResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn arr<const N: usize>(&mut self) -> CheckResult<[u8; N]> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    pub fn be16(&mut self) -> CheckResult<u16> {
        self.arr().map(u16::from_be_bytes)
    }

    pub fn be32(&mut self) -> CheckResult<u32> {
        self.arr().map(u32::from_be_bytes)
    }

    pub fn be64(&mut self) -> CheckResult<u64> {
        self.arr().map(u64::from_be_bytes)
    }

    pub fn le16(&mut self) -> CheckResult<u16> {
        self.arr().map(u16::from_le_bytes)
    }

    pub fn fourcc(&mut self) -> CheckResult<[u8; 4]> {
        self.arr()
    }
}
