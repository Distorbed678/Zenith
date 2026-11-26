//! JSON decoding of `minecraft:tree` configurations and tree decorators
//! (`docs/specs/worldgen/features-trees.md` §1, §3–§7).

use zenith_blocks::Direction;
use zenith_codec::json::{Json, JsonObject};

use super::{
    Resolver, as_arr, as_obj, opt_bool, opt_i32, parse_direction, parse_id, req_f32, req_field,
    req_i32, req_type_path,
};
use crate::features::tree::{
    AboveRootPlacement, FeatureSize, FeatureSizeKind, FoliageKind, FoliagePlacer,
    MangroveRootPlacement, RootPlacer, TreeConfig, TreeDecorator, TrunkKind, TrunkPlacer,
};
use crate::providers::IntProvider;

impl Resolver<'_> {
    /// Decodes a `minecraft:tree` configuration object.
    pub(super) fn decode_tree_config(&mut self, cfg: &JsonObject) -> Result<TreeConfig, String> {
        let trunk_provider = self.decode_bsp_holder(req_field(cfg, "trunk_provider")?)?;
        let trunk_placer = self.decode_trunk_placer(req_field(cfg, "trunk_placer")?)?;
        let foliage_provider = self.decode_bsp_holder(req_field(cfg, "foliage_provider")?)?;
        let foliage_placer = self.decode_foliage_placer(req_field(cfg, "foliage_placer")?)?;
        let root_placer = match cfg.get("root_placer") {
            None | Some(Json::Null) => None,
            Some(j) => Some(self.decode_root_placer(j)?),
        };
        let minimum_size = decode_feature_size(req_field(cfg, "minimum_size")?)?;
        let decorators = self.decode_tree_decorators(req_field(cfg, "decorators")?)?;
        let ignore_vines = opt_bool(cfg, "ignore_vines")?.unwrap_or(false);
        let below_trunk_provider =
            self.decode_bsp_holder(req_field(cfg, "below_trunk_provider")?)?;
        Ok(TreeConfig {
            trunk_provider,
            trunk_placer,
            foliage_provider,
            foliage_placer,
            root_placer,
            minimum_size,
            decorators,
            ignore_vines,
            below_trunk_provider,
        })
    }

    /// Decodes an array of tree decorators.
    pub(super) fn decode_tree_decorators(
        &mut self,
        json: &Json,
    ) -> Result<Vec<TreeDecorator>, String> {
        let mut out = Vec::new();
        for item in as_arr(json)? {
            out.push(self.decode_tree_decorator(item)?);
        }
        Ok(out)
    }

    fn decode_trunk_placer(&self, json: &Json) -> Result<TrunkPlacer, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        let kind = match ty.as_str() {
            "straight_trunk_placer" => TrunkKind::Straight,
            "forking_trunk_placer" => TrunkKind::Forking,
            "giant_trunk_placer" => TrunkKind::Giant,
            "mega_jungle_trunk_placer" => TrunkKind::MegaJungle,
            "dark_oak_trunk_placer" => TrunkKind::DarkOak,
            "fancy_trunk_placer" => TrunkKind::Fancy,
            "bending_trunk_placer" => TrunkKind::Bending {
                min_height_for_leaves: opt_i32(obj, "min_height_for_leaves")?.unwrap_or(1),
                bend_length: self.decode_int_provider(req_field(obj, "bend_length")?)?,
            },
            "upwards_branching_trunk_placer" => TrunkKind::UpwardsBranching {
                extra_branch_steps: self
                    .decode_int_provider(req_field(obj, "extra_branch_steps")?)?,
                place_branch_per_log_probability: req_f32(obj, "place_branch_per_log_probability")?,
                extra_branch_length: self
                    .decode_int_provider(req_field(obj, "extra_branch_length")?)?,
                can_grow_through: self
                    .decode_block_holder_set(req_field(obj, "can_grow_through")?)?,
            },
            "cherry_trunk_placer" => {
                let start = as_obj(req_field(obj, "branch_start_offset_from_top")?)?;
                TrunkKind::Cherry {
                    branch_count: self.decode_int_provider(req_field(obj, "branch_count")?)?,
                    branch_horizontal_length: self
                        .decode_int_provider(req_field(obj, "branch_horizontal_length")?)?,
                    branch_start_offset_from_top: (
                        req_i32(start, "min_inclusive")?,
                        req_i32(start, "max_inclusive")?,
                    ),
                    branch_end_offset_from_top: self
                        .decode_int_provider(req_field(obj, "branch_end_offset_from_top")?)?,
                }
            }
            "poplar_trunk_placer" => TrunkKind::Poplar {
                trunk_height_above_branches: self
                    .decode_int_provider(req_field(obj, "trunk_height_above_branches")?)?,
                branch_amount: self.decode_int_provider(req_field(obj, "branch_amount")?)?,
            },
            other => return Err(format!("unknown trunk_placer type '{other}'")),
        };
        Ok(TrunkPlacer {
            base_height: req_i32(obj, "base_height")?,
            height_rand_a: req_i32(obj, "height_rand_a")?,
            height_rand_b: req_i32(obj, "height_rand_b")?,
            kind,
        })
    }

    fn decode_foliage_placer(&self, json: &Json) -> Result<FoliagePlacer, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        let ip = |key: &str| -> Result<IntProvider, String> {
            self.decode_int_provider(req_field(obj, key)?)
        };
        let kind = match ty.as_str() {
            "blob_foliage_placer" => FoliageKind::Blob {
                height: req_i32(obj, "height")?,
            },
            "spruce_foliage_placer" => FoliageKind::Spruce {
                trunk_height: ip("trunk_height")?,
            },
            "pine_foliage_placer" => FoliageKind::Pine {
                height: ip("height")?,
            },
            "acacia_foliage_placer" => FoliageKind::Acacia,
            "bush_foliage_placer" => FoliageKind::Bush {
                height: req_i32(obj, "height")?,
            },
            "fancy_foliage_placer" => FoliageKind::Fancy {
                height: req_i32(obj, "height")?,
            },
            "jungle_foliage_placer" | "mega_jungle_foliage_placer" => FoliageKind::MegaJungle {
                height: req_i32(obj, "height")?,
            },
            "mega_pine_foliage_placer" => FoliageKind::MegaPine {
                crown_height: ip("crown_height")?,
            },
            "dark_oak_foliage_placer" => FoliageKind::DarkOak,
            "random_spread_foliage_placer" => FoliageKind::RandomSpread {
                foliage_height: ip("foliage_height")?,
                leaf_placement_attempts: req_i32(obj, "leaf_placement_attempts")?,
            },
            "cherry_foliage_placer" => FoliageKind::Cherry {
                height: ip("height")?,
                wide_bottom_layer_hole_chance: req_f32(obj, "wide_bottom_layer_hole_chance")?,
                corner_hole_chance: req_f32(obj, "corner_hole_chance")?,
                hanging_leaves_chance: req_f32(obj, "hanging_leaves_chance")?,
                hanging_leaves_extension_chance: req_f32(obj, "hanging_leaves_extension_chance")?,
            },
            "poplar_foliage_placer" => FoliageKind::Poplar {
                height: ip("height")?,
                side_hole_chance: req_f32(obj, "side_hole_chance")?,
            },
            other => return Err(format!("unknown foliage_placer type '{other}'")),
        };
        Ok(FoliagePlacer {
            radius: ip("radius")?,
            offset: ip("offset")?,
            kind,
        })
    }

    fn decode_root_placer(&mut self, json: &Json) -> Result<RootPlacer, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        if ty != "mangrove_root_placer" {
            return Err(format!("unknown root_placer type '{ty}'"));
        }
        let above_root = match obj.get("above_root_placement") {
            None | Some(Json::Null) => None,
            Some(j) => {
                let a = as_obj(j)?;
                Some(AboveRootPlacement {
                    provider: self.decode_bsp_holder(req_field(a, "above_root_provider")?)?,
                    chance: req_f32(a, "above_root_placement_chance")?,
                })
            }
        };
        let m = as_obj(req_field(obj, "mangrove_root_placement")?)?;
        let mangrove = MangroveRootPlacement {
            can_grow_through: self.decode_block_holder_set(req_field(m, "can_grow_through")?)?,
            muddy_roots_in: self.decode_block_holder_set(req_field(m, "muddy_roots_in")?)?,
            muddy_roots_provider: self.decode_bsp_holder(req_field(m, "muddy_roots_provider")?)?,
            max_root_width: req_i32(m, "max_root_width")?,
            max_root_length: req_i32(m, "max_root_length")?,
            random_skew_chance: req_f32(m, "random_skew_chance")?,
        };
        Ok(RootPlacer {
            trunk_offset_y: self.decode_int_provider(req_field(obj, "trunk_offset_y")?)?,
            root_provider: self.decode_bsp_holder(req_field(obj, "root_provider")?)?,
            above_root,
            mangrove,
        })
    }

    fn decode_tree_decorator(&mut self, json: &Json) -> Result<TreeDecorator, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        Ok(match ty.as_str() {
            "trunk_vine" => TreeDecorator::TrunkVine,
            "leave_vine" => TreeDecorator::LeaveVine {
                probability: req_f32(obj, "probability")?,
            },
            "pale_moss" => {
                let id = parse_id("minecraft:pale_moss_patch")?;
                let patch_feature = if self.feat_raw.contains_key(&id) {
                    Some(self.resolve_feature(&id).map_err(|e| e.to_string())?)
                } else {
                    None
                };
                TreeDecorator::PaleMoss {
                    leaves_probability: req_f32(obj, "leaves_probability")?,
                    trunk_probability: req_f32(obj, "trunk_probability")?,
                    ground_probability: req_f32(obj, "ground_probability")?,
                    patch_feature,
                }
            }
            "creaking_heart" => TreeDecorator::CreakingHeart {
                probability: req_f32(obj, "probability")?,
            },
            "cocoa" => TreeDecorator::Cocoa {
                probability: req_f32(obj, "probability")?,
            },
            "shelf_mushroom" => TreeDecorator::ShelfMushroom {
                probability: req_f32(obj, "probability")?,
            },
            "beehive" => TreeDecorator::Beehive {
                probability: req_f32(obj, "probability")?,
            },
            "alter_ground" => TreeDecorator::AlterGround {
                provider: self.decode_bsp_holder(req_field(obj, "provider")?)?,
            },
            "attached_to_leaves" => TreeDecorator::AttachedToLeaves {
                probability: req_f32(obj, "probability")?,
                exclusion_radius_xz: req_i32(obj, "exclusion_radius_xz")?,
                exclusion_radius_y: req_i32(obj, "exclusion_radius_y")?,
                block_provider: self.decode_bsp_holder(req_field(obj, "block_provider")?)?,
                required_empty_blocks: req_i32(obj, "required_empty_blocks")?,
                directions: decode_directions(req_field(obj, "directions")?)?,
            },
            "place_on_ground" => TreeDecorator::PlaceOnGround {
                tries: opt_i32(obj, "tries")?.unwrap_or(128),
                radius: opt_i32(obj, "radius")?.unwrap_or(2),
                height: opt_i32(obj, "height")?.unwrap_or(1),
                block_state_provider: self
                    .decode_bsp_holder(req_field(obj, "block_state_provider")?)?,
            },
            "attached_to_logs" => TreeDecorator::AttachedToLogs {
                probability: req_f32(obj, "probability")?,
                block_provider: self.decode_bsp_holder(req_field(obj, "block_provider")?)?,
                directions: decode_directions(req_field(obj, "directions")?)?,
            },
            other => return Err(format!("unknown tree_decorator type '{other}'")),
        })
    }
}

