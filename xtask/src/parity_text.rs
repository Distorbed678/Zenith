//! `text-component` parity suite: Zenith's text-component codec, NBT form and plain-string flattening against the
//! game's own, on our hand-made edge cases, every translation key in the user's jar and every component-like value
//! found in the jar's data files. The corpus is built at run time and never stored.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;

use anyhow::Context;
use zenith_codec::codec::DataResult;
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_components::registry_context::{RegistryContext, RegistryLookup};
use zenith_components::text::{
    LanguageTable, decode_component_json, decode_component_nbt, encode_component_json,
    encode_component_nbt, flatten_plain_utf16, flatten_truncated_utf16,
};
use zenith_nbt::Accounter;
use zenith_parity::SuiteResult;
use zenith_registry::Identifier;

use crate::parity::{json_canonical, vanilla_command};
use crate::repo_root;

/// Must equal `TextCheck.LIMIT`.
const LIMIT: usize = 8;

/// Hand-made edge cases (our own data), mostly from the spec's edge-case list.
const SYNTHETIC: &[&str] = &[
    // Shapes.
    r#""plain""#,
    r#""""#,
    r#"5"#,
    r#"2.5"#,
    r#"true"#,
    r#"null"#,
    r#"[]"#,
    r#"["a","b",{"text":"c","bold":true}]"#,
    r#"[["a"],"b"]"#,
    r#"{"text":"a"}"#,
    r#"{"text":""}"#,
    r#"{"text":5}"#,
    r#"{"text":"a","extra":[]}"#,
    r#"{"text":"a","extra":["b",{"text":"c"}]}"#,
    r#"{"text":"a","extra":"b"}"#,
    r#"{"type":"text","text":"a"}"#,
    r#"{"type":"Text","text":"a"}"#,
    r#"{"type":"translatable","text":"a"}"#,
    r#"{"type":"keybind","keybind":"key.jump"}"#,
    r#"{"text":"a","translate":"b"}"#,
    r#"{"translate":"b","score":{"name":"x","objective":"y"}}"#,
    r#"{"selector":"@p","keybind":"key.jump"}"#,
    r#"{"unknown":"a"}"#,
    r#"{}"#,
    r#"{"text":"a","unknown":5}"#,
    // Style.
    r#"{"text":"a","bold":true}"#,
    r#"{"text":"a","bold":false}"#,
    r#"{"text":"a","bold":1}"#,
    r#"{"text":"a","bold":0}"#,
    r#"{"text":"a","bold":256}"#,
    r#"{"text":"a","bold":0.5}"#,
    r#"{"text":"a","bold":"true"}"#,
    r#"{"text":"a","italic":true,"underlined":true,"strikethrough":true,"obfuscated":true}"#,
    r#"{"text":"a","color":"red"}"#,
    r#"{"text":"a","color":"Red"}"#,
    r##"{"text":"a","color":"#ff5555"}"##,
    r##"{"text":"a","color":"#FF5555"}"##,
    r##"{"text":"a","color":"#+FF"}"##,
    r##"{"text":"a","color":"#"}"##,
    r##"{"text":"a","color":"#GG0000"}"##,
    r##"{"text":"a","color":"#-1"}"##,
    r##"{"text":"a","color":"#1000000"}"##,
    r##"{"text":"a","color":"#F"}"##,
    r##"{"text":"a","color":"#0000FF00"}"##,
    r##"{"text":"a","color":"#FFFFFFFF"}"##,
    r##"{"text":"a","color":"#ＡＢ"}"##,
    r##"{"text":"a","color":"#١٢"}"##,
    r#"{"text":"a","color":"reset"}"#,
    r#"{"text":"a","shadow_color":-1}"#,
    r#"{"text":"a","shadow_color":0}"#,
    r#"{"text":"a","shadow_color":[1.0,0.5,0.25,1.0]}"#,
    r#"{"text":"a","shadow_color":[2.0,0,0,1]}"#,
    r#"{"text":"a","shadow_color":[-0.5,0,0,1]}"#,
    r#"{"text":"a","shadow_color":[1,0,0]}"#,
    r#"{"text":"a","shadow_color":"red"}"#,
    r#"{"text":"a","font":"minecraft:uniform"}"#,
    r#"{"text":"a","font":"Bad Font"}"#,
    r#"{"text":"a","insertion":"hi"}"#,
    r#"{"text":"a","insertion":5}"#,
    // Click events.
    r#"{"text":"a","click_event":{"action":"open_url","url":"https://example.com"}}"#,
    r#"{"text":"a","click_event":{"action":"open_url","url":"HTTPS://example.com"}}"#,
    r#"{"text":"a","click_event":{"action":"open_url","url":"ftp://example.com"}}"#,
    r#"{"text":"a","click_event":{"action":"open_url","url":"javascript:alert(1)"}}"#,
    r#"{"text":"a","click_event":{"action":"open_url","url":"example.com"}}"#,
    r#"{"text":"a","click_event":{"action":"open_url","url":"http://exa mple.com"}}"#,
    r#"{"text":"a","click_event":{"action":"open_file","path":"x"}}"#,
    r#"{"text":"a","click_event":{"action":"run_command","command":"/say hi"}}"#,
    r#"{"text":"a","click_event":{"action":"run_command","command":"say §c"}}"#,
    r#"{"text":"a","click_event":{"action":"run_command","command":"say\nhi"}}"#,
    r#"{"text":"a","click_event":{"action":"run_command","command":"say\thi"}}"#,
    r#"{"text":"a","click_event":{"action":"run_command","command":"say\u007f"}}"#,
    r#"{"text":"a","click_event":{"action":"suggest_command","command":"/tp"}}"#,
    r#"{"text":"a","click_event":{"action":"copy_to_clipboard","value":"§\n"}}"#,
    r#"{"text":"a","click_event":{"action":"change_page","page":3}}"#,
    r#"{"text":"a","click_event":{"action":"change_page","page":0}}"#,
    r#"{"text":"a","click_event":{"action":"change_page","page":-1}}"#,
    r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"minecraft:server_links"}}"#,
    r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"minecraft:not_a_dialog"}}"#,
    r#"{"text":"a","click_event":{"action":"show_dialog","dialog":{"type":"minecraft:notice","title":"T"}}}"#,
    r#"{"text":"a","click_event":{"action":"show_dialog","dialog":{"type":"minecraft:nope","title":"T"}}}"#,
    r#"{"text":"a","click_event":{"action":"custom","id":"zenith:test"}}"#,
    r#"{"text":"a","click_event":{"action":"custom","id":"zenith:test","payload":"x"}}"#,
    r#"{"text":"a","click_event":{"action":"nope"}}"#,
    r#"{"text":"a","clickEvent":{"action":"open_url","value":"https://example.com"}}"#,
    // Hover events.
    r#"{"text":"a","hover_event":{"action":"show_text","value":"tip"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_text","value":{"text":"tip","italic":true}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","count":5}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:not_an_item"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","count":0}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:air"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"stone","count":99}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","count":100}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","components":{"minecraft:max_stack_size":5}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:diamond_sword","components":{"minecraft:enchantments":{"minecraft:sharpness":2}}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:diamond_sword","components":{"minecraft:enchantments":{"minecraft:not_real":2}}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","components":{"!minecraft:max_stack_size":{}}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","components":{"minecraft:custom_name":"N"}}}"#,
    r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":"00000001-0002-0003-0004-000000000005"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":"1-1-1-1-1"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[1,2,3,4]}}"#,
    r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":"1-1-1-1-1","name":"Bob"}}"#,
    r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:nope","uuid":"1-1-1-1-1"}}"#,
    r#"{"text":"a","hoverEvent":{"action":"show_text","contents":"tip"}}"#,
    // Translation.
    r#"{"translate":"zenith.missing"}"#,
    r#"{"translate":"zenith.missing","fallback":"F %s","with":["x"]}"#,
    r#"{"translate":"zenith.missing","fallback":5}"#,
    r#"{"translate":"%s %1$s %s","with":["A","B"]}"#,
    r#"{"translate":"%%"}"#,
    r#"{"translate":"100%"}"#,
    r#"{"translate":"50% off"}"#,
    r#"{"translate":"zenith.missing","fallback":"%d"}"#,
    r#"{"translate":"%0$s","with":["A"]}"#,
    r#"{"translate":"%3$s","with":["A","B"]}"#,
    r#"{"translate":"%1$%","with":["A"]}"#,
    r#"{"translate":"%s%s","with":["A"]}"#,
    r#"{"translate":"%s","with":[true]}"#,
    r#"{"translate":"%s","with":[2.0]}"#,
    r#"{"translate":"%s","with":[2]}"#,
    r#"{"translate":"%s","with":[1e20]}"#,
    r#"{"translate":"%s","with":[-0.0]}"#,
    r#"{"translate":"%s","with":[0.1]}"#,
    r#"{"translate":"%s","with":[123456789012]}"#,
    r#"{"translate":"%s","with":[1.5e300]}"#,
    r#"{"translate":"%s","with":[1e2]}"#,
    r#"{"translate":"%s","with":[-0]}"#,
    r#"{"translate":"%s","with":[1e-400]}"#,
    r#"{"translate":"%s","with":[1.0000000000000000001]}"#,
    r#"{"translate":"%s","with":[9007199254740993.0]}"#,
    r#"{"translate":"%s","with":[100000000000000000000]}"#,
    r#"{"translate":"%s","with":[9223372036854775808]}"#,
    r#"{"translate":"%s","with":[-9223372036854775808]}"#,
    r#"{"translate":"%s","with":[16777217]}"#,
    r#"{"translate":"%s","with":[16777217.5]}"#,
    r#"{"translate":"%s","with":[3.4028235e38]}"#,
    r#"{"translate":"%s","with":[0.5]}"#,
    r#"{"translate":"%s","with":[128]}"#,
    r#"{"translate":"%s","with":[-129]}"#,
    r#"{"translate":"%s","with":[32768]}"#,
    r#"{"translate":"%s","with":[2147483648]}"#,
    r#"{"translate":"%s","with":["s",1,true,{"text":"c"}]}"#,
    r#"{"translate":"%s","with":[{"text":"x"}]}"#,
    r#"{"translate":"%s","with":[{"text":"x","bold":true}]}"#,
    r#"{"translate":"%s","with":[["x","y"]]}"#,
    r#"{"translate":"%s","with":[null]}"#,
    r#"{"translate":"%s","with":[]}"#,
    r#"{"translate":"%s","with":"x"}"#,
    r#"{"translate":"%s and %s","with":["a",{"translate":"%s","with":["b"]}]}"#,
    r#"{"translate":"gui.done"}"#,
    r#"{"translate":"chat.type.text","with":["Steve","hello"]}"#,
    r#"{"translate":"gui.done","fallback":"never"}"#,
    // Other contents.
    r#"{"score":{"name":"@p","objective":"obj"}}"#,
    r#"{"score":{"name":"*","objective":"obj"}}"#,
    r#"{"score":{"name":"Steve"}}"#,
    r#"{"score":{"name":"@p","objective":"obj","value":"5"}}"#,
    r#"{"selector":"@p"}"#,
    r#"{"selector":"@e[type=pig]","separator":{"text":"|"}}"#,
    r#"{"selector":"@p","separator":5}"#,
    r#"{"selector":"@"}"#,
    r#"{"selector":"Steve"}"#,
    r#"{"keybind":"key.jump"}"#,
    r#"{"keybind":""}"#,
    r#"{"nbt":"a","entity":"@s"}"#,
    r#"{"nbt":"a","block":"~ ~ ~"}"#,
    r#"{"nbt":"a","block":"1 2 3"}"#,
    r#"{"nbt":"a","block":"^ ^ ^1"}"#,
    r#"{"nbt":"a","block":"1 2"}"#,
    r#"{"nbt":"a","storage":"zenith:s"}"#,
    r#"{"nbt":"a","entity":"@s","block":"~ ~ ~"}"#,
    r#"{"nbt":"a","block":"~ ~ ~","storage":"zenith:s"}"#,
    r#"{"nbt":"a","source":"storage","storage":"zenith:s","entity":"@s"}"#,
    r#"{"nbt":"a","entity":"@s","interpret":true}"#,
    r#"{"nbt":"a","entity":"@s","interpret":"yes"}"#,
    r#"{"nbt":"a","entity":"@s","interpret":true,"plain":true}"#,
    r#"{"nbt":"a","entity":"@s","plain":true}"#,
    r#"{"nbt":"a","entity":"@s","separator":", "}"#,
    r#"{"nbt":"a"}"#,
    r#"{"nbt":"a[0].b{c:1}","entity":"@s"}"#,
    r#"{"nbt":"a.'b c'","entity":"@s"}"#,
    r#"{"nbt":"","entity":"@s"}"#,
    r#"{"nbt":"a[","entity":"@s"}"#,
    r#"{"nbt":" a","entity":"@s"}"#,
    r#"{"nbt":"a b","entity":"@s"}"#,
    r#"{"nbt":"a ","entity":"@s"}"#,
    r#"{"sprite":"item/apple"}"#,
    r#"{"sprite":"item/apple","atlas":"minecraft:items"}"#,
    r#"{"sprite":"item/apple","atlas":"minecraft:blocks"}"#,
    r#"{"object":"atlas","sprite":"item/apple"}"#,
    r#"{"type":"object","object":"atlas","sprite":"item/apple"}"#,
    r#"{"player":{"name":"Steve"}}"#,
    r#"{"player":"Steve"}"#,
    r#"{"player":{}}"#,
    r#"{"player":{"id":[1,2,3,4]}}"#,
    r#"{"player":{"name":"Steve"},"hat":false}"#,
    r#"{"object":"player","player":{"name":"Steve"}}"#,
    r#"{"object":"nope","sprite":"x"}"#,
    r#"{"sprite":"item/apple","fallback":"apple"}"#,
    // Formatting codes and odd strings.
    r#"{"text":"§cRed§rX"}"#,
    r#"{"text":"a§"}"#,
    r#"{"text":"§zq"}"#,
    r#"{"text":"\ud800"}"#,
    r#"{"text":"x\udc00y"}"#,
    r#"{"text":"😀 emoji"}"#,
    r#"{"text":"long text over the limit"}"#,
    r#"["0123","4567","89"]"#,
    r#"{"text":"😀😀😀😀😀"}"#,
];

