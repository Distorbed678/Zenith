//! Block-state table verification against synthetic `Tables` and (when cached) the local
//! 26.3 `tables.bin` cache.

use zenith_data::{Block, Property, Registry, RegistryEntry, StateRow, Tables, TablesMeta};
use zenith_registry::BlockStateTable;

#[test]
fn block_state_table_verifies_synthetic_tables() {
    // Build a synthetic Tables where block properties in `Tables::blocks` are stored in
    // reversed order and boolean values are reversed, while `Tables::states` uses the true
    // vanilla odometer order (alphabetical property names, true before false, last fastest).
    let tables = Tables {
        meta: TablesMeta {
            version: "synthetic".to_owned(),
            jar_sha1: "0".repeat(40),
            world_version: 0,
            protocol: 0,
            ingested_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        blocks: vec![
            Block {
                name: "minecraft:air".to_owned(),
                properties: vec![],
                first_state: 0,
                state_count: 1,
                default_state: 0,
            },
            Block {
                name: "minecraft:lever".to_owned(),
                // Intentionally unsorted property order and reversed boolean order to prove
                // BlockStateTable::from_tables computes ordering from the spec rules.
                properties: vec![
                    Property {
                        name: "powered".to_owned(),
                        values: vec!["false".to_owned(), "true".to_owned()],
                    },
                    Property {
                        name: "facing".to_owned(),
                        values: vec!["north".to_owned(), "south".to_owned()],
                    },
                ],
                first_state: 1,
                state_count: 4,
                default_state: 2,
            },
        ],
        // True spec order: facing (north, south) outer, powered (true, false) inner.
        // Note: in `blocks[1].properties`, index 0 is `powered` (0="false", 1="true"),
        // index 1 is `facing` (0="north", 1="south").
        states: vec![
            StateRow {
                block: 0,
                values: vec![],
            },
            // id 1: facing=north, powered=true
            StateRow {
                block: 1,
                values: vec![1, 0],
            },
            // id 2: facing=north, powered=false (default)
            StateRow {
                block: 1,
                values: vec![0, 0],
            },
            // id 3: facing=south, powered=true
            StateRow {
                block: 1,
                values: vec![1, 1],
            },
            // id 4: facing=south, powered=false
            StateRow {
                block: 1,
                values: vec![0, 1],
            },
        ],
        registries: vec![Registry {
            name: "minecraft:block".to_owned(),
            protocol_id: 0,
            default: Some("minecraft:air".to_owned()),
            entries: vec![
                RegistryEntry {
                    name: "minecraft:air".to_owned(),
                    protocol_id: 0,
                },
                RegistryEntry {
                    name: "minecraft:lever".to_owned(),
                    protocol_id: 1,
                },
            ],
        }],
        packets: vec![],
        commands_json: "{}".to_owned(),
    };

    let computed = BlockStateTable::from_tables(&tables).unwrap();
    let (matched, total) = computed.verify_against(&tables);
    assert_eq!(total, 5);
    assert_eq!(matched, 5, "diffs: {:?}", computed.diff_against(&tables, 5));
    assert_eq!(
        computed
            .block(&"minecraft:lever".parse().unwrap())
            .unwrap()
            .default_state,
        2
    );
}

#[test]
fn block_state_ids_match_cached_26_3_tables() {
    let tables_path = zenith_data::version_dir("26.3").join("tables.bin");
    if !tables_path.exists() {
        eprintln!(
            "skipping block_state_ids_match_cached_26_3_tables: {} not present",
            tables_path.display()
        );
        return;
    }

    let tables = Tables::load(&tables_path).expect("load cached 26.3 tables.bin");
    let computed = BlockStateTable::from_tables(&tables).expect("compute BlockStateTable");
    let (matched, total) = computed.verify_against(&tables);
    eprintln!("verified cached 26.3 block-state IDs: {matched}/{total}");
    assert_eq!(
        matched,
        total,
        "block-state ID mismatch ({matched}/{total}); first diffs: {:?}",
        computed.diff_against(&tables, 10)
    );
    assert!(total > 0, "expected non-empty block-state table");
    assert_eq!(
        computed.state_by_id(0).unwrap().block_id.to_string(),
        "minecraft:air"
    );
}
