//! Heterogeneous NBT [`List`] with binary wrapper-element derivation.

use std::ops::{Deref, DerefMut};

use crate::error::{Error, FormatError};
use crate::tag::{Tag, TagType};

/// An owned NBT list that can hold heterogeneous [`Tag`] elements in memory.
///
/// On binary serialization:
/// - An empty list is written with element type [`TagType::End`] (`0`).
/// - A list whose elements all share a single tag type is written with that element type.
/// - A list with mixed element types is written with element type [`TagType::Compound`] (`10`),
///   wrapping non-compound elements in `{ "": elem }` and also wrapping any compound element
///   that already has the `{ "": ... }` wrapper shape so it survives round-tripping.
/// - Even in an all-compound list, any element that already has the wrapper shape `{ "": ... }`
///   is wrapped once on write so that unwrapping on read restores the original compound.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct List {
    elements: Vec<Tag>,
}

impl List {
    /// Creates an empty NBT list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    /// Creates an empty NBT list with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            elements: Vec::with_capacity(capacity),
        }
    }

    /// Appends a tag to the end of the list.
    pub fn push(&mut self, tag: Tag) {
        self.elements.push(tag);
    }

    /// Removes and returns the last element, or `None` if empty.
    pub fn pop(&mut self) -> Option<Tag> {
        self.elements.pop()
    }

    /// Returns the underlying slice of elements.
    #[must_use]
    pub fn as_slice(&self) -> &[Tag] {
        &self.elements
    }

    /// Consumes the list and returns the underlying `Vec<Tag>`.
    #[must_use]
    pub fn into_vec(self) -> Vec<Tag> {
        self.elements
    }

    /// Derives the wire element [`TagType`] for binary serialization.
    ///
    /// Returns [`FormatError::InvalidTagPlacement`] if any element is [`Tag::End`].
    pub fn wire_element_type(&self) -> Result<TagType, Error> {
        let Some(first) = self.elements.first() else {
            return Ok(TagType::End);
        };
        let first_type = first.tag_type();
        if first_type == TagType::End {
            return Err(FormatError::InvalidTagPlacement(TagType::End).into());
        }
        let mut mixed = false;
        for elem in &self.elements[1..] {
            let ty = elem.tag_type();
            if ty == TagType::End {
                return Err(FormatError::InvalidTagPlacement(TagType::End).into());
            }
            if ty != first_type {
                mixed = true;
            }
        }
        if mixed {
            Ok(TagType::Compound)
        } else {
            Ok(first_type)
        }
    }
}

impl Deref for List {
    type Target = [Tag];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl DerefMut for List {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.elements
    }
}

impl From<Vec<Tag>> for List {
    fn from(elements: Vec<Tag>) -> Self {
        Self { elements }
    }
}

impl From<List> for Vec<Tag> {
    fn from(list: List) -> Self {
        list.elements
    }
}

impl IntoIterator for List {
    type Item = Tag;
    type IntoIter = std::vec::IntoIter<Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<'a> IntoIterator for &'a List {
    type Item = &'a Tag;
    type IntoIter = std::slice::Iter<'a, Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

impl<'a> IntoIterator for &'a mut List {
    type Item = &'a mut Tag;
    type IntoIter = std::slice::IterMut<'a, Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter_mut()
    }
}

impl FromIterator<Tag> for List {
    fn from_iter<T: IntoIterator<Item = Tag>>(iter: T) -> Self {
        Self {
            elements: iter.into_iter().collect(),
        }
    }
}
