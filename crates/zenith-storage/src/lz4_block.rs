//! `lz4-java` `LZ4Block` stream framing (compression ID 4).
//!
//! This implements the `lz4-java` block-stream wire format (NOT the standard LZ4 Frame Format):
//! - 21-byte block header:
//!   - `0..8`: ASCII magic `"LZ4Block"` (`4C 5A 34 42 6C 6F 63 6B`)
//!   - `8`: `token = method | compression_level`
//!     - `method = 0x10` (raw/uncompressed) or `0x20` (LZ4 raw block compressed)
//!     - `compression_level = max(0, ceil(log2(block_size)) - 10)` (`6` for default 65,536-byte blocks)
//!   - `9..13`: `compressed_length` (little-endian `u32`, non-negative signed 32-bit int)
//!   - `13..17`: `decompressed_length` (little-endian `u32`, non-negative signed 32-bit int)
//!   - `17..21`: `checksum = xxHash32(uncompressed, seed = 0x9747B28C) & 0x0FFFFFFF` (little-endian `u32`)
//! - End-of-stream terminator block: 21-byte header with `token = 0x10 | compression_level`,
//!   `compressed_length = 0`, `decompressed_length = 0`, `checksum = 0`, and 0 payload bytes.

use std::fmt;
use std::io::{self, Read, Write};

use xxhash_rust::xxh32::xxh32;

/// 8-byte ASCII magic header `"LZ4Block"`.
pub const LZ4_BLOCK_MAGIC: [u8; 8] = *b"LZ4Block";

/// Fixed block header size in bytes.
pub const LZ4_BLOCK_HEADER_LEN: usize = 21;

/// Default block size in bytes (64 KiB = `2^16`).
pub const LZ4_DEFAULT_BLOCK_SIZE: usize = 65_536;

/// Minimum allowed block size in bytes (`64`).
pub const LZ4_MIN_BLOCK_SIZE: usize = 64;

/// Maximum allowed block size in bytes (`32 MiB = 1 << 25`).
pub const LZ4_MAX_BLOCK_SIZE: usize = 1 << 25;

/// Compression method nibble for raw (uncompressed) blocks.
pub const LZ4_METHOD_RAW: u8 = 0x10;

/// Compression method nibble for LZ4-compressed blocks.
pub const LZ4_METHOD_LZ4: u8 = 0x20;

/// xxHash32 seed used by `lz4-java` `LZ4Block` streams.
pub const LZ4_XXHASH32_SEED: u32 = 0x9747_B28C;

/// 28-bit checksum mask applied to xxHash32 output (clears the top 4 bits).
pub const LZ4_CHECKSUM_MASK: u32 = 0x0FFF_FFFF;

/// Computes the 28-bit masked xxHash32 checksum for an uncompressed block.
#[must_use]
pub fn lz4_block_checksum(uncompressed: &[u8]) -> u32 {
    xxh32(uncompressed, LZ4_XXHASH32_SEED) & LZ4_CHECKSUM_MASK
}

/// Computes `compression_level = max(0, ceil(log2(block_size)) - 10)` for `block_size`.
#[must_use]
pub fn compression_level_for_block_size(block_size: usize) -> u8 {
    debug_assert!((LZ4_MIN_BLOCK_SIZE..=LZ4_MAX_BLOCK_SIZE).contains(&block_size));
    let ceil_log2 = if block_size <= 1 {
        0u32
    } else {
        usize::BITS - (block_size - 1).leading_zeros()
    };
    ceil_log2.saturating_sub(10) as u8
}

/// Streaming writer for the `lz4-java` `LZ4Block` format (`sync_flush = false`).
pub struct Lz4BlockWriter<W: Write> {
    inner: Option<W>,
    buffer: Vec<u8>,
    block_size: usize,
    compression_level: u8,
    finished: bool,
}

impl<W: Write> fmt::Debug for Lz4BlockWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lz4BlockWriter")
            .field("buffered", &self.buffer.len())
            .field("block_size", &self.block_size)
            .field("compression_level", &self.compression_level)
            .field("finished", &self.finished)
            .finish()
    }
}