/// Keys whose values in the jar's data files are (or may be) text components.
const HARVEST_KEYS: &[&str] = &[
    "title",
    "description",
    "author",
    "label",
    "contents",
    "name",
    "text",
    "message",
    "fallback",
];

fn harvest(j: &Json, out: &mut BTreeSet<String>) {
    match j {
        Json::Array(items) => items.iter().for_each(|i| harvest(i, out)),
        Json::Object(obj) => {
            if obj.contains_key("translate") || obj.contains_key("text") {
                out.insert(j.to_json_string());
            }
            for (k, v) in obj.iter() {
                let key = k.as_str().unwrap_or("");
                if HARVEST_KEYS.contains(&key) || key == "lore" || key == "pages" {
                    if (key == "lore" || key == "pages")
                        && let Json::Array(items) = v
                    {
                        for i in items {
                            out.insert(i.to_json_string());
                        }
                    }
                    out.insert(v.to_json_string());
                }
                harvest(v, out);
            }
        }
        _ => {}
    }
}

/// One input's results: `None` for each failed step.
#[derive(PartialEq, Eq, Debug)]
struct Outcome {
    status: u8,
    json: Option<Vec<u16>>,
    nbt: Option<Vec<u8>>,
    nbt_json: Option<Vec<u16>>,
    plain: Vec<u16>,
    truncated: Vec<u16>,
}

