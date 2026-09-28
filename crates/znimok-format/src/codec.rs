//! Little-endian primitives and the tag–length–value frame shared by blocks and object fields.

use crate::{FormatError, Limits};

pub type Tag = [u8; 4];

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
    open: Vec<usize>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    /// `u32` byte length + UTF-8, no terminator.
    pub fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }
    /// Opens a TLV record; the length is patched by [`Writer::close`].
    pub fn open(&mut self, tag: &Tag) {
        self.buf.extend_from_slice(tag);
        self.open.push(self.buf.len());
        self.u32(0);
    }
    pub fn close(&mut self) {
        let at = self.open.pop().expect("close without open");
        let len = (self.buf.len() - at - 4) as u32;
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }
    /// A whole TLV record written by `f`.
    pub fn record(&mut self, tag: &Tag, f: impl FnOnce(&mut Writer)) {
        self.open(tag);
        f(self);
        self.close();
    }
}

/// Bounds-checked reader over a byte slice. Every read fails cleanly instead of panicking.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pub at: usize,
    limits: &'a Limits,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], limits: &'a Limits) -> Self {
        Self {
            data,
            at: 0,
            limits,
        }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if n > self.remaining() {
            return Err(FormatError::Corrupt(format!(
                "unexpected end at byte {} (need {n})",
                self.at
            )));
        }
        let s = &self.data[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }

    fn arr<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        Ok(self.take(N)?.try_into().expect("length checked"))
    }

    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool, FormatError> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    pub fn i64(&mut self) -> Result<i64, FormatError> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    pub fn f32(&mut self) -> Result<f32, FormatError> {
        let v = f32::from_le_bytes(self.arr()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(FormatError::Corrupt("non-finite number".into()))
        }
    }

    pub fn str(&mut self) -> Result<String, FormatError> {
        let n = self.u32()? as usize;
        if n > self.limits.max_string {
            return Err(FormatError::Corrupt(format!(
                "string of {n} bytes exceeds the limit"
            )));
        }
        let b = self.take(n)?;
        String::from_utf8(b.to_vec())
            .map_err(|_| FormatError::Corrupt("string is not UTF-8".into()))
    }

    /// Tag of the next record without consuming anything.
    pub fn peek_tag(&self) -> Option<Tag> {
        self.data
            .get(self.at..self.at + 4)
            .map(|t| t.try_into().expect("4 bytes"))
    }

    /// Next TLV record: tag and a sub-reader over exactly its value.
    pub fn record(&mut self) -> Result<(Tag, Reader<'a>), FormatError> {
        let tag: Tag = self.arr()?;
        let len = self.u32()? as usize;
        let body = self.take(len).map_err(|_| {
            FormatError::Corrupt(format!("record {} runs past the end", tag_str(&tag)))
        })?;
        Ok((
            tag,
            Reader {
                data: body,
                at: 0,
                limits: self.limits,
            },
        ))
    }
}

pub fn tag_str(t: &Tag) -> String {
    t.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}
