//! `pack.mcmeta`: reading, section codecs, format validation, overlays, features, filter (spec §3).

use std::sync::Arc;

use zenith_registry::{FeatureFlagSet, FeatureUniverse};

use crate::codec::{
    Codec, DataResult, Either, IdentifierCodec, IntCodec, RecordReader, StringCodec, either, list,
    non_negative_int, sized_list,
};
use crate::filter::{PackFilter, PackFilterCodec};
use crate::format::{
    Compatibility, FormatDeclaration, FormatRange, IntRange, PackFormat, PackType, WILDCARD,
    validate_format_declaration,
};
use crate::json::{self, Json, JsonObject, ParseMode};
use crate::log::{LogExt, LogSink};
use crate::resources::{PACK_META, PackResources};
use crate::text::{Component, ComponentCodec};

// ----------------------------------------------------------------------------------------------------------
// Reading the file
// ----------------------------------------------------------------------------------------------------------

/// The parsed `pack.mcmeta` of one pack (parsed once; sections are decoded on demand).
#[derive(Debug, Clone)]
pub enum McmetaFile {
    /// No `pack.mcmeta` at the pack root.
    Missing,
    /// The file is not valid metadata JSON.
    Malformed(String),
    /// The root object.
    Object(JsonObject),
}

impl McmetaFile {
    /// Reads and parses the pack's root `pack.mcmeta` with the metadata token rules (spec §3.1).
    pub fn read(resources: &dyn PackResources) -> Self {
        match resources.root_resource(PACK_META) {
            None => Self::Missing,
            Some(bytes) => match json::parse_bytes(&bytes, ParseMode::Metadata) {
                Ok(Json::Object(o)) => Self::Object(o),
                Ok(_) => Self::Malformed("root is not an object".to_owned()),
                Err(e) => Self::Malformed(e.to_string()),
            },
        }
    }

    /// Parses metadata text directly.
    pub fn from_text(text: &str) -> Self {
        match json::parse(text, ParseMode::Metadata) {
            Ok(Json::Object(o)) => Self::Object(o),
            Ok(_) => Self::Malformed("root is not an object".to_owned()),
            Err(e) => Self::Malformed(e.to_string()),
        }
    }