struct Reader<'a> {
    raw: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> anyhow::Result<u8> {
        let b = *self.raw.get(self.at).context("truncated output")?;
        self.at += 1;
        Ok(b)
    }
    fn len(&mut self) -> anyhow::Result<usize> {
        let b: [u8; 4] = self
            .raw
            .get(self.at..self.at + 4)
            .context("truncated output")?
            .try_into()?;
        self.at += 4;
        Ok(u32::from_be_bytes(b) as usize)
    }
    fn bytes(&mut self) -> anyhow::Result<Vec<u8>> {
        let n = self.len()?;
        let b = self
            .raw
            .get(self.at..self.at + n)
            .context("truncated output")?;
        self.at += n;
        Ok(b.to_vec())
    }
    fn units(&mut self) -> anyhow::Result<Vec<u16>> {
        let n = self.len()?;
        let b = self
            .raw
            .get(self.at..self.at + n * 2)
            .context("truncated output")?;
        self.at += n * 2;
        Ok(b.as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect())
    }
    fn flagged_units(&mut self) -> anyhow::Result<Option<Vec<u16>>> {
        Ok(if self.u8()? == 0 {
            Some(self.units()?)
        } else {
            None
        })
    }
    fn outcome(&mut self) -> anyhow::Result<Outcome> {
        let status = self.u8()?;
        if status != 0 {
            return Ok(Outcome {
                status,
                json: None,
                nbt: None,
                nbt_json: None,
                plain: Vec::new(),
                truncated: Vec::new(),
            });
        }
        let json = self.flagged_units()?;
        let nbt = if self.u8()? == 0 {
            Some(self.bytes()?)
        } else {
            None
        };
        let nbt_json = self.flagged_units()?;
        Ok(Outcome {
            status,
            json,
            nbt,
            nbt_json,
            plain: self.units()?,
            truncated: self.units()?,
        })
    }
}