impl<W: Write> Lz4BlockWriter<W> {
    /// Creates a new `LZ4Block` writer with the default 65,536-byte block size (level `6`).
    #[must_use]
    pub fn new(writer: W) -> Self {
        Self::with_block_size(writer, LZ4_DEFAULT_BLOCK_SIZE)
    }

    /// Creates a new `LZ4Block` writer with a custom `block_size` (clamped to `64..=33_554_432`).
    #[must_use]
    pub fn with_block_size(writer: W, block_size: usize) -> Self {
        let block_size = block_size.clamp(LZ4_MIN_BLOCK_SIZE, LZ4_MAX_BLOCK_SIZE);
        let compression_level = compression_level_for_block_size(block_size);
        Self {
            inner: Some(writer),
            buffer: Vec::with_capacity(block_size),
            block_size,
            compression_level,
            finished: false,
        }
    }

    /// Flushes any buffered bytes as a final data block, emits the 21-byte zero-length
    /// terminator block, flushes the underlying writer, and returns it.
    pub fn finish(mut self) -> io::Result<W> {
        self.finish_inner()?;
        let mut inner = self
            .inner
            .take()
            .expect("Lz4BlockWriter inner present before finish");
        inner.flush()?;
        Ok(inner)
    }

    fn finish_inner(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if !self.buffer.is_empty() {
            self.flush_buffered_block()?;
        }
        let header = encode_header(LZ4_METHOD_RAW | self.compression_level, 0, 0, 0);
        if let Some(w) = self.inner.as_mut() {
            w.write_all(&header)?;
        }
        Ok(())
    }

    fn flush_buffered_block(&mut self) -> io::Result<()> {
        let uncompressed = &self.buffer;
        if uncompressed.is_empty() {
            return Ok(());
        }
        let n = uncompressed.len();
        let checksum = lz4_block_checksum(uncompressed);
        let compressed = lz4_flex::block::compress(uncompressed);

        let w = self
            .inner
            .as_mut()
            .ok_or_else(|| io::Error::other("LZ4BlockWriter already finished"))?;

        if compressed.len() >= n {
            let header = encode_header(
                LZ4_METHOD_RAW | self.compression_level,
                n as u32,
                n as u32,
                checksum,
            );
            w.write_all(&header)?;
            w.write_all(uncompressed)?;
        } else {
            let header = encode_header(
                LZ4_METHOD_LZ4 | self.compression_level,
                compressed.len() as u32,
                n as u32,
                checksum,
            );
            w.write_all(&header)?;
            w.write_all(&compressed)?;
        }

        self.buffer.clear();
        Ok(())
    }
}

impl<W: Write> Write for Lz4BlockWriter<W> {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        if self.finished {
            return Err(io::Error::other("write after Lz4BlockWriter::finish"));
        }
        let total = buf.len();
        while !buf.is_empty() {
            let space = self.block_size - self.buffer.len();
            let take = buf.len().min(space);
            self.buffer.extend_from_slice(&buf[..take]);
            buf = &buf[take..];
            if self.buffer.len() == self.block_size {
                self.flush_buffered_block()?;
            }
        }
        Ok(total)
    }

    /// With `sync_flush = false` (per spec), does not force a partial block; only flushes
    /// the underlying writer.
    fn flush(&mut self) -> io::Result<()> {
        if let Some(w) = self.inner.as_mut() {
            w.flush()?;
        }
        Ok(())
    }
}

impl<W: Write> Drop for Lz4BlockWriter<W> {
    fn drop(&mut self) {
        if !self.finished && self.inner.is_some() {
            let _ = self.finish_inner();
        }
    }
}

/// Streaming reader for the `lz4-java` `LZ4Block` format.
pub struct Lz4BlockReader<R: Read> {
    inner: R,
    decoded: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl<R: Read> fmt::Debug for Lz4BlockReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lz4BlockReader")
            .field("decoded_len", &self.decoded.len())
            .field("pos", &self.pos)
            .field("eof", &self.eof)
            .finish()
    }
}

