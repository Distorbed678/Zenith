//! Pack types, pack-format values, ranges and compatibility classes (spec §1, §3.3, §3.4).

use std::fmt;

/// The minor value that stands for "any minor" (`*`).
pub const WILDCARD: i32 = i32::MAX;

/// Minimum value of the legacy multi-version `pack_format` (spec constants).
pub const MIN_LEGACY_PACK_FORMAT: i32 = 15;

/// Which kind of content a pack carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackType {
    /// Server data (`data/`).
    Data,
    /// Client resources (`assets/`).
    Resource,
}

impl PackType {
    /// The content root directory name.
    pub fn root(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Resource => "assets",
        }
    }

    /// The current format in 26.3.
    pub fn current_format(self) -> PackFormat {
        match self {
            Self::Data => PackFormat::new(121, 0),
            Self::Resource => PackFormat::new(97, 1),
        }
    }

    /// The last "pre-minor" major.
    pub fn last_pre_minor(self) -> i32 {
        match self {
            Self::Data => 81,
            Self::Resource => 64,
        }
    }
}

/// A pack-format value `(major, minor)`; compares by major first, then minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackFormat {
    /// Major version.
    pub major: i32,
    /// Minor version ([`WILDCARD`] means any).
    pub minor: i32,
}

impl PackFormat {
    /// Builds a format.
    pub const fn new(major: i32, minor: i32) -> Self {
        Self { major, minor }
    }

    /// The format `major.*`.
    pub const fn wildcard(major: i32) -> Self {
        Self {
            major,
            minor: WILDCARD,
        }
    }
}

impl fmt::Display for PackFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == WILDCARD {
            write!(f, "{}.*", self.major)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

/// An inclusive range of formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatRange {
    /// Lowest format.
    pub min: PackFormat,
    /// Highest format.
    pub max: PackFormat,
}

impl FormatRange {
    /// Builds a range.
    pub const fn new(min: PackFormat, max: PackFormat) -> Self {
        Self { min, max }
    }

    /// The "unknown" range of the fallback schema: `[2147483647.0, 2147483647.0]`.
    pub const UNKNOWN: Self = Self {
        min: PackFormat::new(i32::MAX, 0),
        max: PackFormat::new(i32::MAX, 0),
    };

    /// Whether `format` lies inside the range.
    pub fn contains(&self, format: PackFormat) -> bool {
        self.min <= format && format <= self.max
    }

    /// Compatibility of this range with the current format `current` (spec §3.4).
    pub fn classify(&self, current: PackFormat) -> Compatibility {
        if self.min.major == i32::MAX {
            Compatibility::Unknown
        } else if self.max < current {
            Compatibility::TooOld
        } else if current < self.min {
            Compatibility::TooNew
        } else {
            Compatibility::Compatible
        }
    }
}

impl fmt::Display for FormatRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]", self.min, self.max)
    }
}

/// Compatibility class of a pack's range with the running game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compatibility {
    /// The current format lies inside the range.
    Compatible,
    /// The range ends before the current format.
    TooOld,
    /// The range starts after the current format.
    TooNew,
    /// The pack declared no usable range (fallback schema).
    Unknown,
}

impl Compatibility {
    /// Only `Compatible` counts as compatible.
    pub fn is_compatible(self) -> bool {
        self == Self::Compatible
    }

    /// The translation key of the UI warning (`pack.incompatible.*`), if any.
    pub fn translation_key(self) -> Option<&'static str> {
        match self {
            Self::Compatible => None,
            Self::TooOld => Some("pack.incompatible.old"),
            Self::TooNew => Some("pack.incompatible.new"),
            Self::Unknown => Some("pack.incompatible.unknown"),
        }
    }
}

/// An inclusive range of integer majors (the legacy `supported_formats` / overlay `formats`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    /// Inclusive minimum.
    pub min: i32,
    /// Inclusive maximum.
    pub max: i32,
}

/// The inputs of the `pack` section's format declaration (spec §3.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormatDeclaration {
    /// `min_format`.
    pub min: Option<PackFormat>,
    /// `max_format`.
    pub max: Option<PackFormat>,
    /// Legacy `pack_format`.
    pub pack_format: Option<i32>,
    /// Legacy `supported_formats`.
    pub supported: Option<IntRange>,
}

/// Validates a format declaration with the ordered rules of spec §3.3 and returns the range.
pub fn validate_format_declaration(
    ty: PackType,
    decl: &FormatDeclaration,
) -> Result<FormatRange, String> {
    let lpm = ty.last_pre_minor();
    let FormatDeclaration {
        min: m,
        max: x,
        pack_format: p,
        supported: s,
    } = *decl;
    match (m, x) {
        (Some(_), None) | (None, Some(_)) => {
            Err("Both min_format and max_format must be declared, or neither".to_owned())
        }
        (Some(m), Some(x)) => {
            if m > x {
                return Err(format!(
                    "Invalid format range: min_format {m} is greater than max_format {x}"
                ));
            }
            if m.major > lpm {
                if s.is_some() {
                    return Err(format!(
                        "Remove supported_formats: it is deprecated for formats above {lpm}"
                    ));
                }
                if let Some(p) = p
                    && !(m.major..=x.major).contains(&p)
                {
                    return Err(format!(
                        "pack_format {p} must be within [{}, {}]",
                        m.major, x.major
                    ));
                }
                Ok(FormatRange::new(m, x))
            } else {
                let Some(s) = s else {
                    return Err(format!(
                        "Add supported_formats: min_format {m} targets versions that need it"
                    ));
                };
                if s.min != m.major {
                    return Err(format!(
                        "supported_formats must start at {} (min_format major)",
                        m.major
                    ));
                }
                if s.max != x.major && s.max != lpm {
                    return Err(format!(
                        "supported_formats must end at {} (max_format major) or {lpm}",
                        x.major
                    ));
                }
                let Some(p) = p else {
                    return Err("Add pack_format for versions that need it".to_owned());
                };
                if !(m.major..=x.major).contains(&p) || p < MIN_LEGACY_PACK_FORMAT {
                    return Err(format!(
                        "pack_format {p} must be within [{}, {}] and at least {MIN_LEGACY_PACK_FORMAT}",
                        m.major, x.major
                    ));
                }
                Ok(FormatRange::new(m, x))
            }
        }
        (None, None) => match (s, p) {
            (Some(s), p) => {
                if s.max > lpm {
                    return Err(format!(
                        "Use min_format and max_format: supported_formats ends above {lpm}"
                    ));
                }
                let Some(p) = p else {
                    return Err("Add pack_format alongside supported_formats".to_owned());
                };
                if !(s.min..=s.max).contains(&p) || p < MIN_LEGACY_PACK_FORMAT {
                    return Err(format!(
                        "pack_format {p} must be within [{}, {}] and at least {MIN_LEGACY_PACK_FORMAT}",
                        s.min, s.max
                    ));
                }
                Ok(FormatRange::new(
                    PackFormat::new(s.min, 0),
                    PackFormat::new(s.max, 0),
                ))
            }
            (None, Some(p)) => {
                if p > lpm {
                    Err(format!(
                        "pack_format {p} is above {lpm}: use min_format and max_format"
                    ))
                } else {
                    Ok(FormatRange::new(
                        PackFormat::new(p, 0),
                        PackFormat::new(p, 0),
                    ))
                }
            }
            (None, None) => Err("No pack format information was declared".to_owned()),
        },
    }
}