fn canonical(j: &Json) -> Vec<u16> {
    let mut out = Vec::new();
    json_canonical(j, &mut out);
    out
}

fn success<T>(r: DataResult<T>) -> Option<T> {
    match r {
        DataResult::Success(v) => Some(v),
        _ => None,
    }
}

/// Registry context for the parity suite: static registries from `zenith_data::Tables` and
/// dynamic registries backed by the jar's `data/<id-namespace>/<registry-path>/<id-path>.json`
/// files (spec A.6.4).
struct SuiteRegistries {
    static_regs: HashMap<String, HashSet<String>>,
    jar_data_files: HashSet<String>,
    present_dynamic_dirs: HashSet<String>,
}

impl SuiteRegistries {
    fn new(tables: &zenith_data::Tables, jar_data_names: &[String]) -> Self {
        let mut static_regs = HashMap::with_capacity(tables.registries.len());
        for reg in &tables.registries {
            let entries: HashSet<String> = reg.entries.iter().map(|e| e.name.clone()).collect();
            static_regs.insert(reg.name.clone(), entries);
        }
        let mut jar_data_files = HashSet::with_capacity(jar_data_names.len());
        let mut present_dynamic_dirs = HashSet::new();
        for name in jar_data_names {
            jar_data_files.insert(name.clone());
            if let Some(after_data) = name.strip_prefix("data/")
                && let Some((_, rest)) = after_data.split_once('/')
            {
                for (idx, b) in rest.bytes().enumerate() {
                    if b == b'/' {
                        present_dynamic_dirs.insert(rest[..idx].to_owned());
                    }
                }
            }
        }
        Self {
            static_regs,
            jar_data_files,
            present_dynamic_dirs,
        }
    }
}