fn decode_directions(json: &Json) -> Result<Vec<Direction>, String> {
    let mut out = Vec::new();
    for item in as_arr(json)? {
        let s = item
            .as_str()
            .ok_or_else(|| "direction must be a string".to_owned())?;
        out.push(parse_direction(s)?);
    }
    if out.is_empty() {
        return Err("directions must not be empty".to_owned());
    }
    Ok(out)
}

fn decode_feature_size(json: &Json) -> Result<FeatureSize, String> {
    let obj = as_obj(json)?;
    let ty = req_type_path(obj)?;
    let min_clipped_height = opt_i32(obj, "min_clipped_height")?;
    let kind = match ty.as_str() {
        "two_layers_feature_size" => FeatureSizeKind::TwoLayers {
            limit: opt_i32(obj, "limit")?.unwrap_or(1),
            lower_size: opt_i32(obj, "lower_size")?.unwrap_or(0),
            upper_size: opt_i32(obj, "upper_size")?.unwrap_or(1),
        },
        "three_layers_feature_size" => FeatureSizeKind::ThreeLayers {
            limit: opt_i32(obj, "limit")?.unwrap_or(1),
            upper_limit: opt_i32(obj, "upper_limit")?.unwrap_or(1),
            lower_size: opt_i32(obj, "lower_size")?.unwrap_or(0),
            middle_size: opt_i32(obj, "middle_size")?.unwrap_or(1),
            upper_size: opt_i32(obj, "upper_size")?.unwrap_or(1),
        },
        other => return Err(format!("unknown feature_size type '{other}'")),
    };
    Ok(FeatureSize {
        min_clipped_height,
        kind,
    })
}
