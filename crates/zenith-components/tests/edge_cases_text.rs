//! Spec edge cases 1–36 (Part A: Text components) from `docs/specs/codec/text-and-data-components.md`.

use std::collections::{HashMap, HashSet};

use zenith_codec::json::{Json, ParseMode, parse};
use zenith_nbt::{Compound, NbtString, Tag, parse_snbt};
use zenith_registry::Identifier;

use zenith_components::dynval::{Dyn, Num};
use zenith_components::nbt_path::NbtPath;
use zenith_components::registry_context::{NoRegistries, RegistryContext, RegistryLookup};
use zenith_components::text::*;

fn from_json(s: &str) -> Component {
    decode_component_str_json(s, &NoRegistries)
        .demand()
        .unwrap_or_else(|e| panic!("expected valid JSON component for {s}: {e}"))
}

fn to_json_str(c: &Component) -> String {
    encode_component_json_string(c).demand().unwrap()
}

fn from_snbt(s: &str) -> Component {
    let tag = parse_snbt(s).unwrap_or_else(|e| panic!("bad SNBT {s}: {e}"));
    decode_component_nbt(&tag, &NoRegistries)
        .demand()
        .unwrap_or_else(|e| panic!("expected valid NBT component for {s}: {e}"))
}

#[derive(Default)]
struct MockSource {
    scores: HashMap<(String, String), Component>,
    entities: HashMap<String, Vec<ResolvedEntity>>,
    blocks: HashMap<String, Compound>,
    storage: HashMap<Identifier, Compound>,
}

impl ScoreLookup for MockSource {
    fn get_score(&self, holder: &str, objective: &str) -> Option<Component> {
        self.scores
            .get(&(holder.to_owned(), objective.to_owned()))
            .cloned()
    }
}

impl EntitySelectorLookup for MockSource {
    fn resolve_entities(&self, selector: &str) -> Result<Vec<ResolvedEntity>, ResolveError> {
        Ok(self.entities.get(selector).cloned().unwrap_or_default())
    }
}

impl NbtDataLookup for MockSource {
    fn block_entity_nbt(&self, block_pos: &str) -> Option<Compound> {
        self.blocks.get(block_pos).cloned()
    }

    fn storage_nbt(&self, id: &Identifier) -> Compound {
        self.storage.get(id).cloned().unwrap_or_default()
    }
}