impl RegistryContext for SuiteRegistries {
    fn lookup(&self, registry: &Identifier, id: &Identifier) -> RegistryLookup {
        let reg_key = registry.to_string();
        if let Some(entries) = self.static_regs.get(&reg_key) {
            return if entries.contains(&id.to_string()) {
                RegistryLookup::Present
            } else {
                RegistryLookup::Missing
            };
        }
        let reg_path = registry.path();
        if !self.present_dynamic_dirs.contains(reg_path) {
            return RegistryLookup::Unavailable;
        }
        let file = format!("data/{}/{}/{}.json", id.namespace(), reg_path, id.path());
        if self.jar_data_files.contains(&file) {
            RegistryLookup::Present
        } else {
            RegistryLookup::Missing
        }
    }
}

/// Zenith's outcome for one input. `vanilla_nbt` is the game's own NBT for this input, so the NBT decode path is
/// checked independently of our NBT encoder.
fn zenith_outcome(
    input: &[u8],
    lang: &LanguageTable,
    registries: &dyn RegistryContext,
    vanilla_nbt: Option<&[u8]>,
) -> Outcome {
    let failed = |status| Outcome {
        status,
        json: None,
        nbt: None,
        nbt_json: None,
        plain: Vec::new(),
        truncated: Vec::new(),
    };
    let Ok(tree) = parse_bytes(input, ParseMode::Data) else {
        return failed(2);
    };
    let Some(c) = success(decode_component_json(&tree, registries)) else {
        return failed(1);
    };
    let json = success(encode_component_json(&c)).map(|j| canonical(&j));
    let nbt =
        success(encode_component_nbt(&c)).and_then(|t| zenith_nbt::to_bytes_network(Some(&t)).ok());
    let nbt_json = vanilla_nbt
        .and_then(|b| {
            zenith_nbt::from_bytes_network(b, &mut Accounter::uncompressed())
                .ok()
                .flatten()
        })
        .and_then(|t| success(decode_component_nbt(&t, registries)))
        .and_then(|c| success(encode_component_json(&c)))
        .map(|j| canonical(&j));
    Outcome {
        status: 0,
        json,
        nbt,
        nbt_json,
        plain: flatten_plain_utf16(&c, lang, None).to_utf16(),
        truncated: flatten_truncated_utf16(&c, LIMIT, lang, None).to_utf16(),
    }
}