    /// The raw value of a top-level section (a key set to `null` is present).
    pub fn section(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(o) => o.get(key),
            _ => None,
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// Value encodings
// ----------------------------------------------------------------------------------------------------------

/// Format value (spec §3.3): a non-negative number, or 1–256 non-negative integers.
#[derive(Debug, Clone, Copy)]
pub struct FormatValueCodec {
    /// `true` for the "max" encoding (a bare major means `major.*`), `false` for "min" (`major.0`).
    pub is_max: bool,
}

impl FormatValueCodec {
    fn major_to_format(&self, major: i32) -> PackFormat {
        PackFormat::new(major, if self.is_max { WILDCARD } else { 0 })
    }
}

impl Codec<PackFormat> for FormatValueCodec {
    fn decode(&self, input: &Json) -> DataResult<PackFormat> {
        let codec = either(non_negative_int(), sized_list(non_negative_int(), 1, 256));
        codec.decode(input).map(|e| match e {
            Either::Left(n) => self.major_to_format(n),
            Either::Right(v) if v.len() == 1 => self.major_to_format(v[0]),
            Either::Right(v) => PackFormat::new(v[0], v[1]),
        })
    }

    fn encode(&self, value: &PackFormat) -> DataResult<Json> {
        let arr = [value.major, value.minor];
        list(IntCodec).encode(&arr.to_vec())
    }
}

/// Integer range (spec §3.3): a number, `[a, b]`, or `{min_inclusive, max_inclusive}`, with `a ≤ b`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IntRangeCodec;

fn ordered(r: IntRange) -> DataResult<IntRange> {
    if r.min <= r.max {
        DataResult::Success(r)
    } else {
        DataResult::error(format!(
            "Invalid range: {} is greater than {}",
            r.min, r.max
        ))
    }
}

impl Codec<IntRange> for IntRangeCodec {
    fn decode(&self, input: &Json) -> DataResult<IntRange> {
        let num = IntCodec.decode(input);
        let first = match num {
            DataResult::Success(n) => return DataResult::Success(IntRange { min: n, max: n }),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        let arr = sized_list(IntCodec, 2, 2).decode(input).and_then(|v| {
            ordered(IntRange {
                min: v[0],
                max: v[1],
            })
        });
        let second = match arr {
            DataResult::Success(r) => return DataResult::Success(r),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        let obj = {
            let mut r = RecordReader::new(input);
            let min = r.required("min_inclusive", &IntCodec);
            let max = r.required("max_inclusive", &IntCodec);
            r.finish(|| {
                Some(IntRange {
                    min: min?,
                    max: max?,
                })
            })
            .and_then(ordered)
        };
        match obj {
            DataResult::Success(r) => DataResult::Success(r),
            DataResult::Failure(m) => DataResult::Failure(m),
            DataResult::Error { message, .. } => DataResult::error(format!(
                "Failed to parse either. First: {first}; Second: {second}; Third: {message}"
            )),
        }
    }

    fn encode(&self, value: &IntRange) -> DataResult<Json> {
        if value.min == value.max {
            IntCodec.encode(&value.min)
        } else {
            list(IntCodec).encode(&vec![value.min, value.max])
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// The `pack` section
// ----------------------------------------------------------------------------------------------------------

/// A decoded `pack` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackSection {
    /// The description.
    pub description: Component,
    /// The validated format range ([`FormatRange::UNKNOWN`] for the fallback schema).
    pub range: FormatRange,
}

/// Decodes the strict `pack` schema (spec §3.3).
pub fn decode_pack_section(input: &Json, ty: PackType) -> DataResult<PackSection> {
    let mut r = RecordReader::new(input);
    let description = r.required("description", &ComponentCodec);
    let min = r.optional_opt("min_format", &FormatValueCodec { is_max: false });
    let max = r.optional_opt("max_format", &FormatValueCodec { is_max: true });
    let pack_format = r.optional_opt("pack_format", &IntCodec);
    let supported = r.optional_opt("supported_formats", &IntRangeCodec);
    if r.has_errors() {
        return r.finish(|| None);
    }
    let decl = FormatDeclaration {
        min,
        max,
        pack_format,
        supported,
    };
    match validate_format_declaration(ty, &decl) {
        Ok(range) => r.finish(|| {
            Some(PackSection {
                description: description?,
                range,
            })
        }),
        Err(m) => {
            r.push_error(m);
            r.finish(|| None)
        }
    }
}

/// Decodes the fallback `pack` schema: only `description` is read, the range is unknown.
pub fn decode_pack_section_fallback(input: &Json) -> DataResult<PackSection> {
    let mut r = RecordReader::new(input);
    let description = r.required("description", &ComponentCodec);
    r.finish(|| {
        Some(PackSection {
            description: description?,
            range: FormatRange::UNKNOWN,
        })
    })
}

// ----------------------------------------------------------------------------------------------------------
// The `features` section
// ----------------------------------------------------------------------------------------------------------

/// Decodes `{"enabled": [<flag id>…]}`; any unknown flag makes the section an error (partial = recognized flags).
pub fn decode_features_section(
    input: &Json,
    universe: &FeatureUniverse,
) -> DataResult<FeatureFlagSet> {
    let mut r = RecordReader::new(input);
    let ids = r.required("enabled", &list(IdentifierCodec));
    if r.has_errors() {
        return r.finish(|| None);
    }
    let ids = ids.unwrap_or_default();
    match universe.from_names(&ids) {
        Ok(set) => r.finish(|| Some(set)),
        Err(e) => {
            r.push_error(format!(
                "Unknown feature ids: {}",
                e.unknown
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            let partial = e.partial;
            r.finish(|| Some(partial))
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// The `overlays` section
// ----------------------------------------------------------------------------------------------------------

/// One raw overlay entry as declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayEntry {
    /// Directory name relative to the pack root (`[-_a-zA-Z0-9.]+`).
    pub directory: String,
    /// `min_format`.
    pub min_format: Option<PackFormat>,
    /// `max_format`.
    pub max_format: Option<PackFormat>,
    /// Legacy `formats`.
    pub formats: Option<IntRange>,
}

/// An overlay entry that passed validation, with its format range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidOverlay {
    /// Directory name.
    pub directory: String,
    /// Range of formats the overlay applies to.
    pub range: FormatRange,
}

fn is_overlay_directory(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

struct OverlayEntryCodec;

impl Codec<OverlayEntry> for OverlayEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<OverlayEntry> {
        let mut r = RecordReader::new(input);
        let directory = r.required(
            "directory",
            &crate::codec::validated(StringCodec, |s: &String| {
                if is_overlay_directory(s) {
                    Ok(())
                } else {
                    Err(format!("Invalid overlay directory name: {s}"))
                }
            }),
        );
        let min_format = r.optional_opt("min_format", &FormatValueCodec { is_max: false });
        let max_format = r.optional_opt("max_format", &FormatValueCodec { is_max: true });
        let formats = r.optional_opt("formats", &IntRangeCodec);
        r.finish(|| {
            Some(OverlayEntry {
                directory: directory?,
                min_format,
                max_format,
                formats,
            })
        })
    }

    fn encode(&self, _value: &OverlayEntry) -> DataResult<Json> {
        DataResult::error("overlay entries are read-only")
    }
}

/// Decodes the raw `overlays` section (`{"entries": [...]}`, `entries` required).
pub fn decode_overlay_entries(input: &Json) -> DataResult<Vec<OverlayEntry>> {
    let mut r = RecordReader::new(input);
    let entries = r.required("entries", &list(OverlayEntryCodec));
    r.finish(|| entries)
}

/// Validates the overlay list with the rules of spec §3.5. Entries with no format fields are skipped (logged); any
/// invalid entry fails the whole section.
pub fn validate_overlays(
    ty: PackType,
    entries: &[OverlayEntry],
    log: &dyn LogSink,
) -> Result<Vec<ValidOverlay>, String> {
    let lpm = ty.last_pre_minor();
    let min_version = entries
        .iter()
        .filter_map(|e| match (e.min_format, e.formats) {
            (Some(m), Some(f)) => Some(m.major.min(f.min)),
            (Some(m), None) => Some(m.major),
            (None, Some(f)) => Some(f.min),
            (None, None) => None,
        })
        .min();
    let require_old = min_version.is_some_and(|v| v <= lpm);
    let mut out = Vec::new();
    for e in entries {
        if e.min_format.is_none() && e.max_format.is_none() && e.formats.is_none() {
            log.warn(format!(
                "Unknown or broken overlay entry '{}', skipping",
                e.directory
            ));
            continue;
        }
        let dir = &e.directory;
        let range = match (e.min_format, e.max_format) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(format!(
                    "Overlay '{dir}': both min_format and max_format must be declared, or neither"
                ));
            }
            _ if require_old && e.formats.is_none() => {
                return Err(format!(
                    "Overlay '{dir}': missing `formats` (required because an overlay targets version {lpm} or older)"
                ));
            }
            (Some(m), Some(x)) => {
                if m > x {
                    return Err(format!(
                        "Overlay '{dir}': min_format {m} is greater than max_format {x}"
                    ));
                }
                if m.major > lpm && !require_old {
                    if e.formats.is_some() {
                        return Err(format!(
                            "Overlay '{dir}': remove `formats`: it is deprecated above {lpm}"
                        ));
                    }
                    FormatRange::new(m, x)
                } else {
                    let Some(f) = e.formats else {
                        return Err(format!("Overlay '{dir}': missing `formats`"));
                    };
                    if f.min != m.major {
                        return Err(format!(
                            "Overlay '{dir}': `formats` must start at {}",
                            m.major
                        ));
                    }
                    if f.max != x.major && f.max != lpm {
                        return Err(format!(
                            "Overlay '{dir}': `formats` must end at {} or {lpm}",
                            x.major
                        ));
                    }
                    FormatRange::new(m, x)
                }
            }
            (None, None) => {
                let f = e.formats.expect("at least one field is present");
                if f.max > lpm {
                    return Err(format!(
                        "Overlay '{dir}': `formats` ends above {lpm}: use min_format and max_format"
                    ));
                }
                FormatRange::new(PackFormat::new(f.min, 0), PackFormat::new(f.max, 0))
            }
        };
        out.push(ValidOverlay {
            directory: e.directory.clone(),
            range,
        });
    }
    Ok(out)
}

// ----------------------------------------------------------------------------------------------------------
// Whole-pack metadata
// ----------------------------------------------------------------------------------------------------------

/// The state of the `filter` section after discovery.
#[derive(Debug, Clone, Default)]
pub enum FilterSection {
    /// No `filter` key.
    #[default]
    Absent,
    /// A valid filter.
    Valid(Arc<PackFilter>),
    /// An invalid filter: ignored, with the error logged when the stack is built.
    Invalid(String),
}

impl FilterSection {
    /// The usable filter, if any.
    pub fn filter(&self) -> Option<&Arc<PackFilter>> {
        match self {
            Self::Valid(f) => Some(f),
            _ => None,
        }
    }
}

/// Everything discovery learns from `pack.mcmeta`.
#[derive(Debug, Clone)]
pub struct PackMetadata {
    /// The description.
    pub description: Component,
    /// The declared (or unknown) format range.
    pub range: FormatRange,
    /// Compatibility with the running game.
    pub compatibility: Compatibility,
    /// Feature flags the pack requests.
    pub requested_features: FeatureFlagSet,
    /// Active overlay directories, in listed order (decided once, at discovery).
    pub overlays: Vec<String>,
    /// The `filter` section.
    pub filter: FilterSection,
}

/// Why a pack was dropped during discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    /// No `pack.mcmeta`, or no `pack` key.
    MissingMetadata,
    /// Any failure while reading the metadata.
    Failed(String),
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMetadata => f.write_str("missing metadata"),
            Self::Failed(m) => write!(f, "failed to read pack metadata: {m}"),
        }
    }
}

/// Decodes the `filter` section of an already-parsed file.
pub fn decode_filter_section(file: &McmetaFile) -> FilterSection {
    match file.section("filter") {
        None => FilterSection::Absent,
        Some(json) => match PackFilterCodec.decode(json).demand() {
            Ok(f) => FilterSection::Valid(Arc::new(f)),
            Err(e) => FilterSection::Invalid(e.message),
        },
    }
}

/// Reads one pack's metadata with the ordered steps of spec §3.2.
pub fn read_pack_metadata(
    resources: &dyn PackResources,
    ty: PackType,
    universe: &FeatureUniverse,
    log: &dyn LogSink,
) -> Result<PackMetadata, DropReason> {
    let file = McmetaFile::read(resources);
    read_pack_metadata_from(&file, resources.pack_id(), ty, universe, log)
}

/// As [`read_pack_metadata`], from an already-parsed file.
pub fn read_pack_metadata_from(
    file: &McmetaFile,
    pack_id: &str,
    ty: PackType,
    universe: &FeatureUniverse,
    log: &dyn LogSink,
) -> Result<PackMetadata, DropReason> {
    // Steps 1–3: the `pack` section, with the fallback schema.
    let section = match file {
        McmetaFile::Missing => {
            log.warn(format!(
                "Pack {pack_id} is missing pack.mcmeta, dropping it"
            ));
            return Err(DropReason::MissingMetadata);
        }
        McmetaFile::Malformed(m) => {
            log.warn(format!(
                "Couldn't load pack metadata for {pack_id}: {m}; trying the fallback schema"
            ));
            return Err(DropReason::Failed(m.clone()));
        }
        McmetaFile::Object(_) => match file.section("pack") {
            None => {
                log.warn(format!("Pack {pack_id} has no pack section, dropping it"));
                return Err(DropReason::MissingMetadata);
            }
            Some(json) => match decode_pack_section(json, ty).demand() {
                Ok(s) => s,
                Err(e) => {
                    log.warn(format!(
                        "Couldn't load pack metadata for {pack_id} ({}), using the fallback schema",
                        e.message
                    ));
                    match decode_pack_section_fallback(json).demand() {
                        Ok(s) => s,
                        Err(e2) => {
                            log.warn(format!("Failed to read pack metadata for {pack_id}: {e2}"));
                            return Err(DropReason::Failed(e2.message));
                        }
                    }
                }
            },
        },
    };

    // Step 4: features (data packs only).
    let requested_features = match (ty, file.section("features")) {
        (PackType::Data, Some(json)) => match decode_features_section(json, universe).demand() {
            Ok(set) => set,
            Err(e) => {
                log.warn(format!("Failed to read pack metadata for {pack_id}: {e}"));
                return Err(DropReason::Failed(e.message));
            }
        },
        _ => FeatureFlagSet::empty(),
    };

    // Step 5: compatibility.
    let compatibility = section.range.classify(ty.current_format());

    // Step 6: overlays.
    let overlays = match file.section("overlays") {
        None => Vec::new(),
        Some(json) => {
            let entries = decode_overlay_entries(json)
                .demand()
                .map_err(|e| e.message)
                .and_then(|entries| validate_overlays(ty, &entries, log));
            match entries {
                Ok(valid) => {
                    let current = ty.current_format();
                    valid
                        .into_iter()
                        .filter(|o| o.range.contains(current))
                        .map(|o| o.directory)
                        .collect()
                }
                Err(m) => {
                    log.warn(format!("Failed to read pack metadata for {pack_id}: {m}"));
                    return Err(DropReason::Failed(m));
                }
            }
        }
    };

    Ok(PackMetadata {
        description: section.description,
        range: section.range,
        compatibility,
        requested_features,
        overlays,
        filter: decode_filter_section(file),
    })
}
