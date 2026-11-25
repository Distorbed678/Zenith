//! Optional `serde` integration for converting typed Rust values to and from [`Tag`].

use std::fmt;

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};

use crate::compound::Compound;
use crate::error::Error;
use crate::list::List;
use crate::mutf8::NbtString;
use crate::tag::Tag;

/// Serializes any [`Serialize`] value into a [`Tag`].
pub fn to_tag<T: Serialize + ?Sized>(value: &T) -> Result<Tag, Error> {
    value.serialize(TagSerializer)
}

/// Deserializes a [`Tag`] into any [`DeserializeOwned`] type.
pub fn from_tag<T: DeserializeOwned>(tag: Tag) -> Result<T, Error> {
    T::deserialize(TagDeserializer(tag))
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Serde(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Serde(msg.to_string())
    }
}

// --- Serializer ---

struct TagSerializer;

impl ser::Serializer for TagSerializer {
    type Ok = Tag;
    type Error = Error;

    type SerializeSeq = ListSerializer;
    type SerializeTuple = ListSerializer;
    type SerializeTupleStruct = ListSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = CompoundSerializer;
    type SerializeStruct = CompoundSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<Tag, Error> {
        Ok(Tag::Byte(i8::from(v)))
    }

    fn serialize_i8(self, v: i8) -> Result<Tag, Error> {
        Ok(Tag::Byte(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Tag, Error> {
        Ok(Tag::Short(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Tag, Error> {
        Ok(Tag::Int(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Tag, Error> {
        Ok(Tag::Long(v))
    }

    fn serialize_u8(self, v: u8) -> Result<Tag, Error> {
        Ok(Tag::Byte(v as i8))
    }

    fn serialize_u16(self, v: u16) -> Result<Tag, Error> {
        Ok(Tag::Short(v as i16))
    }

    fn serialize_u32(self, v: u32) -> Result<Tag, Error> {
        Ok(Tag::Int(v as i32))
    }

    fn serialize_u64(self, v: u64) -> Result<Tag, Error> {
        Ok(Tag::Long(v as i64))
    }

    fn serialize_f32(self, v: f32) -> Result<Tag, Error> {
        Ok(Tag::Float(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Tag, Error> {
        Ok(Tag::Double(v))
    }

    fn serialize_char(self, v: char) -> Result<Tag, Error> {
        Ok(Tag::String(NbtString::from(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Tag, Error> {
        Ok(Tag::String(NbtString::from(v)))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Tag, Error> {
        Ok(Tag::ByteArray(v.iter().map(|&b| b as i8).collect()))
    }

    fn serialize_none(self) -> Result<Tag, Error> {
        Ok(Tag::End)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Tag, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Tag, Error> {
        Ok(Tag::End)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Tag, Error> {
        Ok(Tag::Compound(Compound::new()))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Tag, Error> {
        Ok(Tag::String(NbtString::from(variant)))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Tag, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Tag, Error> {
        let mut c = Compound::new();
        c.insert(variant, value.serialize(Self)?);
        Ok(Tag::Compound(c))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(ListSerializer {
            list: List::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Ok(TupleVariantSerializer {
            variant,
            list: List::with_capacity(len),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Ok(CompoundSerializer {
            compound: Compound::new(),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Ok(CompoundSerializer {
            compound: Compound::new(),
            next_key: None,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Ok(StructVariantSerializer {
            variant,
            compound: Compound::new(),
        })
    }
}

struct ListSerializer {
    list: List,
}

impl SerializeSeq for ListSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.list.push(value.serialize(TagSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Tag, Error> {
        Ok(Tag::List(self.list))
    }
}

impl SerializeTuple for ListSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Tag, Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for ListSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Tag, Error> {
        SerializeSeq::end(self)
    }
}

struct TupleVariantSerializer {
    variant: &'static str,
    list: List,
}

impl SerializeTupleVariant for TupleVariantSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.list.push(value.serialize(TagSerializer)?);
        Ok(())
    }

    fn end(self) -> Result<Tag, Error> {
        let mut c = Compound::new();
        c.insert(self.variant, Tag::List(self.list));
        Ok(Tag::Compound(c))
    }
}

struct CompoundSerializer {
    compound: Compound,
    next_key: Option<NbtString>,
}

impl SerializeMap for CompoundSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        let key_tag = key.serialize(TagSerializer)?;
        match key_tag {
            Tag::String(s) => {
                self.next_key = Some(s);
                Ok(())
            }
            other => Err(Error::Serde(format!(
                "compound keys must be strings, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| Error::Serde("serialize_value called before serialize_key".into()))?;
        let val = value.serialize(TagSerializer)?;
        if !matches!(val, Tag::End) {
            self.compound.insert(key, val);
        }
        Ok(())
    }

    fn end(self) -> Result<Tag, Error> {
        Ok(Tag::Compound(self.compound))
    }
}

impl SerializeStruct for CompoundSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let val = value.serialize(TagSerializer)?;
        if !matches!(val, Tag::End) {
            self.compound.insert(key, val);
        }
        Ok(())
    }

    fn end(self) -> Result<Tag, Error> {
        Ok(Tag::Compound(self.compound))
    }
}

struct StructVariantSerializer {
    variant: &'static str,
    compound: Compound,
}

impl SerializeStructVariant for StructVariantSerializer {
    type Ok = Tag;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let val = value.serialize(TagSerializer)?;
        if !matches!(val, Tag::End) {
            self.compound.insert(key, val);
        }
        Ok(())
    }

    fn end(self) -> Result<Tag, Error> {
        let mut outer = Compound::new();
        outer.insert(self.variant, Tag::Compound(self.compound));
        Ok(Tag::Compound(outer))
    }
}

// --- Deserializer ---

struct TagDeserializer(Tag);

impl<'de> de::Deserializer<'de> for TagDeserializer {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::End => visitor.visit_unit(),
            Tag::Byte(v) => visitor.visit_i8(v),
            Tag::Short(v) => visitor.visit_i16(v),
            Tag::Int(v) => visitor.visit_i32(v),
            Tag::Long(v) => visitor.visit_i64(v),
            Tag::Float(v) => visitor.visit_f32(v),
            Tag::Double(v) => visitor.visit_f64(v),
            Tag::ByteArray(arr) => {
                let bytes: Vec<u8> = arr.into_iter().map(|b| b as u8).collect();
                visitor.visit_byte_buf(bytes)
            }
            Tag::String(s) => visitor.visit_string(s.to_string_lossy().into_owned()),
            Tag::List(list) => visitor.visit_seq(TagSeqAccess(list.into_iter())),
            Tag::Compound(compound) => visitor.visit_map(CompoundMapAccess {
                iter: compound.into_iter(),
                pending_value: None,
            }),
            Tag::IntArray(arr) => {
                let tags = arr.into_iter().map(Tag::Int).collect::<Vec<_>>();
                visitor.visit_seq(TagSeqAccess(tags.into_iter()))
            }
            Tag::LongArray(arr) => {
                let tags = arr.into_iter().map(Tag::Long).collect::<Vec<_>>();
                visitor.visit_seq(TagSeqAccess(tags.into_iter()))
            }
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::Byte(v) => visitor.visit_bool(v != 0),
            other => Err(Error::Serde(format!(
                "expected Byte for bool, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::Byte(v) => visitor.visit_u8(v as u8),
            Tag::Short(v) => u8::try_from(v)
                .map_err(|_| Error::Serde(format!("short {v} out of u8 range")))
                .and_then(|n| visitor.visit_u8(n)),
            Tag::Int(v) => u8::try_from(v)
                .map_err(|_| Error::Serde(format!("int {v} out of u8 range")))
                .and_then(|n| visitor.visit_u8(n)),
            other => Err(Error::Serde(format!(
                "expected integer for u8, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::Short(v) => visitor.visit_u16(v as u16),
            Tag::Int(v) => u16::try_from(v)
                .map_err(|_| Error::Serde(format!("int {v} out of u16 range")))
                .and_then(|n| visitor.visit_u16(n)),
            other => Err(Error::Serde(format!(
                "expected integer for u16, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::Int(v) => visitor.visit_u32(v as u32),
            Tag::Long(v) => u32::try_from(v)
                .map_err(|_| Error::Serde(format!("long {v} out of u32 range")))
                .and_then(|n| visitor.visit_u32(n)),
            other => Err(Error::Serde(format!(
                "expected integer for u32, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::Long(v) => visitor.visit_u64(v as u64),
            other => Err(Error::Serde(format!(
                "expected Long for u64, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::End => visitor.visit_none(),
            other => visitor.visit_some(Self(other)),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Tag::List(list) => visitor.visit_seq(TagSeqAccess(list.into_iter())),
            Tag::ByteArray(arr) => {
                let tags: Vec<Tag> = arr.into_iter().map(Tag::Byte).collect();
                visitor.visit_seq(TagSeqAccess(tags.into_iter()))
            }
            Tag::IntArray(arr) => {
                let tags: Vec<Tag> = arr.into_iter().map(Tag::Int).collect();
                visitor.visit_seq(TagSeqAccess(tags.into_iter()))
            }
            Tag::LongArray(arr) => {
                let tags: Vec<Tag> = arr.into_iter().map(Tag::Long).collect();
                visitor.visit_seq(TagSeqAccess(tags.into_iter()))
            }
            other => Err(Error::Serde(format!(
                "expected list or typed array, got {:?}",
                other.tag_type()
            ))),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Tag::String(s) => visitor.visit_enum(de::value::StringDeserializer::new(
                s.to_string_lossy().into_owned(),
            )),
            Tag::Compound(c) => {
                if c.len() != 1 {
                    return Err(Error::Serde(format!(
                        "expected single-entry compound for enum, got {} entries",
                        c.len()
                    )));
                }
                let (k, v) = c.into_iter().next().expect("len == 1");
                visitor.visit_enum(EnumAccess {
                    variant: k.to_string_lossy().into_owned(),
                    value: v,
                })
            }
            other => Err(Error::Serde(format!(
                "expected String or Compound for enum, got {:?}",
                other.tag_type()
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i32 i64 f32 f64 char str string bytes byte_buf unit unit_struct
        tuple tuple_struct map struct identifier ignored_any
    }
}

struct TagSeqAccess(std::vec::IntoIter<Tag>);

impl<'de> SeqAccess<'de> for TagSeqAccess {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.0.next() {
            Some(tag) => seed.deserialize(TagDeserializer(tag)).map(Some),
            None => Ok(None),
        }
    }
}

struct CompoundMapAccess {
    iter: crate::compound::IntoIter,
    pending_value: Option<Tag>,
}

impl<'de> MapAccess<'de> for CompoundMapAccess {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        match self.iter.next() {
            Some((k, v)) => {
                self.pending_value = Some(v);
                seed.deserialize(TagDeserializer(Tag::String(k))).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let val = self
            .pending_value
            .take()
            .ok_or_else(|| Error::Serde("next_value_seed called before next_key_seed".into()))?;
        seed.deserialize(TagDeserializer(val))
    }
}

struct EnumAccess {
    variant: String,
    value: Tag,
}

impl<'de> de::EnumAccess<'de> for EnumAccess {
    type Error = Error;
    type Variant = VariantAccess;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let v = seed.deserialize(de::value::StringDeserializer::<Error>::new(self.variant))?;
        Ok((v, VariantAccess(self.value)))
    }
}

struct VariantAccess(Tag);

impl<'de> de::VariantAccess<'de> for VariantAccess {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(TagDeserializer(self.0))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_seq(TagDeserializer(self.0), visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_map(TagDeserializer(self.0), visitor)
    }
}
