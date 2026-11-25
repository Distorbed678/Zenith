//! `ZPD1` dump format. Specified in `docs/specs/parity/dump-format.md`.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use anyhow::{Context, bail, ensure};

const MAGIC: &[u8; 4] = b"ZPD1";

/// Well-known phases. Unknown values from newer writers are preserved as raw `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase(pub u16);

impl Phase {
    pub const TICK_START: Phase = Phase(0);
    pub const TICK_END: Phase = Phase(1);
    pub const CHUNK_GENERATED: Phase = Phase(10);
    pub const PACKET_OUT: Phase = Phase(20);
    pub const PACKET_IN: Phase = Phase(21);
    pub const ENTITY_STATE: Phase = Phase(30);

    pub fn name(self) -> String {
        match self.0 {
            0 => "tick_start".into(),
            1 => "tick_end".into(),
            10 => "chunk_generated".into(),
            20 => "packet_out".into(),
            21 => "packet_in".into(),
            30 => "entity_state".into(),
            n => format!("phase_{n}"),
        }
    }
}

/// A field value. Floats keep their raw bits so comparisons are bit-exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    I64(i64),
    F64Bits(u64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn f64(v: f64) -> Self {
        Value::F64Bits(v.to_bits())
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::I64(v) => write!(f, "{v}"),
            Value::F64Bits(b) => write!(f, "{} (0x{b:016x})", f64::from_bits(*b)),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub source: String,
    pub version: String,
    pub scenario: String,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub tick: u64,
    pub phase: Phase,
    pub subject: String,
    pub fields: Vec<(String, Value)>,
}

impl Record {
    pub fn new(tick: u64, phase: Phase, subject: impl Into<String>) -> Self {
        Self {
            tick,
            phase,
            subject: subject.into(),
            fields: Vec::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: Value) -> Self {
        self.fields.push((key.into(), value));
        self
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) -> anyhow::Result<()> {
    let len = u16::try_from(s.len()).context("string longer than 65535 bytes")?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// Encode a record body (without its length prefix).
pub fn encode_body(r: &Record) -> anyhow::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(64);
    b.extend_from_slice(&r.tick.to_be_bytes());
    b.extend_from_slice(&r.phase.0.to_be_bytes());
    put_str(&mut b, &r.subject)?;
    b.extend_from_slice(
        &u16::try_from(r.fields.len())
            .context("too many fields")?
            .to_be_bytes(),
    );
    for (k, v) in &r.fields {
        put_str(&mut b, k)?;
        match v {
            Value::I64(x) => {
                b.push(0);
                b.extend_from_slice(&x.to_be_bytes());
            }
            Value::F64Bits(x) => {
                b.push(1);
                b.extend_from_slice(&x.to_be_bytes());
            }
            Value::Bool(x) => {
                b.push(2);
                b.push(u8::from(*x));
            }
            Value::Str(s) => {
                b.push(3);
                put_str(&mut b, s)?;
            }
            Value::Bytes(d) => {
                b.push(4);
                b.extend_from_slice(
                    &u32::try_from(d.len())
                        .context("blob too large")?
                        .to_be_bytes(),
                );
                b.extend_from_slice(d);
            }
        }
    }
    Ok(b)
}

/// Streaming writer.
#[derive(Debug)]
pub struct DumpWriter<W: Write> {
    out: W,
}

impl DumpWriter<BufWriter<File>> {
    pub fn create(path: &Path, header: &Header) -> anyhow::Result<Self> {
        let f = File::create(path).with_context(|| format!("create {}", path.display()))?;
        Self::new(BufWriter::new(f), header)
    }
}

impl<W: Write> DumpWriter<W> {
    pub fn new(mut out: W, header: &Header) -> anyhow::Result<Self> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC);
        put_str(&mut h, &header.source)?;
        put_str(&mut h, &header.version)?;
        put_str(&mut h, &header.scenario)?;
        h.extend_from_slice(&header.seed.to_be_bytes());
        out.write_all(&h)?;
        Ok(Self { out })
    }

    pub fn write(&mut self, r: &Record) -> anyhow::Result<()> {
        let body = encode_body(r)?;
        self.out.write_all(
            &u32::try_from(body.len())
                .context("record too large")?
                .to_be_bytes(),
        )?;
        self.out.write_all(&body)?;
        Ok(())
    }

    pub fn finish(mut self) -> anyhow::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> anyhow::Result<&[u8]> {
        ensure!(self.at + n <= self.b.len(), "truncated record");
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> anyhow::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> anyhow::Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn str(&mut self) -> anyhow::Result<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8(self.take(n)?.to_vec())?)
    }
}

