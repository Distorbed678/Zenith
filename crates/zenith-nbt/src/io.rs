//! Binary NBT reader and writer for named-root (file), unnamed-root (network),
//! gzip-compressed, and zlib-compressed streams.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};

use crate::accounter::{Accounter, charges};
use crate::compound::Compound;
use crate::error::{Error, FormatError};
use crate::list::List;
use crate::mutf8::{AsNbtString, NbtString, decode_mutf8};
use crate::tag::{Tag, TagType};

/// Cap on upfront `Vec::with_capacity` allocations when decoding from a stream,
/// preventing OOM on malformed inputs even when using [`Accounter::unlimited`].
const MAX_PREALLOC_ELEMS: usize = 65_536;

// --- Low-level big-endian primitives ---

fn read_u8<R: Read>(r: &mut R) -> Result<u8, Error> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_i8<R: Read>(r: &mut R) -> Result<i8, Error> {
    Ok(read_u8(r)? as i8)
}

fn read_u16<R: Read>(r: &mut R) -> Result<u16, Error> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_be_bytes(buf))
}

fn read_i16<R: Read>(r: &mut R) -> Result<i16, Error> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(i16::from_be_bytes(buf))
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32, Error> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_i32<R: Read>(r: &mut R) -> Result<i32, Error> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(i32::from_be_bytes(buf))
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64, Error> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

fn read_i64<R: Read>(r: &mut R) -> Result<i64, Error> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(i64::from_be_bytes(buf))
}

fn read_mutf8_string<R: Read>(r: &mut R) -> Result<NbtString, Error> {
    let byte_len = usize::from(read_u16(r)?);
    let mut buf = vec![0u8; byte_len];
    r.read_exact(&mut buf)?;
    decode_mutf8(&buf)
}

fn write_mutf8_string<W: Write>(w: &mut W, s: &impl AsNbtString) -> Result<(), Error> {
    let encoded = s.encode_mutf8()?;
    let len = u16::try_from(encoded.len()).map_err(|_| Error::StringTooLong {
        encoded_len: encoded.len(),
    })?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&encoded)?;
    Ok(())
}

// --- Core iterative read / write ---

enum ReadFrame {
    Compound {
        compound: Compound,
        pending_key: Option<NbtString>,
    },
    List {
        list: List,
        elem_type: TagType,
        remaining: usize,
    },
}

struct FrameStackGuard<'a> {
    acc: &'a mut Accounter,
    frames: Vec<ReadFrame>,
}

impl Drop for FrameStackGuard<'_> {
    fn drop(&mut self) {
        for _ in 0..self.frames.len() {
            self.acc.pop_depth();
        }
    }
}

fn read_byte_array<R: Read>(r: &mut R, acc: &mut Accounter) -> Result<Vec<i8>, Error> {
    acc.charge(charges::BYTE_ARRAY_BASE)?;
    let len_i32 = read_i32(r)?;
    if len_i32 < 0 {
        return Err(FormatError::NegativeLength(len_i32).into());
    }
    let len = len_i32 as usize;
    acc.charge_many(len as u64, 1)?;
    let mut out = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMS));
    let mut remaining = len;
    let mut chunk = [0u8; 4096];
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        r.read_exact(&mut chunk[..n])?;
        out.extend(chunk[..n].iter().map(|&b| b as i8));
        remaining -= n;
    }
    Ok(out)
}

fn read_int_array<R: Read>(r: &mut R, acc: &mut Accounter) -> Result<Vec<i32>, Error> {
    acc.charge(charges::INT_ARRAY_BASE)?;
    let len_i32 = read_i32(r)?;
    if len_i32 < 0 {
        return Err(FormatError::NegativeLength(len_i32).into());
    }
    let len = len_i32 as usize;
    acc.charge_many(len as u64, 4)?;
    let mut out = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMS));
    for _ in 0..len {
        out.push(read_i32(r)?);
    }
    Ok(out)
}