fn run_text_check(version: &str, inputs: &[Vec<u8>]) -> anyhow::Result<Vec<u8>> {
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/text-component");
    fs::create_dir_all(&dir)?;
    let (corpus, results) = (dir.join("text.in"), dir.join("text.out"));
    let mut buf = Vec::new();
    for i in inputs {
        buf.extend_from_slice(&u32::try_from(i.len())?.to_be_bytes());
        buf.extend_from_slice(i);
    }
    fs::write(&corpus, buf)?;
    let out = vanilla_command(&java.java)?
        .arg("-Xmx2G")
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.TextCheck")
        .arg(&corpus)
        .arg(&results)
        .output()
        .context("run vanilla text check")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla text check failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&results)?;
    let _ = fs::remove_file(&corpus); // contains game data
    let _ = fs::remove_file(&results);
    Ok(raw)
}

fn show(u: &[u16]) -> String {
    let s = String::from_utf16_lossy(&u[..u.len().min(90)]);
    if u.len() > 90 { format!("{s}…") } else { s }
}

fn describe(label: &str, v: &Outcome, z: &Outcome) -> String {
    let opt = |o: &Option<Vec<u16>>| o.as_deref().map_or("<failed>".to_owned(), show);
    let mut parts = vec![label.to_string()];
    if v.status != z.status {
        parts.push(format!("status vanilla {} | zenith {}", v.status, z.status));
    }
    if v.json != z.json {
        parts.push(format!(
            "json\n      vanilla {}\n      zenith  {}",
            opt(&v.json),
            opt(&z.json)
        ));
    }
    if v.nbt != z.nbt {
        let hex = |b: &Option<Vec<u8>>| {
            b.as_ref().map_or("<failed>".to_owned(), |b| {
                b.iter()
                    .take(48)
                    .map(|x| format!("{x:02x}"))
                    .collect::<String>()
            })
        };
        parts.push(format!(
            "nbt\n      vanilla {}\n      zenith  {}",
            hex(&v.nbt),
            hex(&z.nbt)
        ));
    }
    if v.nbt_json != z.nbt_json {
        parts.push(format!(
            "nbt→json\n      vanilla {}\n      zenith  {}",
            opt(&v.nbt_json),
            opt(&z.nbt_json)
        ));
    }
    if v.plain != z.plain {
        parts.push(format!(
            "plain vanilla {:?} | zenith {:?}",
            show(&v.plain),
            show(&z.plain)
        ));
    }
    if v.truncated != z.truncated {
        parts.push(format!(
            "truncated vanilla {:?} | zenith {:?}",
            show(&v.truncated),
            show(&z.truncated)
        ));
    }
    parts.join("\n    ")
}