impl<R: Read> Lz4BlockReader<R> {
    /// Creates a new `LZ4Block` reader wrapping `reader`.
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            decoded: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    /// Consumes the reader and returns the underlying stream.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.inner
    }

    fn refill(&mut self) -> io::Result<()> {
        self.decoded.clear();
        self.pos = 0;

        let mut header = [0u8; LZ4_BLOCK_HEADER_LEN];
        read_exact_full(&mut self.inner, &mut header)?;

        if header[0..8] != LZ4_BLOCK_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid LZ4Block magic",
            ));
        }

        let token = header[8];
        let method = token & 0xF0;
        let level = token & 0x0F;
        if method != LZ4_METHOD_RAW && method != LZ4_METHOD_LZ4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid LZ4Block method nibble: 0x{method:02x}"),
            ));
        }
        let max_block_size: usize = 1usize << (usize::from(level) + 10);

        let compressed_len_i32 =
            i32::from_le_bytes([header[9], header[10], header[11], header[12]]);
        let decompressed_len_i32 =
            i32::from_le_bytes([header[13], header[14], header[15], header[16]]);
        let expected_checksum =
            u32::from_le_bytes([header[17], header[18], header[19], header[20]]);

        if compressed_len_i32 < 0 || decompressed_len_i32 < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "negative LZ4Block length",
            ));
        }
        let compressed_len = compressed_len_i32 as usize;
        let decompressed_len = decompressed_len_i32 as usize;

        if (decompressed_len == 0) != (compressed_len == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "inconsistent zero/non-zero LZ4Block lengths",
            ));
        }
        if decompressed_len > max_block_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "LZ4Block decompressed length {decompressed_len} exceeds max block size {max_block_size}"
                ),
            ));
        }
        if method == LZ4_METHOD_RAW && compressed_len != decompressed_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LZ4Block raw block compressed_len != decompressed_len",
            ));
        }

        // Terminator block.
        if decompressed_len == 0 {
            if expected_checksum != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "non-zero checksum in LZ4Block terminator",
                ));
            }
            self.eof = true;
            return Ok(());
        }

        let mut payload = vec![0u8; compressed_len];
        read_exact_full(&mut self.inner, &mut payload)?;

        let uncompressed = if method == LZ4_METHOD_RAW {
            payload
        } else {
            let out = lz4_flex::block::decompress(&payload, decompressed_len).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("LZ4Block decompression failed: {e}"),
                )
            })?;
            if out.len() != decompressed_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "LZ4Block decompressed size mismatch: expected {decompressed_len}, got {}",
                        out.len()
                    ),
                ));
            }
            out
        };

        let actual_checksum = lz4_block_checksum(&uncompressed);
        if actual_checksum != expected_checksum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "LZ4Block checksum mismatch: expected 0x{expected_checksum:08x}, got 0x{actual_checksum:08x}"
                ),
            ));
        }

        self.decoded = uncompressed;
        Ok(())
    }
}

impl<R: Read> Read for Lz4BlockReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.eof {
            return Ok(0);
        }
        if self.pos >= self.decoded.len() {
            self.refill()?;
            if self.eof {
                return Ok(0);
            }
        }
        let avail = &self.decoded[self.pos..];
        let n = buf.len().min(avail.len());
        buf[..n].copy_from_slice(&avail[..n]);
        self.pos += n;
        Ok(n)
    }
}

fn encode_header(
    token: u8,
    compressed_len: u32,
    decompressed_len: u32,
    checksum: u32,
) -> [u8; LZ4_BLOCK_HEADER_LEN] {
    let mut header = [0u8; LZ4_BLOCK_HEADER_LEN];
    header[0..8].copy_from_slice(&LZ4_BLOCK_MAGIC);
    header[8] = token;
    header[9..13].copy_from_slice(&compressed_len.to_le_bytes());
    header[13..17].copy_from_slice(&decompressed_len.to_le_bytes());
    header[17..21].copy_from_slice(&checksum.to_le_bytes());
    header
}

fn read_exact_full<R: Read>(r: &mut R, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        match r.read(buf) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF inside LZ4Block stream",
                ));
            }
            Ok(n) => buf = &mut buf[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