fn read_long_array<R: Read>(r: &mut R, acc: &mut Accounter) -> Result<Vec<i64>, Error> {
    acc.charge(charges::LONG_ARRAY_BASE)?;
    let len_i32 = read_i32(r)?;
    if len_i32 < 0 {
        return Err(FormatError::NegativeLength(len_i32).into());
    }
    let len = len_i32 as usize;
    acc.charge_many(len as u64, 8)?;
    let mut out = Vec::with_capacity(len.min(MAX_PREALLOC_ELEMS));
    for _ in 0..len {
        out.push(read_i64(r)?);
    }
    Ok(out)
}

/// Begins decoding `tag_type`: returns `Ok(Some(tag))` for leaf tags, or pushes a container
/// frame onto `state.frames` and returns `Ok(None)` for `Compound`/`List`.
fn begin_read_tag<R: Read>(
    r: &mut R,
    tag_type: TagType,
    state: &mut FrameStackGuard<'_>,
) -> Result<Option<Tag>, Error> {
    match tag_type {
        TagType::End => {
            state.acc.charge(charges::END)?;
            Ok(Some(Tag::End))
        }
        TagType::Byte => {
            state.acc.charge(charges::BYTE)?;
            Ok(Some(Tag::Byte(read_i8(r)?)))
        }
        TagType::Short => {
            state.acc.charge(charges::SHORT)?;
            Ok(Some(Tag::Short(read_i16(r)?)))
        }
        TagType::Int => {
            state.acc.charge(charges::INT)?;
            Ok(Some(Tag::Int(read_i32(r)?)))
        }
        TagType::Long => {
            state.acc.charge(charges::LONG)?;
            Ok(Some(Tag::Long(read_i64(r)?)))
        }
        TagType::Float => {
            state.acc.charge(charges::FLOAT)?;
            Ok(Some(Tag::Float(f32::from_bits(read_u32(r)?))))
        }
        TagType::Double => {
            state.acc.charge(charges::DOUBLE)?;
            Ok(Some(Tag::Double(f64::from_bits(read_u64(r)?))))
        }
        TagType::ByteArray => read_byte_array(r, state.acc).map(|v| Some(Tag::ByteArray(v))),
        TagType::String => {
            state.acc.charge(charges::STRING_BASE)?;
            let s = read_mutf8_string(r)?;
            state
                .acc
                .charge_many(s.utf16_len() as u64, charges::UTF16_UNIT)?;
            Ok(Some(Tag::String(s)))
        }
        TagType::List => {
            state.acc.push_depth()?;
            let init_res = (|| -> Result<ReadFrame, Error> {
                state.acc.charge(charges::LIST_BASE)?;
                let elem_type = TagType::from_id(read_u8(r)?)?;
                let count_i32 = read_i32(r)?;
                if count_i32 < 0 {
                    return Err(FormatError::NegativeLength(count_i32).into());
                }
                if elem_type == TagType::End && count_i32 > 0 {
                    return Err(FormatError::NonEmptyEndList(count_i32).into());
                }
                let count = count_i32 as usize;
                state
                    .acc
                    .charge_many(count as u64, charges::LIST_PER_ELEM)?;
                Ok(ReadFrame::List {
                    list: List::with_capacity(count.min(MAX_PREALLOC_ELEMS)),
                    elem_type,
                    remaining: count,
                })
            })();
            match init_res {
                Ok(frame) => {
                    state.frames.push(frame);
                    Ok(None)
                }
                Err(e) => {
                    state.acc.pop_depth();
                    Err(e)
                }
            }
        }
        TagType::Compound => {
            state.acc.push_depth()?;
            if let Err(e) = state.acc.charge(charges::COMPOUND_BASE) {
                state.acc.pop_depth();
                return Err(e);
            }
            state.frames.push(ReadFrame::Compound {
                compound: Compound::new(),
                pending_key: None,
            });
            Ok(None)
        }
        TagType::IntArray => read_int_array(r, state.acc).map(|v| Some(Tag::IntArray(v))),
        TagType::LongArray => read_long_array(r, state.acc).map(|v| Some(Tag::LongArray(v))),
    }
}