#[test]
fn edge_01_bare_string_object_and_singleton_list_collapse_to_string() {
    let a = from_json(r#""hello""#);
    let b = from_json(r#"{"text":"hello"}"#);
    let c = from_json(r#"["hello"]"#);
    assert_eq!(a, b);
    assert_eq!(b, c);
    assert_eq!(to_json_str(&a), r#""hello""#);
    assert_eq!(to_json_str(&b), r#""hello""#);
    assert_eq!(to_json_str(&c), r#""hello""#);
}

#[test]
fn edge_02_empty_list_and_empty_extra_are_decode_errors() {
    assert!(
        decode_component_str_json("[]", &NoRegistries)
            .demand()
            .is_err()
    );
    assert!(
        decode_component_str_json(r#"{"text":"a","extra":[]}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_03_list_merges_into_first_element_inheriting_style() {
    let c = from_json(r#"[{"text":"a","color":"red"},"b"]"#);
    assert_eq!(c.siblings.len(), 1);
    let runs = render_styled_runs(&c, &Style::empty(), &EmptyLanguage, None);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text, "a");
    assert_eq!(runs[1].text, "b");
    assert_eq!(runs[0].style.color, TextColor::named("red"));
    assert_eq!(runs[1].style.color, TextColor::named("red"));
}

#[test]
fn edge_04_kind_fallback_order_and_explicit_type_no_fallback() {
    let c1 = from_json(r#"{"text":"a","translate":"b"}"#);
    assert_eq!(c1.content, Content::Literal(NbtString::from("a")));

    let c2 = from_json(r#"{"translate":"b","keybind":"c"}"#);
    assert!(matches!(c2.content, Content::Translatable(ref t) if t.key == "b"));

    assert!(
        decode_component_str_json(r#"{"type":"keybind","text":"x"}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_05_explicit_type_is_case_sensitive_with_no_fallback() {
    assert!(
        decode_component_str_json(r#"{"type":"Text","text":"a"}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_06_explicit_bold_false_preserved_as_map() {
    let c = from_json(r#"{"text":"a","bold":false}"#);
    assert_eq!(c.style.bold, Some(false));
    assert!(!c.style.is_empty());
    let encoded = to_json_str(&c);
    assert!(encoded.starts_with('{') && encoded.contains(r#""bold":false"#));
}

#[test]
fn edge_07_old_camelcase_click_and_hover_events_ignored() {
    let c = from_json(
        r#"{"text":"a","clickEvent":{"action":"run_command","value":"/hi"},"hoverEvent":{"action":"show_text","contents":"x"}}"#,
    );
    assert_eq!(c, Component::literal("a"));
    assert_eq!(c.style.click_event, None);
    assert_eq!(c.style.hover_event, None);
}

#[test]
fn edge_08_click_event_open_file_rejected_and_open_url_schemes() {
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"open_file","path":"/tmp/x"}}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    let in_mem = Component::literal("a").with_style(Style {
        click_event: Some(ClickEvent::OpenFile("/tmp/x".to_owned())),
        ..Style::empty()
    });
    assert!(encode_component(&in_mem).demand().is_err());

    for bad in [
        "ftp://example.com",
        "javascript:alert(1)",
        "example.com/no-scheme",
    ] {
        let s = format!(r#"{{"text":"a","click_event":{{"action":"open_url","url":"{bad}"}}}}"#);
        assert!(
            decode_component_str_json(&s, &NoRegistries)
                .demand()
                .is_err(),
            "{bad}"
        );
    }
    let ok = from_json(
        r#"{"text":"a","click_event":{"action":"open_url","url":"HTTPS://example.com/ok"}}"#,
    );
    assert_eq!(
        ok.style.click_event,
        Some(ClickEvent::OpenUrl("HTTPS://example.com/ok".to_owned()))
    );
}

#[test]
fn edge_09_run_command_rejects_unsafe_chars_copy_to_clipboard_accepts_any() {
    for bad in ["say §c hi", "say\nhi", "say\thi", "say\u{7f}hi"] {
        let j = Json::Object({
            let mut o = zenith_codec::json::JsonObject::new();
            o.insert("text", Json::string("a"));
            let mut ce = zenith_codec::json::JsonObject::new();
            ce.insert("action", Json::string("run_command"));
            ce.insert("command", Json::string(bad));
            o.insert("click_event", Json::Object(ce));
            o
        });
        assert!(
            decode_component_json(&j, &NoRegistries).demand().is_err(),
            "{bad:?}"
        );
    }
    let ok = from_json(
        r#"{"text":"a","click_event":{"action":"copy_to_clipboard","value":"§c\n\t\u007f"}}"#,
    );
    assert_eq!(
        ok.style.click_event,
        Some(ClickEvent::CopyToClipboard("§c\n\t\u{7f}".to_owned()))
    );
}

#[test]
fn edge_10_change_page_requires_at_least_one() {
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"change_page","page":0}}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"change_page","page":-5}}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    let ok = from_json(r#"{"text":"a","click_event":{"action":"change_page","page":1}}"#);
    assert_eq!(ok.style.click_event, Some(ClickEvent::ChangePage(1)));
}

#[test]
fn edge_11_color_parsing_and_hex_quirks() {
    assert!(TextColor::parse("Red").is_err());
    let c = TextColor::parse("#ff5555").unwrap();
    assert_eq!(c.rgb(), 0xFF5555);
    assert_eq!(c.serialize(), "#FF5555");

    let plus = TextColor::parse("#+FF").unwrap();
    assert_eq!(plus.rgb(), 0x0000FF);
    assert_eq!(plus.serialize(), "#0000FF");

    let short = TextColor::parse("#F").unwrap();
    assert_eq!(short.rgb(), 0x00000F);
    assert_eq!(short.serialize(), "#00000F");

    let padded = TextColor::parse("#0000FF00").unwrap();
    assert_eq!(padded.rgb(), 0x00FF00);

    for bad in ["#", "#GG0000", "#-1", "#1000000"] {
        assert!(TextColor::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn edge_12_named_and_hex_colors_compare_equal_but_encode_differently() {
    let c1 = from_json(r#"{"text":"a","color":"red"}"#);
    let c2 = from_json(r##"{"text":"a","color":"#FF5555"}"##);
    assert_eq!(c1.style, c2.style);
    assert_eq!(c1, c2);
    assert_eq!(to_json_str(&c1), r#"{"text":"a","color":"red"}"#);
    assert_eq!(to_json_str(&c2), r##"{"text":"a","color":"#FF5555"}"##);
}

#[test]
fn edge_13_shadow_color_float_list_wrapping_and_int_reencode() {
    let c1 = from_json(r#"{"text":"a","shadow_color":[2.0, 0, 0, 1]}"#);
    let expected1 = ((255i32) << 24) | (254 << 16);
    assert_eq!(c1.style.shadow_color, Some(expected1));
    assert!(to_json_str(&c1).contains(&format!(r#""shadow_color":{expected1}"#)));

    let c2 = from_json(r#"{"text":"a","shadow_color":[-0.5, 0, 0, 1]}"#);
    let expected2 = ((255i32) << 24) | (128 << 16);
    assert_eq!(c2.style.shadow_color, Some(expected2));
}

#[test]
fn edge_14_nbt_and_json_boolean_rules() {
    assert_eq!(from_snbt("{text:'a',bold:1b}").style.bold, Some(true));
    assert_eq!(from_snbt("{text:'a',bold:5}").style.bold, Some(true));
    assert_eq!(from_snbt("{text:'a',bold:0.5f}").style.bold, Some(true));
    assert_eq!(from_snbt("{text:'a',bold:0.0d}").style.bold, Some(false));
    assert!(
        decode_component_nbt(
            &parse_snbt("{text:'a',bold:'true'}").unwrap(),
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    // Spec A.6.1 rule 9 & edge case 14: JSON booleans must be true/false; numbers are rejected.
    for bad in [
        r#"{"text":"a","bold":1}"#,
        r#"{"text":"a","bold":0}"#,
        r#"{"text":"a","bold":256}"#,
        r#"{"text":"a","bold":0.5}"#,
    ] {
        assert!(
            decode_component_str_json(bad, &NoRegistries)
                .demand()
                .is_err(),
            "{bad}"
        );
    }
    assert_eq!(
        from_json(r#"{"text":"a","bold":true}"#).style.bold,
        Some(true)
    );
    assert_eq!(
        from_json(r#"{"text":"a","bold":false}"#).style.bold,
        Some(false)
    );
}

#[test]
fn edge_15_numbers_are_not_strings_in_nbt_or_json() {
    assert!(
        decode_component_nbt(&parse_snbt("{text:5}").unwrap(), &NoRegistries)
            .demand()
            .is_err()
    );
    assert!(
        decode_component_str_json(r#"{"text":5}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_16_lenient_vs_strict_optional_fields() {
    let t = from_json(r#"{"translate":"k","fallback":5}"#);
    match &t.content {
        Content::Translatable(tr) => {
            assert_eq!(tr.key, "k");
            assert_eq!(tr.fallback, None);
        }
        other => panic!("expected translatable, got {other:?}"),
    }

    let n = from_json(r#"{"nbt":"a","entity":"@s","interpret":"yes"}"#);
    match &n.content {
        Content::Nbt(nc) => assert!(!nc.interpret),
        other => panic!("expected nbt, got {other:?}"),
    }

    assert!(
        decode_component_str_json(r#"{"selector":"@p","separator":5}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_17_nbt_interpret_and_plain_both_true_and_missing_source_fail() {
    assert!(
        decode_component_str_json(
            r#"{"nbt":"a","entity":"@s","interpret":true,"plain":true}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(r#"{"nbt":"a"}"#, &NoRegistries)
            .demand()
            .is_err()
    );
}

#[test]
fn edge_18_nbt_source_precedence_entity_block_storage() {
    let c1 = from_json(r#"{"nbt":"a","entity":"@s","block":"0 0 0","storage":"minecraft:x"}"#);
    assert!(matches!(
        c1.content,
        Content::Nbt(NbtContent {
            source: NbtSource::Entity(_),
            ..
        })
    ));
    let c2 = from_json(r#"{"nbt":"a","block":"0 0 0","storage":"minecraft:x"}"#);
    assert!(matches!(
        c2.content,
        Content::Nbt(NbtContent {
            source: NbtSource::Block(_),
            ..
        })
    ));
}

#[test]
fn edge_19_nbt_interpret_string_tag_is_literal_not_json() {
    let mut src = MockSource::default();
    let mut comp = Compound::new();
    comp.insert("msg", Tag::String(r#"{"text":"x"}"#.into()));
    src.storage
        .insert(Identifier::parse("test:s").unwrap(), comp);

    let c = from_json(r#"{"nbt":"msg","storage":"test:s","interpret":true}"#);
    let mut ctx = ResolveContext::server(&src, None);
    let resolved = resolve_component(&c, &mut ctx).unwrap();
    assert_eq!(
        flatten_plain(&resolved, &EmptyLanguage, None),
        r#"{"text":"x"}"#
    );
}

#[test]
fn edge_20_nbt_non_interpret_string_renders_quoted_colored_unless_plain() {
    let mut src = MockSource::default();
    let mut comp = Compound::new();
    comp.insert("s", Tag::String("x".into()));
    src.storage
        .insert(Identifier::parse("test:s").unwrap(), comp);

    let colored_c = from_json(r#"{"nbt":"s","storage":"test:s"}"#);
    let mut ctx = ResolveContext::server(&src, None);
    let res_colored = resolve_component(&colored_c, &mut ctx).unwrap();
    assert_eq!(flatten_plain(&res_colored, &EmptyLanguage, None), r#""x""#);
    let runs = render_styled_runs(&res_colored, &Style::empty(), &EmptyLanguage, None);
    assert!(
        runs.iter()
            .any(|r| r.text == "x" && r.style.color == TextColor::named("green"))
    );

    let plain_c = from_json(r#"{"nbt":"s","storage":"test:s","plain":true}"#);
    let res_plain = resolve_component(&plain_c, &mut ctx).unwrap();
    assert_eq!(flatten_plain(&res_plain, &EmptyLanguage, None), r#""x""#);
    let runs_plain = render_styled_runs(&res_plain, &Style::empty(), &EmptyLanguage, None);
    assert!(runs_plain.iter().all(|r| r.style.color.is_none()));
}

#[test]
fn edge_21_unresolved_flattening_per_kind() {
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"score":{"name":"@p","objective":"obj"}}"#),
            &EmptyLanguage,
            None
        ),
        ""
    );
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"nbt":"a","storage":"minecraft:s"}"#),
            &EmptyLanguage,
            None
        ),
        ""
    );
    assert_eq!(
        flatten_plain(&from_json(r#"{"selector":"@p"}"#), &EmptyLanguage, None),
        "@p"
    );
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"keybind":"key.jump"}"#),
            &EmptyLanguage,
            None
        ),
        "key.jump"
    );
}

#[test]
fn edge_22_score_selector_multi_error_and_zero_matches_literal() {
    let mut src = MockSource::default();
    src.entities.insert(
        "@a".to_owned(),
        vec![
            ResolvedEntity {
                score_holder_name: "Alice".to_owned(),
                display_name: Component::literal("Alice"),
                nbt: None,
            },
            ResolvedEntity {
                score_holder_name: "Bob".to_owned(),
                display_name: Component::literal("Bob"),
                nbt: None,
            },
        ],
    );
    let c = from_json(r#"{"score":{"name":"@a","objective":"pts"}}"#);
    let mut ctx = ResolveContext::server(&src, None);
    assert!(matches!(
        resolve_component(&c, &mut ctx),
        Err(ResolveError::TooManyEntities { .. })
    ));

    // Zero matches -> uses literal holder name "@a".
    src.entities.clear();
    src.scores.insert(
        ("@a".to_owned(), "pts".to_owned()),
        Component::literal("42"),
    );
    let mut ctx2 = ResolveContext::server(&src, None);
    let res = resolve_component(&c, &mut ctx2).unwrap();
    assert_eq!(flatten_plain(&res, &EmptyLanguage, None), "42");
}

#[test]
fn edge_23_score_star_resolves_to_context_entity() {
    let mut src = MockSource::default();
    src.scores.insert(
        ("Steve".to_owned(), "kills".to_owned()),
        Component::literal("7"),
    );
    let c = from_json(r#"{"score":{"name":"*","objective":"kills"}}"#);
    let mut ctx = ResolveContext::server(&src, Some("Steve".to_owned()));
    let res = resolve_component(&c, &mut ctx).unwrap();
    assert_eq!(flatten_plain(&res, &EmptyLanguage, None), "7");
}

#[test]
fn edge_24_translation_sequential_and_positional_args() {
    let mut lang = LanguageTable::new();
    lang.insert_lang_entry("k", "%s %1$s %s");
    let c = from_json(r#"{"translate":"k","with":["A","B"]}"#);
    assert_eq!(flatten_plain(&c, &lang, None), "A A B");
}

#[test]
fn edge_25_translation_percent_and_malformed_templates() {
    let c_pct = from_json(r#"{"translate":"100%%"}"#);
    assert_eq!(flatten_plain(&c_pct, &EmptyLanguage, None), "100%");

    for raw in ["100%", "50% off", "%0$s", "%3$s", "%1$%"] {
        let c = Component::translatable(
            raw,
            vec![
                TranslationArg::Str(NbtString::from("A")),
                TranslationArg::Str(NbtString::from("B")),
            ],
        );
        assert_eq!(flatten_plain(&c, &EmptyLanguage, None), raw, "{raw}");
    }

    let fb_d = from_json(r#"{"translate":"missing","fallback":"%d","with":[1]}"#);
    assert_eq!(flatten_plain(&fb_d, &EmptyLanguage, None), "%d");
}

#[test]
fn edge_26_lang_file_df_rewrite_vs_fallback() {
    let mut lang = LanguageTable::new();
    lang.insert_lang_entry("k", "a=%d b=%.1f c=%2$d");
    let c = from_json(r#"{"translate":"k","with":[1,2.5]}"#);
    assert_eq!(flatten_plain(&c, &lang, None), "a=1 b=2.5 c=2.5");

    let fb = from_json(r#"{"translate":"missing","fallback":"a=%d","with":[1]}"#);
    assert_eq!(flatten_plain(&fb, &lang, None), "a=%d");
}

#[test]
fn edge_27_missing_key_uses_fallback_or_key_as_formatted_template() {
    let with_fb = from_json(r#"{"translate":"missing","fallback":"Hi %s","with":["Bob"]}"#);
    assert_eq!(flatten_plain(&with_fb, &EmptyLanguage, None), "Hi Bob");

    let no_fb = from_json(r#"{"translate":"Hi %s","with":["Bob"]}"#);
    assert_eq!(flatten_plain(&no_fb, &EmptyLanguage, None), "Hi Bob");
}

#[test]
fn edge_28_translation_arg_number_rendering_snbt_nbt_and_json() {
    assert_eq!(
        flatten_plain(
            &from_snbt("{translate:'%s',with:[true]}"),
            &EmptyLanguage,
            None
        ),
        "1"
    );
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"translate":"%s","with":[true]}"#),
            &EmptyLanguage,
            None
        ),
        "true"
    );
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"translate":"%s","with":[2.0]}"#),
            &EmptyLanguage,
            None
        ),
        "2"
    );
    assert_eq!(
        flatten_plain(
            &from_snbt("{translate:'%s',with:[2.0d]}"),
            &EmptyLanguage,
            None
        ),
        "2.0"
    );
    assert_eq!(
        flatten_plain(
            &from_json(r#"{"translate":"%s","with":[1e20]}"#),
            &EmptyLanguage,
            None
        ),
        "1.0E20"
    );
}

#[test]
fn edge_29_with_plain_text_collapses_to_string_styled_stays_component() {
    let c = from_json(r#"{"translate":"%s %s","with":[{"text":"x"},{"text":"y","bold":true}]}"#);
    match &c.content {
        Content::Translatable(t) => {
            assert_eq!(t.args[0], TranslationArg::Str(NbtString::from("x")));
            assert!(matches!(t.args[1], TranslationArg::Component(_)));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(
        to_json_str(&c),
        r#"{"translate":"%s %s","with":["x",{"text":"y","bold":true}]}"#
    );
}

#[test]
fn edge_30_object_without_type_default_atlas_and_player_fallback() {
    let a = from_json(r#"{"sprite":"item/apple"}"#);
    assert_eq!(flatten_plain(&a, &EmptyLanguage, None), "[item/apple]");

    let b = from_json(r#"{"sprite":"item/apple","atlas":"minecraft:items"}"#);
    assert_eq!(
        flatten_plain(&b, &EmptyLanguage, None),
        "[item/apple@items]"
    );

    let p = from_json(r#"{"player":{}}"#);
    assert_eq!(
        flatten_plain(&p, &EmptyLanguage, None),
        "[unknown player head]"
    );
}

#[test]
fn edge_31_motd_sanitizing_player_object_depth_16_and_no_source() {
    let player_obj = from_json(r#"{"player":{"name":"Steve"}}"#);
    let motd_player = resolve_motd(&player_obj);
    assert_eq!(
        flatten_plain(&motd_player, &EmptyLanguage, None),
        "[Steve head]"
    );

    // Build nested siblings of depth 18 (depths 0..=16 resolve, depth 17 becomes "...").
    let mut deep = Component::literal("leaf");
    for _ in 0..17 {
        deep = Component::literal("x").append(deep);
    }
    let resolved_deep = resolve_motd(&deep);
    let flat = flatten_plain(&resolved_deep, &EmptyLanguage, None);
    assert!(flat.ends_with("..."), "got {flat}");
    assert_eq!(flat, format!("{}...", "x".repeat(17)));

    // score / selector / nbt become empty without a source.
    assert_eq!(
        flatten_plain(
            &resolve_motd(&from_json(r#"{"selector":"@p"}"#)),
            &EmptyLanguage,
            None
        ),
        ""
    );
    assert_eq!(
        flatten_plain(
            &resolve_motd(&from_json(r#"{"score":{"name":"a","objective":"b"}}"#)),
            &EmptyLanguage,
            None
        ),
        ""
    );
    assert_eq!(
        flatten_plain(
            &resolve_motd(&from_json(r#"{"nbt":"a","storage":"minecraft:s"}"#)),
            &EmptyLanguage,
            None
        ),
        ""
    );
}

#[test]
fn edge_32_server_resolution_depth_100_copies_depth_101_unresolved() {
    let mut src = MockSource::default();
    src.entities.insert(
        "@p".to_owned(),
        vec![ResolvedEntity {
            score_holder_name: "Alice".to_owned(),
            display_name: Component::literal("Alice"),
            nbt: None,
        }],
    );

    // Node at depth 100 resolves; node at depth 101 stays unresolved ("@p").
    let leaf_101 = from_json(r#"{"selector":"@p"}"#);
    let mut chain = from_json(r#"{"selector":"@p"}"#).append(leaf_101); // 100 -> 101
    for _ in 0..100 {
        chain = Component::empty().append(chain);
    }
    let mut ctx = ResolveContext::server(&src, None);
    let resolved = resolve_component(&chain, &mut ctx).unwrap();
    assert_eq!(flatten_plain(&resolved, &EmptyLanguage, None), "Alice@p");
}

#[test]
fn edge_33_nbt_content_cap_65537_resolve_65538th_empty() {
    let mut src = MockSource::default();
    let mut comp = Compound::new();
    comp.insert("x", Tag::Int(1));
    src.storage
        .insert(Identifier::parse("test:cap").unwrap(), comp);

    let nbt_node = from_json(r#"{"nbt":"x","storage":"test:cap","plain":true}"#);
    let mut root = Component::empty();
    root.siblings = vec![nbt_node; 65_540];

    let mut ctx = ResolveContext::server(&src, None);
    let resolved = resolve_component(&root, &mut ctx).unwrap();
    assert_eq!(ctx.nbt_count, 65_537);
    let non_empty = resolved
        .siblings
        .iter()
        .filter(|s| !flatten_plain(s, &EmptyLanguage, None).is_empty())
        .count();
    assert_eq!(non_empty, 65_537);
    assert_eq!(
        flatten_plain(&resolved.siblings[65_536], &EmptyLanguage, None),
        "1"
    );
    assert_eq!(
        flatten_plain(&resolved.siblings[65_537], &EmptyLanguage, None),
        ""
    );
}

#[test]
fn edge_34_legacy_section_codes_rendering_and_plain_flattening() {
    let base = Style::empty().with_color(TextColor::named("blue").unwrap());
    let c = Component::literal("§cRed§rX§zY§")
        .with_style(base)
        .append(Component::literal("Sib"));

    // Plain-string flattening keeps codes verbatim.
    assert_eq!(flatten_plain(&c, &EmptyLanguage, None), "§cRed§rX§zY§Sib");

    // Styled rendering interprets codes within each segment only.
    let runs = render_styled_runs(&c, &Style::empty(), &EmptyLanguage, None);
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].text, "Red");
    assert_eq!(runs[0].style.color, TextColor::named("red"));
    assert_eq!(runs[1].text, "XY");
    assert_eq!(runs[1].style.color, TextColor::named("blue"));
    assert_eq!(runs[2].text, "Sib");
    assert_eq!(runs[2].style.color, TextColor::named("blue"));

    // Codes do not leak from parent segment into sibling even without §r.
    let c2 = Component::literal("§cRed").append(Component::literal("Next"));
    let runs2 = render_styled_runs(&c2, &Style::empty(), &EmptyLanguage, None);
    assert_eq!(runs2[0].style.color, TextColor::named("red"));
    assert_eq!(runs2[1].style.color, None);
}

#[test]
fn edge_35_unpaired_surrogates_render_as_fffd_plain_keeps_as_is() {
    let json = parse(r#""a\ud800b""#, ParseMode::Data).unwrap();
    let comp = decode_component_json(&json, &NoRegistries)
        .demand()
        .unwrap();

    // Plain UTF-16 flattening keeps the raw unpaired surrogate.
    let flat_u16 = flatten_plain_utf16(&comp, &EmptyLanguage, None);
    assert_eq!(flat_u16.to_utf16(), vec![b'a' as u16, 0xD800, b'b' as u16]);

    // Styled rendering turns unpaired surrogates into U+FFFD.
    let runs = render_styled_runs(&comp, &Style::empty(), &EmptyLanguage, None);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "a\u{FFFD}b");
}

#[test]
fn edge_36_written_book_page_32767_limit_on_decode_and_resolution() {
    let ok_text = "a".repeat(32_765); // `"..."` -> 32,767 chars
    assert!(
        decode_book_page(&Dyn::string(ok_text), &NoRegistries)
            .demand()
            .is_ok()
    );

    let too_long = "a".repeat(32_766); // `"..."` -> 32,768 chars
    assert!(
        decode_book_page(&Dyn::string(too_long), &NoRegistries)
            .demand()
            .is_err()
    );

    // On resolution: one page expanding beyond 32,767 JSON chars cancels resolution of ALL pages,
    // while still marking the book resolved.
    let mut src = MockSource::default();
    let mut comp = Compound::new();
    comp.insert("big", Tag::String("z".repeat(40_000).into()));
    comp.insert("small", Tag::String("ok".into()));
    src.storage
        .insert(Identifier::parse("test:book").unwrap(), comp);

    let p1 = from_json(r#"{"nbt":"small","storage":"test:book","plain":true}"#);
    let p2 = from_json(r#"{"nbt":"big","storage":"test:book","plain":true}"#);
    let mut ctx = ResolveContext::server(&src, None);
    let (result_pages, resolved_flag) = resolve_book_pages(&[p1.clone(), p2.clone()], &mut ctx);
    assert!(resolved_flag);
    assert_eq!(result_pages, vec![p1, p2]);
}

#[test]
fn default_language_table_deprecated_rules() {
    let en_us = br#"{
        "keep": "Value %d",
        "del": "Gone",
        "chain.a": "Moved",
        "overwrite.old": "New",
        "overwrite.new": "Old",
        "drop_target.new": "ShouldBeDeleted",
        "removed_then_renamed": "RemovedFirst",
        "removed_then_renamed.target": "AlsoDeleted"
    }"#;
    let deprecated = br#"{
        "removed": ["del", "removed_then_renamed"],
        "renamed": {
            "chain.a": "chain.b",
            "chain.b": "chain.c",
            "overwrite.old": "overwrite.new",
            "missing.old": "drop_target.new",
            "removed_then_renamed": "removed_then_renamed.target"
        }
    }"#;

    // Without deprecated.json: all en_us entries kept, %d rewritten to %s.
    let base = LanguageTable::from_default_json(en_us, None).unwrap();
    assert_eq!(base.get("keep"), Some("Value %s"));
    assert_eq!(base.get("del"), Some("Gone"));

    // With deprecated.json: removed deleted first, then renames applied in document order.
    let tbl = LanguageTable::from_default_json(en_us, Some(deprecated)).unwrap();
    assert_eq!(tbl.get("keep"), Some("Value %s"));
    assert_eq!(tbl.get("del"), None);
    // Rename chain: chain.a -> chain.b -> chain.c.
    assert_eq!(tbl.get("chain.a"), None);
    assert_eq!(tbl.get("chain.b"), None);
    assert_eq!(tbl.get("chain.c"), Some("Moved"));
    // Rename overwrites existing target.
    assert_eq!(tbl.get("overwrite.old"), None);
    assert_eq!(tbl.get("overwrite.new"), Some("New"));
    // Missing old key deletes the target key too.
    assert_eq!(tbl.get("drop_target.new"), None);
    // Key deleted by `removed` is absent when `renamed` runs, so target is also deleted.
    assert_eq!(tbl.get("removed_then_renamed"), None);
    assert_eq!(tbl.get("removed_then_renamed.target"), None);
}

#[test]
fn json_number_narrowing_exact_decimal() {
    let cases: &[(&str, Num, &str)] = &[
        ("-0.0", Num::Byte(0), "0"),
        ("-0", Num::Byte(0), "0"),
        ("2.0", Num::Byte(2), "2"),
        ("1e2", Num::Byte(100), "100"),
        ("128", Num::Short(128), "128"),
        ("-129", Num::Short(-129), "-129"),
        ("32768", Num::Int(32768), "32768"),
        ("2147483648", Num::Long(2_147_483_648), "2147483648"),
        (
            "9007199254740993.0",
            Num::Long(9_007_199_254_740_993),
            "9007199254740993",
        ),
        (
            "-9223372036854775808",
            Num::Long(i64::MIN),
            "-9223372036854775808",
        ),
        ("1e-400", Num::Float(0.0), "0.0"),
        ("1.0000000000000000001", Num::Float(1.0), "1.0"),
        ("1.5", Num::Float(1.5), "1.5"),
        ("0.1", Num::Double(0.1), "0.1"),
        ("1e20", Num::Double(1e20), "1.0E20"),
    ];

    for (raw, expected_num, expected_display) in cases {
        let json = format!(r#"{{"translate":"%s","with":[{raw}]}}"#);
        let c = from_json(&json);
        let Content::Translatable(tr) = &c.content else {
            panic!("expected translatable");
        };
        assert_eq!(tr.args.len(), 1);
        assert_eq!(
            tr.args[0],
            TranslationArg::Num(expected_num.clone()),
            "{raw}"
        );
        assert_eq!(
            flatten_plain(&c, &EmptyLanguage, None),
            *expected_display,
            "{raw}"
        );
    }

    // -0.0 encodes to NBT as TAG_Byte(0).
    let neg_zero = from_json(r#"{"translate":"%s","with":[-0.0]}"#);
    let tag = encode_component_nbt(&neg_zero).demand().unwrap();
    let with_tag = tag.as_compound().unwrap().get("with").unwrap();
    assert_eq!(with_tag.as_list().unwrap().as_slice(), &[Tag::Byte(0)]);
}

#[test]
fn nbt_path_empty_and_spaces() {
    // Empty path is valid (zero nodes) and selects the root tag itself.
    let empty = NbtPath::parse("").unwrap();
    assert!(empty.nodes().is_empty());
    let mut root = Compound::new();
    root.insert("k", Tag::Int(7));
    let root_tag = Tag::Compound(root);
    assert_eq!(empty.get(&root_tag), vec![root_tag]);

    let c = from_json(r#"{"nbt":"","entity":"@s"}"#);
    assert!(matches!(c.content, Content::Nbt(NbtContent { ref path, .. }) if path.is_empty()));

    // Spaces outside quotes terminate parsing and produce a trailing-data error.
    for bad in [" a", "a b", "a "] {
        assert!(NbtPath::parse(bad).is_err(), "{bad:?}");
        let j = format!(r#"{{"nbt":"{bad}","entity":"@s"}}"#);
        assert!(
            decode_component_str_json(&j, &NoRegistries)
                .demand()
                .is_err(),
            "{bad:?}"
        );
    }

    // Spaces inside quoted keys remain valid.
    assert!(NbtPath::parse("a.'b c'").is_ok());
}

#[test]
fn encoded_key_order_dispatched_and_atlas() {
    let ce = from_json(
        r#"{"text":"a","click_event":{"action":"open_url","url":"https://example.com"}}"#,
    );
    assert_eq!(
        to_json_str(&ce),
        r#"{"text":"a","click_event":{"url":"https://example.com","action":"open_url"}}"#
    );

    let he = from_json(r#"{"text":"a","hover_event":{"action":"show_text","value":"tip"}}"#);
    assert_eq!(
        to_json_str(&he),
        r#"{"text":"a","hover_event":{"value":"tip","action":"show_text"}}"#
    );

    let atlas = from_json(r#"{"sprite":"item/apple","atlas":"minecraft:items"}"#);
    assert_eq!(
        to_json_str(&atlas),
        r#"{"atlas":"minecraft:items","sprite":"minecraft:item/apple"}"#
    );
}

#[test]
fn registry_context_checks_present_missing_unavailable() {
    struct TestRegistries {
        regs: HashMap<String, HashSet<String>>,
    }
    impl RegistryContext for TestRegistries {
        fn lookup(&self, registry: &Identifier, id: &Identifier) -> RegistryLookup {
            match self.regs.get(&registry.to_string()) {
                None => RegistryLookup::Unavailable,
                Some(entries) => {
                    if entries.contains(&id.to_string()) {
                        RegistryLookup::Present
                    } else {
                        RegistryLookup::Missing
                    }
                }
            }
        }
    }

    let mut regs: HashMap<String, HashSet<String>> = HashMap::new();
    regs.insert(
        "minecraft:item".to_owned(),
        [
            "minecraft:stone",
            "minecraft:air",
            "minecraft:diamond_sword",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    );
    regs.insert(
        "minecraft:entity_type".to_owned(),
        ["minecraft:pig"].into_iter().map(str::to_owned).collect(),
    );
    regs.insert(
        "minecraft:dialog".to_owned(),
        ["minecraft:server_links"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    );
    regs.insert(
        "minecraft:enchantment".to_owned(),
        ["minecraft:sharpness"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    );
    let ctx = TestRegistries { regs };

    // show_item: Present passes; Missing, minecraft:air, and Unavailable fail.
    let stone_hover =
        r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone","count":5}}"#;
    let decoded_stone = decode_component_str_json(stone_hover, &ctx)
        .demand()
        .unwrap();
    assert_eq!(
        to_json_str(&decoded_stone),
        r#"{"text":"a","hover_event":{"id":"minecraft:stone","count":5,"action":"show_item"}}"#
    );
    assert!(
        decode_component_str_json(stone_hover, &NoRegistries)
            .demand()
            .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:not_an_item"}}"#,
            &ctx
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:air"}}"#,
            &ctx
        )
        .demand()
        .is_err()
    );

    // show_item with enchantments: Present passes; Missing fails.
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:diamond_sword","components":{"minecraft:enchantments":{"minecraft:sharpness":2}}}}"#,
            &ctx
        )
        .demand()
        .is_ok()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:diamond_sword","components":{"minecraft:enchantments":{"minecraft:not_real":2}}}}"#,
            &ctx
        )
        .demand()
        .is_err()
    );

    // show_entity: Present passes; Missing and Unavailable fail.
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[1,2,3,4]}}"#,
            &ctx
        )
        .demand()
        .is_ok()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:nope","uuid":[1,2,3,4]}}"#,
            &ctx
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[1,2,3,4]}}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );

    // show_dialog by id: Present passes; Missing and Unavailable fail; inline succeeds even with NoRegistries.
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"minecraft:server_links"}}"#,
            &ctx
        )
        .demand()
        .is_ok()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"minecraft:not_a_dialog"}}"#,
            &ctx
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"minecraft:server_links"}}"#,
            &NoRegistries
        )
        .demand()
        .is_err()
    );
    assert!(
        decode_component_str_json(
            r#"{"text":"a","click_event":{"action":"show_dialog","dialog":{"type":"minecraft:notice","title":"T"}}}"#,
            &NoRegistries
        )
        .demand()
        .is_ok()
    );
}
