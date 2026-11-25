//! Region file compression schemes (IDs 1, 2, 3, 4, and custom sentinel 127).

use std::io::{BufReader, BufWriter, Read, Write};

use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::{Compression, GzBuilder};

use crate::error::{Result, StorageError};
use crate::layout::Identifier;
use crate::lz4_block::{Lz4BlockReader, Lz4BlockWriter};

/// Buffer capacity (8,192 bytes) used around chunk compression and decompression streams.
pub const STREAM_BUFFER_SIZE: usize = 8_192;

/// Bit 7 flag on chunk version bytes indicating an external `c.<x>.<z>.mcc` stream.
pub const EXTERNAL_STREAM_FLAG: u8 = 0x80;

/// Compression scheme identifiers stored in bits `0..=6` of a chunk's version byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CompressionId {
    /// ID `1` — GZip (RFC 1952). Read-only for region chunks (cannot be selected in `server.properties`).
    Gzip = 1,
    /// ID `2` — Zlib / Deflate (RFC 1950). Default write compression (`"deflate"`).
    #[default]
    Zlib = 2,
    /// ID `3` — Raw uncompressed NBT (`"none"`).
    None = 3,
    /// ID `4` — `lz4-java` `LZ4Block` stream (`"lz4"`).
    Lz4 = 4,
    /// ID `127` — Custom compression sentinel (registered in valid table; rejected on full read and write).
    Custom = 127,
}

impl CompressionId {
    /// Converts a raw 7-bit compression ID into a known [`CompressionId`].
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::Gzip),
            2 => Some(Self::Zlib),
            3 => Some(Self::None),
            4 => Some(Self::Lz4),
            127 => Some(Self::Custom),
            _ => None,
        }
    }

    /// Returns the numeric 7-bit compression ID.
    #[must_use]
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// Returns the external version byte (`id | 0x80`).
    #[must_use]
    pub const fn external_id(self) -> u8 {
        (self as u8) | EXTERNAL_STREAM_FLAG
    }

    /// Parses a `region-file-compression` `server.properties` value (`"deflate"`, `"none"`, `"lz4"`).
    ///
    /// Note that `"gzip"` (ID `1`) and custom (ID `127`) are intentionally rejected.
    pub fn from_server_property(value: &str) -> Result<Self> {
        match value {
            "deflate" => Ok(Self::Zlib),
            "none" => Ok(Self::None),
            "lz4" => Ok(Self::Lz4),
            other => {
                log::error!("Invalid region-file-compression value: {other:?}");
                Err(StorageError::InvalidCompressionName(other.to_owned()))
            }
        }
    }

    /// Returns the `server.properties` name for writable compression schemes, or `None` for read-only IDs.
    #[must_use]
    pub const fn server_property_name(self) -> Option<&'static str> {
        match self {
            Self::Zlib => Some("deflate"),
            Self::None => Some("none"),
            Self::Lz4 => Some("lz4"),
            Self::Gzip | Self::Custom => None,
        }
    }

    /// Returns `true` if this compression ID is supported for writing region chunks.
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(self, Self::Zlib | Self::None | Self::Lz4)
    }

    /// Updates `current` from a `server.properties` value, logging an error and keeping `current`
    /// unchanged if `value` is invalid (matching vanilla config behavior).
    #[must_use]
    pub fn apply_server_property(current: Self, value: &str) -> Self {
        match Self::from_server_property(value) {
            Ok(next) => next,
            Err(_) => current,
        }
    }
}

/// Builds a gzip encoder configured with RFC 1952 `mtime = 0`, OS byte `0xFF`, and default level (6).
#[must_use]
pub fn new_gzip_encoder<W: Write>(writer: W) -> GzEncoder<W> {
    GzBuilder::new()
        .mtime(0)
        .operating_system(0xFF)
        .write(writer, Compression::default())
}

/// Compresses `write_fn`'s output into `out` using `compression` (8,192-byte buffered writer).
///
/// Fails with [`StorageError::UnsupportedCompressionForWrite`] for read-only IDs (`Gzip`, `Custom`).
pub fn compress_chunk_payload<F>(
    compression: CompressionId,
    out: &mut Vec<u8>,
    write_fn: F,
) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    match compression {
        CompressionId::Zlib => {
            let encoder = ZlibEncoder::new(out, Compression::default());
            let mut buffered = BufWriter::with_capacity(STREAM_BUFFER_SIZE, encoder);
            write_fn(&mut buffered)?;
            let encoder = buffered.into_inner().map_err(|e| e.into_error())?;
            encoder.finish()?;
            Ok(())
        }
        CompressionId::None => {
            let mut buffered = BufWriter::with_capacity(STREAM_BUFFER_SIZE, out);
            write_fn(&mut buffered)?;
            buffered.flush()?;
            Ok(())
        }
        CompressionId::Lz4 => {
            let encoder = Lz4BlockWriter::new(out);
            let mut buffered = BufWriter::with_capacity(STREAM_BUFFER_SIZE, encoder);
            write_fn(&mut buffered)?;
            let encoder = buffered.into_inner().map_err(|e| e.into_error())?;
            encoder.finish()?;
            Ok(())
        }
        CompressionId::Gzip | CompressionId::Custom => Err(
            StorageError::UnsupportedCompressionForWrite(compression.id()),
        ),
    }
}

/// Wraps a raw compressed byte stream in the decompressor for `compression` (8,192-byte read buffer).
///
/// For [`CompressionId::Custom`] (`127`), intercepts the stream before decoding: reads one modified-UTF-8
/// string (propagating any I/O or modified-UTF-8 error), validates `<namespace>:<path>` syntax,
/// logs an error either way, and returns `Ok(None)` (chunk treated as missing).
pub fn open_chunk_decompressor<'a>(
    compression: CompressionId,
    mut stream: Box<dyn Read + 'a>,
) -> Result<Option<Box<dyn Read + 'a>>> {
    match compression {
        CompressionId::Gzip => Ok(Some(Box::new(BufReader::with_capacity(
            STREAM_BUFFER_SIZE,
            GzDecoder::new(stream),
        )))),
        CompressionId::Zlib => Ok(Some(Box::new(BufReader::with_capacity(
            STREAM_BUFFER_SIZE,
            ZlibDecoder::new(stream),
        )))),
        CompressionId::None => Ok(Some(Box::new(BufReader::with_capacity(
            STREAM_BUFFER_SIZE,
            stream,
        )))),
        CompressionId::Lz4 => Ok(Some(Box::new(BufReader::with_capacity(
            STREAM_BUFFER_SIZE,
            Lz4BlockReader::new(stream),
        )))),
        CompressionId::Custom => {
            // Read modified-UTF-8 string (`u16` BE byte length + bytes); I/O errors propagate.
            let mut len_buf = [0u8; 2];
            stream.read_exact(&mut len_buf)?;
            let byte_len = usize::from(u16::from_be_bytes(len_buf));
            let mut bytes = vec![0u8; byte_len];
            stream.read_exact(&mut bytes)?;
            let custom_name = zenith_nbt::mutf8::decode_mutf8(&bytes)?;
            let lossy = custom_name.to_string_lossy();
            if Identifier::is_valid(&lossy) {
                log::error!("Unsupported custom chunk compression version: {lossy}");
            } else {
                log::error!("Invalid custom chunk compression identifier: {lossy:?}");
            }
            Ok(None)
        }
    }
}