fn read_payload<R: Read>(r: &mut R, tag_type: TagType, acc: &mut Accounter) -> Result<Tag, Error> {
    let mut state = FrameStackGuard {
        acc,
        frames: Vec::new(),
    };
    let mut completed = begin_read_tag(r, tag_type, &mut state)?;

    loop {
        if let Some(val) = completed.take() {
            match state.frames.last_mut() {
                None => return Ok(val),
                Some(ReadFrame::Compound {
                    compound,
                    pending_key,
                }) => {
                    let key = pending_key
                        .take()
                        .expect("compound frame must have a pending key when receiving a value");
                    if compound.insert(key, val).is_none() {
                        state.acc.charge(charges::COMPOUND_NEW_KEY_ENTRY)?;
                    }
                }
                Some(ReadFrame::List {
                    list,
                    elem_type,
                    remaining,
                }) => {
                    if *elem_type == TagType::Compound {
                        let Tag::Compound(c) = val else {
                            unreachable!("list of compounds always reads a compound");
                        };
                        match c.unwrap_list_wrapper() {
                            Ok(inner) => list.push(inner),
                            Err(c) => list.push(Tag::Compound(c)),
                        }
                    } else {
                        list.push(val);
                    }
                    *remaining -= 1;
                }
            }
        }

        enum NextAction {
            Pop(Tag),
            ReadNext(TagType),
        }

        let action = match state
            .frames
            .last_mut()
            .expect("stack non-empty while not completed")
        {
            ReadFrame::Compound { pending_key, .. } => {
                let entry_type = TagType::from_id(read_u8(r)?)?;
                if entry_type == TagType::End {
                    let Some(ReadFrame::Compound { compound, .. }) = state.frames.pop() else {
                        unreachable!()
                    };
                    state.acc.pop_depth();
                    NextAction::Pop(Tag::Compound(compound))
                } else {
                    let key = read_mutf8_string(r)?;
                    state.acc.charge(charges::COMPOUND_KEY_BASE)?;
                    state
                        .acc
                        .charge_many(key.utf16_len() as u64, charges::UTF16_UNIT)?;
                    *pending_key = Some(key);
                    NextAction::ReadNext(entry_type)
                }
            }
            ReadFrame::List {
                elem_type,
                remaining,
                ..
            } => {
                if *remaining == 0 {
                    let Some(ReadFrame::List { list, .. }) = state.frames.pop() else {
                        unreachable!()
                    };
                    state.acc.pop_depth();
                    NextAction::Pop(Tag::List(list))
                } else {
                    NextAction::ReadNext(*elem_type)
                }
            }
        };

        match action {
            NextAction::Pop(tag) => completed = Some(tag),
            NextAction::ReadNext(next_ty) => {
                completed = begin_read_tag(r, next_ty, &mut state)?;
            }
        }
    }
}

fn write_compound_payload<W: Write>(w: &mut W, compound: &Compound) -> Result<(), Error> {
    compound.check_order_supported()?;
    for (key, value) in compound.iter() {
        let ty = value.tag_type();
        if ty == TagType::End {
            return Err(FormatError::InvalidTagPlacement(TagType::End).into());
        }
        w.write_all(&[ty.id()])?;
        write_mutf8_string(w, key)?;
        write_payload(w, value)?;
    }
    w.write_all(&[TagType::End.id()])?;
    Ok(())
}

fn write_wrapped_list_element<W: Write>(w: &mut W, elem: &Tag) -> Result<(), Error> {
    let ty = elem.tag_type();
    if ty == TagType::End {
        return Err(FormatError::InvalidTagPlacement(TagType::End).into());
    }
    // Single-entry compound `{ "": elem }`: type ID, empty string (0x00 0x00), payload, End (0x00).
    w.write_all(&[ty.id(), 0, 0])?;
    write_payload(w, elem)?;
    w.write_all(&[TagType::End.id()])?;
    Ok(())
}

