//! NBT I/O wrappers over `zenith-nbt`:
//! - Oversized-string-to-empty write fallback (> 65,535 modified UTF-8 bytes → `""`)
//! - Streaming NBT visitor with early termination and field skipping (used by blending scan & world summary)

use std::io::{BufWriter, Read, Write};

use zenith_nbt::{
    Accounter, Compound, Error as NbtError, FormatError, List, MAX_ENCODED_LEN, NbtString, Tag,
    TagType,
};

use crate::compression::{STREAM_BUFFER_SIZE, new_gzip_encoder};
use crate::error::Result;

// --- Oversized-string-to-empty write wrapper ---

/// Returns `true` if `tag` (or any nested key/string) has a modified UTF-8 encoding
/// exceeding [`MAX_ENCODED_LEN`] (65,535 bytes).
#[must_use]
pub fn tag_has_oversized_string(tag: &Tag) -> bool {
    let mut stack: Vec<&Tag> = vec![tag];
    while let Some(cur) = stack.pop() {
        match cur {
            Tag::String(s) => {
                if s.mutf8_len() > MAX_ENCODED_LEN {
                    return true;
                }
            }
            Tag::List(list) => {
                for elem in list.iter() {
                    stack.push(elem);
                }
            }
            Tag::Compound(c) => {
                for (k, v) in c.iter() {
                    if k.mutf8_len() > MAX_ENCODED_LEN {
                        return true;
                    }
                    stack.push(v);
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns `true` if `compound` (or any nested key/string) has a modified UTF-8 encoding
/// exceeding [`MAX_ENCODED_LEN`] (65,535 bytes).
#[must_use]
pub fn compound_has_oversized_string(compound: &Compound) -> bool {
    for (k, v) in compound.iter() {
        if k.mutf8_len() > MAX_ENCODED_LEN || tag_has_oversized_string(v) {
            return true;
        }
    }
    false
}

fn write_mutf8_fallback<W: Write + ?Sized>(w: &mut W, s: &NbtString) -> Result<()> {
    match s.encode_mutf8() {
        Ok(encoded) => {
            // One write for length + bytes, like Java's writeUTF (write boundaries matter for compression parity).
            let mut out = Vec::with_capacity(2 + encoded.len());
            out.extend_from_slice(&(encoded.len() as u16).to_be_bytes());
            out.extend_from_slice(&encoded);
            w.write_all(&out)?;
            Ok(())
        }
        Err(NbtError::StringTooLong { encoded_len }) => {
            log::error!(
                "NBT string modified-UTF-8 length {encoded_len} exceeds 65535 bytes; writing empty string instead"
            );
            w.write_all(&0u16.to_be_bytes())?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn write_compound_payload_fallback<W: Write + ?Sized>(
    w: &mut W,
    compound: &Compound,
) -> Result<()> {
    compound.check_order_supported()?;
    for (key, value) in compound.iter() {
        let ty = value.tag_type();
        if ty == TagType::End {
            return Err(NbtError::Format(FormatError::InvalidTagPlacement(TagType::End)).into());
        }
        // Fast path: delegate the entire entry `(type, key, payload)` directly to `zenith_nbt::write_named`
        // without cloning when neither the key nor any string inside `value` overflows 65,535 bytes.
        if key.mutf8_len() <= MAX_ENCODED_LEN && !tag_has_oversized_string(value) {
            let mut adapter = WriteAdapter(w);
            zenith_nbt::write_named(&mut adapter, key, value)?;
        } else {
            w.write_all(&[ty.id()])?;
            write_mutf8_fallback(w, key)?;
            write_payload_fallback(w, value)?;
        }
    }
    w.write_all(&[TagType::End.id()])?;
    Ok(())
}

fn write_wrapped_list_elem_fallback<W: Write + ?Sized>(w: &mut W, elem: &Tag) -> Result<()> {
    let ty = elem.tag_type();
    if ty == TagType::End {
        return Err(NbtError::Format(FormatError::InvalidTagPlacement(TagType::End)).into());
    }
    // Single-entry wrapper `{ "": elem }`
    w.write_all(&[ty.id(), 0, 0])?;
    write_payload_fallback(w, elem)?;
    w.write_all(&[TagType::End.id()])?;
    Ok(())
}

fn write_payload_fallback<W: Write + ?Sized>(w: &mut W, tag: &Tag) -> Result<()> {
    match tag {
        Tag::String(s) => write_mutf8_fallback(w, s),
        Tag::Compound(c) => write_compound_payload_fallback(w, c),
        Tag::List(list) => {
            let elem_type = list.wire_element_type()?;
            let len = i32::try_from(list.len())
                .map_err(|_| NbtError::Format(FormatError::NegativeLength(i32::MIN)))?;
            w.write_all(&[elem_type.id()])?;
            w.write_all(&len.to_be_bytes())?;
            if elem_type == TagType::Compound {
                for elem in list.iter() {
                    match elem {
                        Tag::Compound(c) if !c.is_list_wrapper() => {
                            write_compound_payload_fallback(w, c)?;
                        }
                        _ => {
                            write_wrapped_list_elem_fallback(w, elem)?;
                        }
                    }
                }
            } else {
                for elem in list.iter() {
                    write_payload_fallback(w, elem)?;
                }
            }
            Ok(())
        }
        // Any non-string leaf tag cannot overflow strings; encode via zenith-nbt directly.
        other => {
            let mut buf = Vec::new();
            zenith_nbt::write_network(&mut buf, Some(other))?;
            // Skip the leading 1-byte tag type emitted by write_network.
            w.write_all(&buf[1..])?;
            Ok(())
        }
    }
}

struct WriteAdapter<'a, W: Write + ?Sized>(&'a mut W);

impl<W: Write + ?Sized> Write for WriteAdapter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Writes a named-root NBT compound (with empty root name `""`) to `writer`, replacing any
/// string whose modified-UTF-8 encoding exceeds 65,535 bytes with `""` instead of failing.
///
/// Preserves exact [`Compound`] iteration order without cloning.
pub fn write_named_compound_fallback<W: Write + ?Sized>(
    writer: &mut W,
    compound: &Compound,
) -> Result<()> {
    writer.write_all(&[TagType::Compound.id(), 0, 0])?;
    write_compound_payload_fallback(writer, compound)
}

/// Writes a gzip-compressed named-root NBT compound (empty root name `""`) using 8,192-byte
/// buffering around an RFC 1952 gzip encoder (`mtime = 0`, OS `0xFF`, default level 6) and
/// the oversized-string-to-empty fallback.
pub fn write_gzip_named_compound_fallback<W: Write>(writer: W, compound: &Compound) -> Result<()> {
    let outer = BufWriter::with_capacity(STREAM_BUFFER_SIZE, writer);
    let gz = new_gzip_encoder(outer);
    let mut inner = BufWriter::with_capacity(STREAM_BUFFER_SIZE, gz);
    write_named_compound_fallback(&mut inner, compound)?;
    let gz = inner.into_inner().map_err(|e| e.into_error())?;
    let mut outer = gz.finish()?;
    outer.flush()?;
    Ok(())
}

/// Writes a gzip-compressed named-root wrapper `{ <wrapper_key>: <inner_compound> }`
/// without cloning `inner_compound` (preserving its exact iteration order).
pub fn write_gzip_wrapped_compound_fallback<W: Write>(
    writer: W,
    wrapper_key: &str,
    inner_compound: &Compound,
) -> Result<()> {
    let outer = BufWriter::with_capacity(STREAM_BUFFER_SIZE, writer);
    let gz = new_gzip_encoder(outer);
    let mut inner = BufWriter::with_capacity(STREAM_BUFFER_SIZE, gz);
    // Root named Compound (""):
    inner.write_all(&[TagType::Compound.id(), 0, 0])?;
    // Entry: TagType::Compound, key = wrapper_key
    inner.write_all(&[TagType::Compound.id()])?;
    write_mutf8_fallback(&mut inner, &NbtString::from(wrapper_key))?;
    write_compound_payload_fallback(&mut inner, inner_compound)?;
    // End of root compound
    inner.write_all(&[TagType::End.id()])?;
    let gz = inner.into_inner().map_err(|e| e.into_error())?;
    let mut outer = gz.finish()?;
    outer.flush()?;
    Ok(())
}

/// Reads a named-root NBT compound using [`Accounter::unlimited`] (unlimited heap quota,
/// 512 nesting-depth limit).
pub fn read_named_compound_unlimited<R: Read>(reader: &mut R) -> Result<Compound> {
    let mut acc = Accounter::unlimited();
    let (_name, compound) = zenith_nbt::read_named_compound(reader, &mut acc)?;
    Ok(compound)
}

/// Reads a named-root NBT compound using [`Accounter::uncompressed`] (100 MiB quota,
/// 512 nesting-depth limit).
pub fn read_named_compound_level<R: Read>(reader: &mut R) -> Result<Compound> {
    let mut acc = Accounter::uncompressed();
    let (_name, compound) = zenith_nbt::read_named_compound(reader, &mut acc)?;
    Ok(compound)
}

// --- Streaming NBT visitor ---

/// Flow-control result returned by value callbacks in [`StreamTagVisitor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueResult {
    /// Continue scanning the current container.
    Continue,
    /// Stop scanning the current container (skipping remaining entries/elements).
    Break,
    /// Stop scanning the entire stream immediately.
    Halt,
}

/// Flow-control result returned by entry-decision callbacks in [`StreamTagVisitor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryResult {
    /// Enter and visit this entry's value.
    Enter,
    /// Skip this entry's value without descending into it.
    Skip,
    /// Stop scanning the current compound (skipping this entry and remaining entries).
    Break,
    /// Stop scanning the entire stream immediately.
    Halt,
}

/// Streaming NBT visitor supporting early termination (`Halt`/`Break`) and zero-DOM field skipping.
pub trait StreamTagVisitor {
    /// Called for the root tag type before visiting its payload.
    fn visit_root_entry(&mut self, _tag_type: TagType) -> ValueResult {
        ValueResult::Continue
    }

    /// Called when an entry `(tag_type, name)` is encountered inside a compound.
    fn visit_entry(&mut self, _tag_type: TagType, _name: &NbtString) -> EntryResult {
        EntryResult::Enter
    }

    /// Called for a `Tag::Byte`.
    fn visit_byte(&mut self, _value: i8) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::Short`.
    fn visit_short(&mut self, _value: i16) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::Int`.
    fn visit_int(&mut self, _value: i32) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::Long`.
    fn visit_long(&mut self, _value: i64) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::Float`.
    fn visit_float(&mut self, _value: f32) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::Double`.
    fn visit_double(&mut self, _value: f64) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::String`.
    fn visit_string(&mut self, _value: &NbtString) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::ByteArray`.
    fn visit_byte_array(&mut self, _value: &[i8]) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::IntArray`.
    fn visit_int_array(&mut self, _value: &[i32]) -> ValueResult {
        ValueResult::Continue
    }

    /// Called for a `Tag::LongArray`.
    fn visit_long_array(&mut self, _value: &[i64]) -> ValueResult {
        ValueResult::Continue
    }

    /// Called when entering a `Tag::List`.
    fn visit_list_start(&mut self, _elem_type: TagType, _len: usize) -> ValueResult {
        ValueResult::Continue
    }

    /// Called before each element of a `Tag::List`.
    fn visit_list_element(&mut self, _elem_type: TagType, _index: usize) -> EntryResult {
        EntryResult::Enter
    }

    /// Called when finishing a `Tag::Compound` or `Tag::List`.
    fn visit_container_end(&mut self) -> ValueResult {
        ValueResult::Continue
    }
}

fn read_u8<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u16<R: Read>(r: &mut R) -> Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

fn read_i16<R: Read>(r: &mut R) -> Result<i16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(i16::from_be_bytes(b))
}

fn read_i32<R: Read>(r: &mut R) -> Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_be_bytes(b))
}

fn read_i64<R: Read>(r: &mut R) -> Result<i64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(i64::from_be_bytes(b))
}

fn skip_exact<R: Read>(r: &mut R, mut count: u64) -> Result<()> {
    let mut sink = [0u8; 4096];
    while count > 0 {
        let step = (count as usize).min(sink.len());
        r.read_exact(&mut sink[..step])?;
        count -= step as u64;
    }
    Ok(())
}

fn read_mutf8<R: Read>(r: &mut R) -> Result<NbtString> {
    let len = usize::from(read_u16(r)?);
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(zenith_nbt::mutf8::decode_mutf8(&buf)?)
}

fn skip_mutf8<R: Read>(r: &mut R) -> Result<()> {
    let len = u64::from(read_u16(r)?);
    skip_exact(r, len)
}

/// Skips a tag payload on the wire without allocating a DOM, still enforcing nesting depth.
fn skip_payload<R: Read>(r: &mut R, tag_type: TagType, acc: &mut Accounter) -> Result<()> {
    match tag_type {
        TagType::End => Ok(()),
        TagType::Byte => skip_exact(r, 1),
        TagType::Short => skip_exact(r, 2),
        TagType::Int | TagType::Float => skip_exact(r, 4),
        TagType::Long | TagType::Double => skip_exact(r, 8),
        TagType::String => skip_mutf8(r),
        TagType::ByteArray => {
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            skip_exact(r, len as u64)
        }
        TagType::IntArray => {
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            skip_exact(r, (len as u64) * 4)
        }
        TagType::LongArray => {
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            skip_exact(r, (len as u64) * 8)
        }
        TagType::List => {
            acc.push_depth()?;
            let res = (|| -> Result<()> {
                let elem_type = TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                let len = read_i32(r)?;
                if len < 0 {
                    return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
                }
                if elem_type == TagType::End && len > 0 {
                    return Err(NbtError::Format(FormatError::NonEmptyEndList(len)).into());
                }
                for _ in 0..len {
                    skip_payload(r, elem_type, acc)?;
                }
                Ok(())
            })();
            acc.pop_depth();
            res
        }
        TagType::Compound => {
            acc.push_depth()?;
            let res = (|| -> Result<()> {
                loop {
                    let ty = TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                    if ty == TagType::End {
                        break;
                    }
                    skip_mutf8(r)?;
                    skip_payload(r, ty, acc)?;
                }
                Ok(())
            })();
            acc.pop_depth();
            res
        }
    }
}

fn scan_payload<R: Read, V: StreamTagVisitor>(
    r: &mut R,
    tag_type: TagType,
    acc: &mut Accounter,
    visitor: &mut V,
) -> Result<ValueResult> {
    match tag_type {
        TagType::End => Ok(ValueResult::Continue),
        TagType::Byte => {
            acc.charge(9)?;
            Ok(visitor.visit_byte(read_u8(r)? as i8))
        }
        TagType::Short => {
            acc.charge(10)?;
            Ok(visitor.visit_short(read_i16(r)?))
        }
        TagType::Int => {
            acc.charge(12)?;
            Ok(visitor.visit_int(read_i32(r)?))
        }
        TagType::Long => {
            acc.charge(16)?;
            Ok(visitor.visit_long(read_i64(r)?))
        }
        TagType::Float => {
            acc.charge(12)?;
            Ok(visitor.visit_float(f32::from_bits(read_i32(r)? as u32)))
        }
        TagType::Double => {
            acc.charge(16)?;
            Ok(visitor.visit_double(f64::from_bits(read_i64(r)? as u64)))
        }
        TagType::String => {
            acc.charge(36)?;
            let s = read_mutf8(r)?;
            acc.charge_many(s.utf16_len() as u64, 2)?;
            Ok(visitor.visit_string(&s))
        }
        TagType::ByteArray => {
            acc.charge(24)?;
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            let count = len as usize;
            acc.charge_many(count as u64, 1)?;
            let mut raw = vec![0u8; count];
            r.read_exact(&mut raw)?;
            let signed: Vec<i8> = raw.into_iter().map(|b| b as i8).collect();
            Ok(visitor.visit_byte_array(&signed))
        }
        TagType::IntArray => {
            acc.charge(24)?;
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            let count = len as usize;
            acc.charge_many(count as u64, 4)?;
            let mut out = Vec::with_capacity(count.min(65_536));
            for _ in 0..count {
                out.push(read_i32(r)?);
            }
            Ok(visitor.visit_int_array(&out))
        }
        TagType::LongArray => {
            acc.charge(24)?;
            let len = read_i32(r)?;
            if len < 0 {
                return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
            }
            let count = len as usize;
            acc.charge_many(count as u64, 8)?;
            let mut out = Vec::with_capacity(count.min(65_536));
            for _ in 0..count {
                out.push(read_i64(r)?);
            }
            Ok(visitor.visit_long_array(&out))
        }
        TagType::List => {
            acc.push_depth()?;
            let res = (|| -> Result<ValueResult> {
                acc.charge(36)?;
                let elem_type = TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                let len = read_i32(r)?;
                if len < 0 {
                    return Err(NbtError::Format(FormatError::NegativeLength(len)).into());
                }
                if elem_type == TagType::End && len > 0 {
                    return Err(NbtError::Format(FormatError::NonEmptyEndList(len)).into());
                }
                let count = len as usize;
                acc.charge_many(count as u64, 4)?;
                match visitor.visit_list_start(elem_type, count) {
                    ValueResult::Halt => return Ok(ValueResult::Halt),
                    ValueResult::Break => {
                        for _ in 0..count {
                            skip_payload(r, elem_type, acc)?;
                        }
                        return Ok(visitor.visit_container_end());
                    }
                    ValueResult::Continue => {}
                }
                let mut i = 0;
                while i < count {
                    match visitor.visit_list_element(elem_type, i) {
                        EntryResult::Halt => return Ok(ValueResult::Halt),
                        EntryResult::Break => {
                            while i < count {
                                skip_payload(r, elem_type, acc)?;
                                i += 1;
                            }
                            break;
                        }
                        EntryResult::Skip => {
                            skip_payload(r, elem_type, acc)?;
                        }
                        EntryResult::Enter => match scan_payload(r, elem_type, acc, visitor)? {
                            ValueResult::Halt => return Ok(ValueResult::Halt),
                            ValueResult::Break => {
                                i += 1;
                                while i < count {
                                    skip_payload(r, elem_type, acc)?;
                                    i += 1;
                                }
                                break;
                            }
                            ValueResult::Continue => {}
                        },
                    }
                    i += 1;
                }
                Ok(visitor.visit_container_end())
            })();
            acc.pop_depth();
            res
        }
        TagType::Compound => {
            acc.push_depth()?;
            let res = (|| -> Result<ValueResult> {
                acc.charge(48)?;
                loop {
                    let ty = TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                    if ty == TagType::End {
                        break;
                    }
                    let key = read_mutf8(r)?;
                    acc.charge(28)?;
                    acc.charge_many(key.utf16_len() as u64, 2)?;
                    match visitor.visit_entry(ty, &key) {
                        EntryResult::Halt => return Ok(ValueResult::Halt),
                        EntryResult::Break => {
                            skip_payload(r, ty, acc)?;
                            // Drain remaining entries of this compound.
                            loop {
                                let rem_ty =
                                    TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                                if rem_ty == TagType::End {
                                    break;
                                }
                                skip_mutf8(r)?;
                                skip_payload(r, rem_ty, acc)?;
                            }
                            break;
                        }
                        EntryResult::Skip => {
                            skip_payload(r, ty, acc)?;
                        }
                        EntryResult::Enter => match scan_payload(r, ty, acc, visitor)? {
                            ValueResult::Halt => return Ok(ValueResult::Halt),
                            ValueResult::Break => {
                                loop {
                                    let rem_ty =
                                        TagType::from_id(read_u8(r)?).map_err(NbtError::Format)?;
                                    if rem_ty == TagType::End {
                                        break;
                                    }
                                    skip_mutf8(r)?;
                                    skip_payload(r, rem_ty, acc)?;
                                }
                                break;
                            }
                            ValueResult::Continue => {}
                        },
                    }
                }
                Ok(visitor.visit_container_end())
            })();
            acc.pop_depth();
            res
        }
    }
}

/// Streams a named-root NBT value into `visitor` without building a full DOM, skipping the
/// root name without accounting (per spec) and stopping early on [`ValueResult::Halt`].
pub fn scan_named_stream<R: Read, V: StreamTagVisitor>(
    reader: &mut R,
    accounter: &mut Accounter,
    visitor: &mut V,
) -> Result<()> {
    let root_type = TagType::from_id(read_u8(reader)?).map_err(NbtError::Format)?;
    if root_type == TagType::End {
        return Ok(());
    }
    // Root name is skipped without accounting.
    skip_mutf8(reader)?;
    match visitor.visit_root_entry(root_type) {
        ValueResult::Continue => {
            let _ = scan_payload(reader, root_type, accounter, visitor)?;
        }
        ValueResult::Break | ValueResult::Halt => {}
    }
    Ok(())
}

/// Drives `visitor` over an already-in-memory [`Compound`] (used by the IO worker when a
/// streaming scan hits a chunk in the pending-write map).
pub fn scan_compound_in_memory<V: StreamTagVisitor>(compound: &Compound, visitor: &mut V) {
    if visitor.visit_root_entry(TagType::Compound) != ValueResult::Continue {
        return;
    }
    let _ = visit_compound_in_memory(compound, visitor);
}

fn visit_tag_in_memory<V: StreamTagVisitor>(tag: &Tag, visitor: &mut V) -> ValueResult {
    match tag {
        Tag::End => ValueResult::Continue,
        Tag::Byte(v) => visitor.visit_byte(*v),
        Tag::Short(v) => visitor.visit_short(*v),
        Tag::Int(v) => visitor.visit_int(*v),
        Tag::Long(v) => visitor.visit_long(*v),
        Tag::Float(v) => visitor.visit_float(*v),
        Tag::Double(v) => visitor.visit_double(*v),
        Tag::String(s) => visitor.visit_string(s),
        Tag::ByteArray(a) => visitor.visit_byte_array(a),
        Tag::IntArray(a) => visitor.visit_int_array(a),
        Tag::LongArray(a) => visitor.visit_long_array(a),
        Tag::List(list) => visit_list_in_memory(list, visitor),
        Tag::Compound(c) => visit_compound_in_memory(c, visitor),
    }
}

fn visit_list_in_memory<V: StreamTagVisitor>(list: &List, visitor: &mut V) -> ValueResult {
    let elem_type = list.wire_element_type().unwrap_or(TagType::End);
    match visitor.visit_list_start(elem_type, list.len()) {
        ValueResult::Halt => return ValueResult::Halt,
        ValueResult::Break => return visitor.visit_container_end(),
        ValueResult::Continue => {}
    }
    for (idx, elem) in list.iter().enumerate() {
        match visitor.visit_list_element(elem.tag_type(), idx) {
            EntryResult::Halt => return ValueResult::Halt,
            EntryResult::Break => break,
            EntryResult::Skip => {}
            EntryResult::Enter => match visit_tag_in_memory(elem, visitor) {
                ValueResult::Halt => return ValueResult::Halt,
                ValueResult::Break => break,
                ValueResult::Continue => {}
            },
        }
    }
    visitor.visit_container_end()
}

fn visit_compound_in_memory<V: StreamTagVisitor>(
    compound: &Compound,
    visitor: &mut V,
) -> ValueResult {
    for (k, v) in compound.iter() {
        match visitor.visit_entry(v.tag_type(), k) {
            EntryResult::Halt => return ValueResult::Halt,
            EntryResult::Break => break,
            EntryResult::Skip => {}
            EntryResult::Enter => match visit_tag_in_memory(v, visitor) {
                ValueResult::Halt => return ValueResult::Halt,
                ValueResult::Break => break,
                ValueResult::Continue => {}
            },
        }
    }
    visitor.visit_container_end()
}

/// Streaming visitor that collects a [`Compound`] while skipping selected nested compound paths
/// (used by world-summary scanning to skip `Data.Player` and `Data.WorldGenSettings`).
#[derive(Debug)]
pub struct SkippingCompoundCollector {
    skip_paths: Vec<Vec<String>>,
    stack: Vec<CollectorFrame>,
    current_path: Vec<String>,
    root: Option<Tag>,
}

#[derive(Debug)]
enum CollectorFrame {
    Compound {
        compound: Compound,
        pending_key: Option<NbtString>,
    },
    List {
        list: List,
        elem_type: TagType,
    },
}

impl SkippingCompoundCollector {
    /// Creates a collector skipping the specified nested paths (e.g. `&[&["Data", "Player"]]`).
    #[must_use]
    pub fn new(skip_paths: &[&[&str]]) -> Self {
        Self {
            skip_paths: skip_paths
                .iter()
                .map(|p| p.iter().map(|s| (*s).to_owned()).collect())
                .collect(),
            stack: Vec::new(),
            current_path: Vec::new(),
            root: None,
        }
    }

    /// Consumes the collector and returns the collected root [`Tag`], if any.
    #[must_use]
    pub fn into_root(self) -> Option<Tag> {
        self.root
    }

    fn should_skip(&self, name: &NbtString) -> bool {
        let Some(name_str) = name.as_str() else {
            return false;
        };
        let depth = self.current_path.len();
        self.skip_paths.iter().any(|p| {
            p.len() == depth + 1
                && p[..depth]
                    .iter()
                    .zip(self.current_path.iter())
                    .all(|(a, b)| a == b)
                && p[depth] == name_str
        })
    }

    fn push_value(&mut self, tag: Tag) {
        match self.stack.last_mut() {
            None => self.root = Some(tag),
            Some(CollectorFrame::Compound {
                compound,
                pending_key,
            }) => {
                if let Some(key) = pending_key.take() {
                    compound.insert(key, tag);
                }
                self.current_path.pop();
            }
            Some(CollectorFrame::List { list, elem_type }) => {
                if *elem_type == TagType::Compound {
                    if let Tag::Compound(c) = tag {
                        match c.unwrap_list_wrapper() {
                            Ok(inner) => list.push(inner),
                            Err(c) => list.push(Tag::Compound(c)),
                        }
                    } else {
                        list.push(tag);
                    }
                } else {
                    list.push(tag);
                }
            }
        }
    }
}

impl StreamTagVisitor for SkippingCompoundCollector {
    fn visit_root_entry(&mut self, tag_type: TagType) -> ValueResult {
        if tag_type == TagType::Compound {
            self.stack.push(CollectorFrame::Compound {
                compound: Compound::new(),
                pending_key: None,
            });
            ValueResult::Continue
        } else {
            // Non-compound root: let caller inspect root = None or leaf.
            ValueResult::Continue
        }
    }

    fn visit_entry(&mut self, tag_type: TagType, name: &NbtString) -> EntryResult {
        if tag_type == TagType::Compound && self.should_skip(name) {
            return EntryResult::Skip;
        }
        if let Some(CollectorFrame::Compound { pending_key, .. }) = self.stack.last_mut() {
            *pending_key = Some(name.clone());
            self.current_path.push(name.to_string_lossy().into_owned());
        }
        if tag_type == TagType::Compound {
            self.stack.push(CollectorFrame::Compound {
                compound: Compound::new(),
                pending_key: None,
            });
        }
        EntryResult::Enter
    }

    fn visit_byte(&mut self, value: i8) -> ValueResult {
        self.push_value(Tag::Byte(value));
        ValueResult::Continue
    }

    fn visit_short(&mut self, value: i16) -> ValueResult {
        self.push_value(Tag::Short(value));
        ValueResult::Continue
    }

    fn visit_int(&mut self, value: i32) -> ValueResult {
        self.push_value(Tag::Int(value));
        ValueResult::Continue
    }

    fn visit_long(&mut self, value: i64) -> ValueResult {
        self.push_value(Tag::Long(value));
        ValueResult::Continue
    }

    fn visit_float(&mut self, value: f32) -> ValueResult {
        self.push_value(Tag::Float(value));
        ValueResult::Continue
    }

    fn visit_double(&mut self, value: f64) -> ValueResult {
        self.push_value(Tag::Double(value));
        ValueResult::Continue
    }

    fn visit_string(&mut self, value: &NbtString) -> ValueResult {
        self.push_value(Tag::String(value.clone()));
        ValueResult::Continue
    }

    fn visit_byte_array(&mut self, value: &[i8]) -> ValueResult {
        self.push_value(Tag::ByteArray(value.to_vec()));
        ValueResult::Continue
    }

    fn visit_int_array(&mut self, value: &[i32]) -> ValueResult {
        self.push_value(Tag::IntArray(value.to_vec()));
        ValueResult::Continue
    }

    fn visit_long_array(&mut self, value: &[i64]) -> ValueResult {
        self.push_value(Tag::LongArray(value.to_vec()));
        ValueResult::Continue
    }

    fn visit_list_start(&mut self, elem_type: TagType, len: usize) -> ValueResult {
        self.stack.push(CollectorFrame::List {
            list: List::with_capacity(len.min(65_536)),
            elem_type,
        });
        ValueResult::Continue
    }

    fn visit_list_element(&mut self, elem_type: TagType, _index: usize) -> EntryResult {
        if elem_type == TagType::Compound {
            self.stack.push(CollectorFrame::Compound {
                compound: Compound::new(),
                pending_key: None,
            });
        }
        EntryResult::Enter
    }

    fn visit_container_end(&mut self) -> ValueResult {
        if let Some(frame) = self.stack.pop() {
            match frame {
                CollectorFrame::Compound { compound, .. } => {
                    self.push_value(Tag::Compound(compound));
                }
                CollectorFrame::List { list, .. } => {
                    self.push_value(Tag::List(list));
                }
            }
        }
        ValueResult::Continue
    }
}
