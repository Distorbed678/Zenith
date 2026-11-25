//! The command NBT-path grammar and its query semantics (used by `nbt` text contents).
//!
//! Grammar (public command documentation): an optional leading compound filter, then nodes separated by `.`:
//! `key`, `"quoted key"`, `key{filter}`, `[]`, `[index]`, `[{filter}]`.

use zenith_nbt::{Compound, Tag, parse_snbt_compound};

/// One step of a path.
#[derive(Debug, Clone, PartialEq)]
pub enum PathNode {
    /// A leading `{…}`: keeps the root only when it matches.
    Root(Compound),
    /// A compound member.
    Key(String),
    /// A compound member that must also match a filter.
    KeyFiltered(String, Compound),
    /// `[]`: every list element.
    AllElements,
    /// `[n]`: one list element (negative counts from the end).
    Index(i32),
    /// `[{…}]`: list elements matching a filter.
    Filter(Compound),
}

/// A parsed NBT path.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtPath {
    source: String,
    nodes: Vec<PathNode>,
}

fn is_name_char(c: char) -> bool {
    !matches!(c, ' ' | '"' | '[' | ']' | '.' | '{' | '}')
}

impl NbtPath {
    /// The original text.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// The parsed nodes.
    pub fn nodes(&self) -> &[PathNode] {
        &self.nodes
    }

    /// Parses a complete path; the whole string must be consumed.
    pub fn parse(s: &str) -> Result<Self, String> {
        let chars: Vec<char> = s.chars().collect();
        let mut p = Parser { s: &chars, i: 0 };
        let nodes = p.parse_all()?;
        Ok(Self {
            source: s.to_owned(),
            nodes,
        })
    }

    /// Runs the path against one root tag, returning every selected value.
    pub fn get(&self, root: &Tag) -> Vec<Tag> {
        let mut cur = vec![root.clone()];
        for node in &self.nodes {
            let mut next = Vec::new();
            for t in &cur {
                match node {
                    PathNode::Root(f) => {
                        if matches_filter(&Tag::Compound(f.clone()), t) {
                            next.push(t.clone());
                        }
                    }
                    PathNode::Key(k) => {
                        if let Some(c) = t.as_compound()
                            && let Some(v) = c.get(k)
                        {
                            next.push(v.clone());
                        }
                    }
                    PathNode::KeyFiltered(k, f) => {
                        if let Some(c) = t.as_compound()
                            && let Some(v) = c.get(k)
                            && matches_filter(&Tag::Compound(f.clone()), v)
                        {
                            next.push(v.clone());
                        }
                    }
                    PathNode::AllElements => next.extend(elements(t)),
                    PathNode::Index(i) => {
                        let els = elements(t);
                        let idx = if *i < 0 {
                            els.len() as i64 + i64::from(*i)
                        } else {
                            i64::from(*i)
                        };
                        if idx >= 0 && (idx as usize) < els.len() {
                            next.push(els[idx as usize].clone());
                        }
                    }
                    PathNode::Filter(f) => {
                        if matches!(t, Tag::List(_)) {
                            let ft = Tag::Compound(f.clone());
                            next.extend(elements(t).into_iter().filter(|e| matches_filter(&ft, e)));
                        }
                    }
                }
            }
            cur = next;
        }
        cur
    }
}

fn elements(t: &Tag) -> Vec<Tag> {
    match t {
        Tag::List(l) => l.as_slice().to_vec(),
        Tag::ByteArray(a) => a.iter().map(|v| Tag::Byte(*v)).collect(),
        Tag::IntArray(a) => a.iter().map(|v| Tag::Int(*v)).collect(),
        Tag::LongArray(a) => a.iter().map(|v| Tag::Long(*v)).collect(),
        _ => Vec::new(),
    }
}