#[inline(never)]
fn write_byte_array<W: Write>(w: &mut W, arr: &[i8]) -> Result<(), Error> {
    let len = i32::try_from(arr.len()).map_err(|_| FormatError::NegativeLength(i32::MIN))?;
    w.write_all(&len.to_be_bytes())?;
    let mut chunk = [0u8; 4096];
    for slice in arr.chunks(chunk.len()) {
        for (dst, &src) in chunk.iter_mut().zip(slice.iter()) {
            *dst = src as u8;
        }
        w.write_all(&chunk[..slice.len()])?;
    }
    Ok(())
}

fn write_payload<W: Write>(w: &mut W, tag: &Tag) -> Result<(), Error> {
    match tag {
        Tag::End => Ok(()),
        Tag::Byte(v) => {
            w.write_all(&[*v as u8])?;
            Ok(())
        }
        Tag::Short(v) => {
            w.write_all(&v.to_be_bytes())?;
            Ok(())
        }
        Tag::Int(v) => {
            w.write_all(&v.to_be_bytes())?;
            Ok(())
        }
        Tag::Long(v) => {
            w.write_all(&v.to_be_bytes())?;
            Ok(())
        }
        Tag::Float(v) => {
            w.write_all(&v.to_bits().to_be_bytes())?;
            Ok(())
        }
        Tag::Double(v) => {
            w.write_all(&v.to_bits().to_be_bytes())?;
            Ok(())
        }
        Tag::ByteArray(arr) => write_byte_array(w, arr),
        Tag::String(s) => write_mutf8_string(w, s),
        Tag::List(list) => {
            let elem_type = list.wire_element_type()?;
            let len =
                i32::try_from(list.len()).map_err(|_| FormatError::NegativeLength(i32::MIN))?;
            w.write_all(&[elem_type.id()])?;
            w.write_all(&len.to_be_bytes())?;
            if elem_type == TagType::Compound {
                for elem in list.iter() {
                    match elem {
                        Tag::Compound(c) if !c.is_list_wrapper() => {
                            write_compound_payload(w, c)?;
                        }
                        _ => {
                            write_wrapped_list_element(w, elem)?;
                        }
                    }
                }
            } else {
                for elem in list.iter() {
                    write_payload(w, elem)?;
                }
            }
            Ok(())
        }
        Tag::Compound(compound) => write_compound_payload(w, compound),
        Tag::IntArray(arr) => {
            let len =
                i32::try_from(arr.len()).map_err(|_| FormatError::NegativeLength(i32::MIN))?;
            w.write_all(&len.to_be_bytes())?;
            for &v in arr {
                w.write_all(&v.to_be_bytes())?;
            }
            Ok(())
        }
        Tag::LongArray(arr) => {
            let len =
                i32::try_from(arr.len()).map_err(|_| FormatError::NegativeLength(i32::MIN))?;
            w.write_all(&len.to_be_bytes())?;
            for &v in arr {
                w.write_all(&v.to_be_bytes())?;
            }
            Ok(())
        }
    }
}

// --- Public API ---

/// Reads a **named-root** NBT value (the format used by `.dat` files and region chunks).
///
/// If the root tag type ID is `End` (`0`), no root name is read, [`Accounter`] is charged
/// 8 bytes, and `(NbtString::new(), Tag::End)` is returned.
pub fn read_named<R: Read>(
    reader: &mut R,
    accounter: &mut Accounter,
) -> Result<(NbtString, Tag), Error> {
    let tag_type = TagType::from_id(read_u8(reader)?)?;
    if tag_type == TagType::End {
        accounter.charge(charges::END)?;
        return Ok((NbtString::new(), Tag::End));
    }
    let name = read_mutf8_string(reader)?;
    let tag = read_payload(reader, tag_type, accounter)?;
    Ok((name, tag))
}

/// Reads a named-root NBT compound, returning [`FormatError::UnexpectedTagType`] if the root
/// is not a [`Tag::Compound`].
pub fn read_named_compound<R: Read>(
    reader: &mut R,
    accounter: &mut Accounter,
) -> Result<(NbtString, Compound), Error> {
    let (name, tag) = read_named(reader, accounter)?;
    Ok((name, tag.into_compound()?))
}