/// Decode one record body.
pub fn decode_body(body: &[u8]) -> anyhow::Result<Record> {
    let mut c = Cursor { b: body, at: 0 };
    let tick = c.u64()?;
    let phase = Phase(c.u16()?);
    let subject = c.str()?;
    let n = c.u16()?;
    let mut fields = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let key = c.str()?;
        let v = match c.u8()? {
            0 => Value::I64(c.u64()? as i64),
            1 => Value::F64Bits(c.u64()?),
            2 => Value::Bool(c.u8()? != 0),
            3 => Value::Str(c.str()?),
            4 => {
                let len = c.u32()? as usize;
                Value::Bytes(c.take(len)?.to_vec())
            }
            t => bail!("unknown value type {t}"),
        };
        fields.push((key, v));
    }
    ensure!(c.at == body.len(), "trailing bytes in record");
    Ok(Record {
        tick,
        phase,
        subject,
        fields,
    })
}

/// Streaming reader; iterate to get records.
#[derive(Debug)]
pub struct DumpReader<R: Read> {
    input: R,
    pub header: Header,
}

impl DumpReader<BufReader<File>> {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::new(BufReader::new(f))
    }
}

fn read_str<R: Read>(r: &mut R) -> anyhow::Result<String> {
    let mut l = [0u8; 2];
    r.read_exact(&mut l)?;
    let mut s = vec![0u8; u16::from_be_bytes(l) as usize];
    r.read_exact(&mut s)?;
    Ok(String::from_utf8(s)?)
}

impl<R: Read> DumpReader<R> {
    pub fn new(mut input: R) -> anyhow::Result<Self> {
        let mut magic = [0u8; 4];
        input.read_exact(&mut magic)?;
        ensure!(&magic == MAGIC, "not a ZPD1 dump");
        let source = read_str(&mut input)?;
        let version = read_str(&mut input)?;
        let scenario = read_str(&mut input)?;
        let mut seed = [0u8; 8];
        input.read_exact(&mut seed)?;
        Ok(Self {
            input,
            header: Header {
                source,
                version,
                scenario,
                seed: u64::from_be_bytes(seed),
            },
        })
    }

    fn next_record(&mut self) -> anyhow::Result<Option<Record>> {
        let mut len = [0u8; 4];
        match self.input.read_exact(&mut len) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
        self.input
            .read_exact(&mut body)
            .context("truncated record")?;
        decode_body(&body).map(Some)
    }
}

impl<R: Read> Iterator for DumpReader<R> {
    type Item = anyhow::Result<Record>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_record().transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Record> {
        vec![
            Record::new(0, Phase::TICK_END, "server").with("tickCount", Value::I64(1)),
            Record::new(1, Phase::TICK_END, "minecraft:overworld")
                .with("gameTime", Value::I64(42))
                .with("x", Value::f64(-0.0))
                .with("ok", Value::Bool(true))
                .with("name", Value::Str("é✓".into()))
                .with("raw", Value::Bytes(vec![1, 2, 3])),
        ]
    }

    #[test]
    fn roundtrip() {
        let header = Header {
            source: "zenith".into(),
            version: "26.3".into(),
            scenario: "t".into(),
            seed: 7,
        };
        let mut w = DumpWriter::new(Vec::new(), &header).unwrap();
        for r in sample() {
            w.write(&r).unwrap();
        }
        let bytes = w.finish().unwrap();
        let r = DumpReader::new(&bytes[..]).unwrap();
        assert_eq!(r.header, header);
        let back: Vec<Record> = r.map(Result::unwrap).collect();
        assert_eq!(back, sample());
    }

    #[test]
    fn negative_zero_is_not_zero() {
        assert_ne!(Value::f64(-0.0), Value::f64(0.0));
    }
}