pub fn text_component(version: &str, write: bool) -> anyhow::Result<bool> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install")?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&install.client_jar)?)?;

    // The game's default language, read from the user's jar like the game does (en_us + deprecated).
    let mut lang_bytes = Vec::new();
    zip.by_name("assets/minecraft/lang/en_us.json")?
        .read_to_end(&mut lang_bytes)?;
    let dep_bytes = match zip.by_name("assets/minecraft/lang/deprecated.json") {
        Ok(mut f) => {
            let mut b = Vec::new();
            f.read_to_end(&mut b)?;
            Some(b)
        }
        Err(zip::result::ZipError::FileNotFound) => None,
        Err(e) => return Err(e.into()),
    };
    let lang = LanguageTable::from_default_json(&lang_bytes, dep_bytes.as_deref())
        .map_err(anyhow::Error::msg)?;
    let lang_json = parse_bytes(&lang_bytes, ParseMode::Data).context("parse en_us.json")?;
    let Json::Object(lang_obj) = &lang_json else {
        anyhow::bail!("en_us.json is not an object");
    };
    let entries: Vec<(&str, &str)> = lang_obj
        .iter()
        .filter_map(|(k, v)| Some((k.as_str()?, v.as_str()?)))
        .collect();

    let mut labels: Vec<String> = Vec::new();
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for (i, s) in SYNTHETIC.iter().enumerate() {
        labels.push(format!("synthetic#{i} {s}"));
        inputs.push(s.as_bytes().to_vec());
    }
    let synthetic = inputs.len();

    // Every translation key, bare and with mixed arguments.
    for (k, _) in &entries {
        let key = Json::string(*k).to_json_string();
        labels.push(format!("lang {k}"));
        inputs.push(format!(r#"{{"translate":{key}}}"#).into_bytes());
        labels.push(format!("lang+args {k}"));
        inputs.push(
            format!(r#"{{"translate":{key},"with":["A",{{"text":"B","bold":true}},3,2.5,true]}}"#)
                .into_bytes(),
        );
    }
    let lang_inputs = inputs.len() - synthetic;

    // Component-like values in the jar's data files.
    let names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("data/") && n.ends_with(".json"))
        .map(str::to_owned)
        .collect();
    let mut found = BTreeSet::new();
    for n in &names {
        let mut b = Vec::new();
        zip.by_name(n)?.read_to_end(&mut b)?;
        if let Ok(j) = parse_bytes(&b, ParseMode::Data) {
            harvest(&j, &mut found);
        }
    }
    let harvested = found.len();
    for (i, s) in found.into_iter().enumerate() {
        labels.push(format!(
            "jar#{i} {}",
            s.chars().take(80).collect::<String>()
        ));
        inputs.push(s.into_bytes());
    }

    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let registries = SuiteRegistries::new(&tables, &names);

    let raw = run_text_check(version, &inputs)?;
    let mut reader = Reader { raw: &raw, at: 0 };
    let (mut passed, mut total) = (0u64, 0u64);
    let mut failures: Vec<String> = Vec::new();
    let mut by_aspect = [0u64; 6];
    let limit: usize = std::env::var("ZENITH_PARITY_SHOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    for (i, input) in inputs.iter().enumerate() {
        let v = reader.outcome()?;
        let z = zenith_outcome(input, &lang, &registries, v.nbt.as_deref());
        total += 1;
        if v == z {
            passed += 1;
            continue;
        }
        for (slot, differs) in [
            v.status != z.status,
            v.json != z.json,
            v.nbt != z.nbt,
            v.nbt_json != z.nbt_json,
            v.plain != z.plain,
            v.truncated != z.truncated,
        ]
        .into_iter()
        .enumerate()
        {
            by_aspect[slot] += u64::from(differs);
        }
        if failures.len() < limit {
            failures.push(describe(&labels[i], &v, &z));
        }
    }
    anyhow::ensure!(reader.at == raw.len(), "vanilla returned extra output");

    println!(
        "text-component {version}: {passed}/{total} identical ({synthetic} synthetic, {lang_inputs} translation, {harvested} jar values)"
    );
    if passed != total {
        println!(
            "  differing: status {} · json {} · nbt {} · nbt→json {} · plain {} · truncated {}",
            by_aspect[0], by_aspect[1], by_aspect[2], by_aspect[3], by_aspect[4], by_aspect[5]
        );
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("text-component", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}
