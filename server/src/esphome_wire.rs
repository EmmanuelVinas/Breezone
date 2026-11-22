//! The native API of ESPHome as the fake devices speak it (`mock_devices`) and the tests: the few
//! protobuf messages used, by hand, and the frames in clear (the module `esphome` is what drives
//! real nodes).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ---------------------------------------------------------------------------------------
// Protobuf, by hand: the few messages used
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Varint(u64),
    Fixed32(u32),
    Fixed64(u64),
    Bytes(Vec<u8>),
}

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_varint(data: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *data.get(*i)?;
        *i += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// A message being written.
#[derive(Default)]
pub struct Msg(pub Vec<u8>);

impl Msg {
    pub fn varint(mut self, field: u32, v: u64) -> Self {
        put_varint(&mut self.0, u64::from(field) << 3);
        put_varint(&mut self.0, v);
        self
    }
    pub fn bool(self, field: u32, v: bool) -> Self {
        self.varint(field, u64::from(v))
    }
    pub fn fixed32(mut self, field: u32, v: u32) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | 5);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn float(self, field: u32, v: f32) -> Self {
        self.fixed32(field, v.to_bits())
    }
    pub fn string(mut self, field: u32, v: &str) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | 2);
        put_varint(&mut self.0, v.len() as u64);
        self.0.extend_from_slice(v.as_bytes());
        self
    }
}

/// The fields of a message (a repeated one appears several times).
pub fn decode(data: &[u8]) -> Vec<(u32, Field)> {
    let mut out = vec![];
    let mut i = 0;
    while i < data.len() {
        let Some(tag) = get_varint(data, &mut i) else { break };
        let (field, wire) = ((tag >> 3) as u32, tag & 7);
        let value = match wire {
            0 => match get_varint(data, &mut i) {
                Some(v) => Field::Varint(v),
                None => break,
            },
            1 => {
                let Some(b) = data.get(i..i + 8) else { break };
                i += 8;
                Field::Fixed64(u64::from_le_bytes(b.try_into().unwrap_or_default()))
            }
            2 => {
                let Some(len) = get_varint(data, &mut i) else { break };
                let Some(b) = data.get(i..i + len as usize) else { break };
                i += len as usize;
                Field::Bytes(b.to_vec())
            }
            5 => {
                let Some(b) = data.get(i..i + 4) else { break };
                i += 4;
                Field::Fixed32(u32::from_le_bytes(b.try_into().unwrap_or_default()))
            }
            _ => break,
        };
        out.push((field, value));
    }
    out
}

/// Reading the fields of a decoded message.
pub struct Fields(pub Vec<(u32, Field)>);

impl Fields {
    fn get(&self, n: u32) -> Option<&Field> {
        self.0.iter().find(|(f, _)| *f == n).map(|(_, v)| v)
    }
    pub fn string(&self, n: u32) -> String {
        match self.get(n) {
            Some(Field::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    }
    pub fn uint(&self, n: u32) -> u64 {
        match self.get(n) {
            Some(Field::Varint(v)) => *v,
            _ => 0,
        }
    }
    pub fn bool(&self, n: u32) -> bool {
        self.uint(n) != 0
    }
    pub fn fixed32(&self, n: u32) -> u32 {
        match self.get(n) {
            Some(Field::Fixed32(v)) => *v,
            _ => 0,
        }
    }
    pub fn float(&self, n: u32) -> Option<f32> {
        match self.get(n) {
            Some(Field::Fixed32(v)) => Some(f32::from_bits(*v)),
            _ => None,
        }
    }
    /// A repeated varint, packed or not.
    pub fn uints(&self, n: u32) -> Vec<u64> {
        let mut out = vec![];
        for (f, v) in &self.0 {
            if *f != n {
                continue;
            }
            match v {
                Field::Varint(x) => out.push(*x),
                Field::Bytes(b) => {
                    let mut i = 0;
                    while i < b.len() {
                        match get_varint(b, &mut i) {
                            Some(x) => out.push(x),
                            None => break,
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// Frames in clear: `0x00, varint(len), varint(type), message`
// ---------------------------------------------------------------------------------------

pub struct Frames<S> {
    stream: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Frames<S> {
    pub fn plain(stream: S) -> Self {
        Self { stream }
    }

    pub async fn send(&mut self, kind: u16, msg: &[u8]) -> std::io::Result<()> {
        let mut f = vec![0x00];
        put_varint(&mut f, msg.len() as u64);
        put_varint(&mut f, u64::from(kind));
        f.extend_from_slice(msg);
        self.stream.write_all(&f).await
    }

    pub async fn recv(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        if self.stream.read_u8().await? != 0x00 {
            return Err(std::io::Error::other("not a frame in clear"));
        }
        let len = read_varint(&mut self.stream).await?;
        let kind = read_varint(&mut self.stream).await?;
        let mut msg = vec![0u8; len as usize];
        self.stream.read_exact(&mut msg).await?;
        Ok((kind as u16, msg))
    }
}

async fn read_varint<S: AsyncRead + Unpin>(s: &mut S) -> std::io::Result<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = s.read_u8().await?;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(std::io::Error::other("bad varint"))
}