/// Writes a **named-root** NBT value (type ID, root name in modified UTF-8, payload).
///
/// If `tag` is [`Tag::End`], writes only the single `0` byte (no name).
pub fn write_named<W: Write>(
    writer: &mut W,
    name: impl AsNbtString,
    tag: &Tag,
) -> Result<(), Error> {
    let tag_type = tag.tag_type();
    writer.write_all(&[tag_type.id()])?;
    if tag_type == TagType::End {
        return Ok(());
    }
    write_mutf8_string(writer, &name)?;
    write_payload(writer, tag)
}

/// Reads an **unnamed-root** NBT value (the network protocol format).
///
/// An `End` (`0`) tag type ID charges 8 bytes and returns `Ok(None)` ("no value").
pub fn read_network<R: Read>(
    reader: &mut R,
    accounter: &mut Accounter,
) -> Result<Option<Tag>, Error> {
    let tag_type = TagType::from_id(read_u8(reader)?)?;
    if tag_type == TagType::End {
        accounter.charge(charges::END)?;
        return Ok(None);
    }
    Ok(Some(read_payload(reader, tag_type, accounter)?))
}

/// Writes an **unnamed-root** NBT value (the network protocol format).
///
/// `None` or `Some(Tag::End)` writes the single `0` (`End`) byte.
pub fn write_network<W: Write>(writer: &mut W, tag: Option<&Tag>) -> Result<(), Error> {
    match tag {
        None | Some(Tag::End) => {
            writer.write_all(&[TagType::End.id()])?;
            Ok(())
        }
        Some(tag) => {
            writer.write_all(&[tag.tag_type().id()])?;
            write_payload(writer, tag)
        }
    }
}

/// Reads a gzip-compressed named-root NBT stream (`.dat` files).
pub fn read_gzip_named<R: Read>(
    reader: R,
    accounter: &mut Accounter,
) -> Result<(NbtString, Tag), Error> {
    let mut decoder = GzDecoder::new(reader);
    read_named(&mut decoder, accounter)
}

/// Writes a gzip-compressed named-root NBT stream (`.dat` files).
pub fn write_gzip_named<W: Write>(
    writer: W,
    name: impl AsNbtString,
    tag: &Tag,
) -> Result<(), Error> {
    let mut encoder = GzEncoder::new(writer, Compression::default());
    write_named(&mut encoder, name, tag)?;
    encoder.finish()?;
    Ok(())
}

/// Reads a zlib-compressed named-root NBT stream (e.g. region chunk payloads).
pub fn read_zlib_named<R: Read>(
    reader: R,
    accounter: &mut Accounter,
) -> Result<(NbtString, Tag), Error> {
    let mut decoder = ZlibDecoder::new(reader);
    read_named(&mut decoder, accounter)
}

/// Writes a zlib-compressed named-root NBT stream.
pub fn write_zlib_named<W: Write>(
    writer: W,
    name: impl AsNbtString,
    tag: &Tag,
) -> Result<(), Error> {
    let mut encoder = ZlibEncoder::new(writer, Compression::default());
    write_named(&mut encoder, name, tag)?;
    encoder.finish()?;
    Ok(())
}

/// Convenience helper: encodes a named-root NBT value into a new `Vec<u8>`.
pub fn to_bytes_named(name: impl AsNbtString, tag: &Tag) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    write_named(&mut buf, name, tag)?;
    Ok(buf)
}

/// Convenience helper: decodes a named-root NBT value from a byte slice.
pub fn from_bytes_named(
    bytes: &[u8],
    accounter: &mut Accounter,
) -> Result<(NbtString, Tag), Error> {
    let mut slice = bytes;
    read_named(&mut slice, accounter)
}

/// Convenience helper: encodes an unnamed-root (network) NBT value into a new `Vec<u8>`.
pub fn to_bytes_network(tag: Option<&Tag>) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    write_network(&mut buf, tag)?;
    Ok(buf)
}

/// Convenience helper: decodes an unnamed-root (network) NBT value from a byte slice.
pub fn from_bytes_network(bytes: &[u8], accounter: &mut Accounter) -> Result<Option<Tag>, Error> {
    let mut slice = bytes;
    read_network(&mut slice, accounter)
}