/// Subset match used by path filters: compounds match when every filter key is present and matches; lists match
/// when every filter element matches some target element (an empty filter list matches only an empty list); other
/// tags match when equal.
pub fn matches_filter(filter: &Tag, target: &Tag) -> bool {
    match (filter, target) {
        (Tag::Compound(f), Tag::Compound(t)) => f.iter().all(|(k, fv)| {
            t.get(&k.to_string_lossy())
                .is_some_and(|tv| matches_filter(fv, tv))
        }),
        (Tag::List(f), Tag::List(t)) => {
            if f.is_empty() {
                return t.is_empty();
            }
            f.as_slice()
                .iter()
                .all(|fe| t.as_slice().iter().any(|te| matches_filter(fe, te)))
        }
        (a, b) => a == b,
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn parse_all(&mut self) -> Result<Vec<PathNode>, String> {
        let mut nodes = Vec::new();
        // Spec A.2.2: parsing reads nodes until end-of-string or a space; anything remaining is
        // a trailing-data error. The empty path "" has zero nodes and selects the root tag.
        if self.s.is_empty() {
            return Ok(nodes);
        }
        if self.peek() == Some(' ') {
            return Err("Invalid NBT path: trailing data".to_owned());
        }
        let mut need_node = false;
        if self.peek() == Some('{') {
            let c = self.compound()?;
            nodes.push(PathNode::Root(c));
            // A separator is optional after the root filter.
            if self.peek() == Some('.') {
                self.i += 1;
                need_node = true;
            }
        }
        while self.i < self.s.len() && self.peek() != Some(' ') {
            match self.peek() {
                Some('[') => nodes.push(self.bracket()?),
                _ => nodes.push(self.key()?),
            }
            need_node = false;
            match self.peek() {
                None | Some(' ') => break,
                Some('.') => {
                    self.i += 1;
                    if self.i >= self.s.len() || self.peek() == Some(' ') {
                        return Err("Invalid NBT path: trailing '.'".to_owned());
                    }
                    need_node = true;
                }
                Some('[') => {}
                Some(c) => return Err(format!("Invalid NBT path: unexpected '{c}'")),
            }
        }
        if need_node {
            return Err("Invalid NBT path".to_owned());
        }
        if self.i < self.s.len() {
            return Err("Invalid NBT path: trailing data".to_owned());
        }
        Ok(nodes)
    }

    fn key(&mut self) -> Result<PathNode, String> {
        let name = if matches!(self.peek(), Some('"' | '\'')) {
            self.quoted()?
        } else {
            let start = self.i;
            while self.peek().is_some_and(is_name_char) {
                self.i += 1;
            }
            if start == self.i {
                return Err("Invalid NBT path: expected a key".to_owned());
            }
            self.s[start..self.i].iter().collect()
        };
        if self.peek() == Some('{') {
            let c = self.compound()?;
            Ok(PathNode::KeyFiltered(name, c))
        } else {
            Ok(PathNode::Key(name))
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        let q = self.peek().expect("quote");
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                c if c == q => return Ok(out),
                '\\' => match self.peek() {
                    Some(n) if n == q || n == '\\' => {
                        out.push(n);
                        self.i += 1;
                    }
                    _ => return Err("Invalid escape in NBT path".to_owned()),
                },
                c => out.push(c),
            }
        }
        Err("Unterminated quoted key".to_owned())
    }

    fn bracket(&mut self) -> Result<PathNode, String> {
        self.i += 1;
        let node = match self.peek() {
            Some(']') => PathNode::AllElements,
            Some('{') => PathNode::Filter(self.compound()?),
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                let t: String = self.s[start..self.i].iter().collect();
                PathNode::Index(
                    t.parse::<i32>()
                        .map_err(|_| "Invalid list index".to_owned())?,
                )
            }
            _ => return Err("Invalid NBT path: bad list selector".to_owned()),
        };
        if self.peek() != Some(']') {
            return Err("Invalid NBT path: expected ']'".to_owned());
        }
        self.i += 1;
        Ok(node)
    }

    fn compound(&mut self) -> Result<Compound, String> {
        let start = self.i;
        let mut depth = 0i32;
        let mut quote: Option<char> = None;
        let mut escaped = false;
        while let Some(c) = self.peek() {
            self.i += 1;
            if let Some(q) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                '"' | '\'' => quote = Some(c),
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let text: String = self.s[start..self.i].iter().collect();
                        return parse_snbt_compound(&text).map_err(|e| e.to_string());
                    }
                }
                _ => {}
            }
        }
        Err("Unterminated compound filter".to_owned())
    }
}
