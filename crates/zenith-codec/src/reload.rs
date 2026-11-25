//! The reload pipeline as data and hooks (spec §6). The registries themselves arrive in later milestones; this
//! module fixes the **order** of the stages for world load and `/reload`, which loader uses which lookup mode, and
//! which elements `/reload` skips.

use std::any::Any;
use std::fmt;

/// What is being run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadKind {
    /// Server start or opening a singleplayer world (spec §6.1).
    WorldLoad,
    /// The `/reload` command (spec §6.2).
    Reload,
}

/// The four registry layers in load order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegistryLayer {
    /// Built-in (static) registries: only their tags are data-driven.
    Static,
    /// The 51 world-layer registries (biomes, dimension types, enchantments, …).
    World,
    /// The dimension layer (`dimension`, the level stems).
    Dimension,
    /// Loot tables, predicates, context providers, item modifiers, slot sources, advancements, recipes.
    Reloadable,
}

impl RegistryLayer {
    /// Whether the layer's **elements** are (re)loaded by `kind`. `/reload` reloads reloadable elements only.
    pub fn reloads_elements(self, kind: ReloadKind) -> bool {
        match (kind, self) {
            (_, Self::Static) => false,
            (ReloadKind::WorldLoad, _) => true,
            (ReloadKind::Reload, layer) => layer == Self::Reloadable,
        }
    }

    /// Whether the layer's **tags** are (re)loaded by `kind`: always for every existing registry.
    pub fn reloads_tags(self, _kind: ReloadKind) -> bool {
        true
    }
}

/// How a loader walks the pack stack (spec §5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupMode {
    /// Directory listing, one winner per file (spec §5.6).
    ListingOneWinner,
    /// Directory listing, stacks of every version (spec §5.7).
    ListingStacks,
    /// Looked up on demand per id (structure templates).
    OnDemand,
}

/// A data consumer of the pack stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoaderKind {
    /// Dynamic registries: world, dimension and reloadable elements.
    DynamicRegistry,
    /// Tags of every registry, including function tags.
    Tags,
    /// `.mcfunction` files.
    Functions,
    /// Structure templates.
    StructureTemplates,
}

impl LoaderKind {
    /// The lookup mode this loader uses.
    pub fn lookup_mode(self) -> LookupMode {
        match self {
            Self::DynamicRegistry | Self::Functions => LookupMode::ListingOneWinner,
            Self::Tags => LookupMode::ListingStacks,
            Self::StructureTemplates => LookupMode::OnDemand,
        }
    }

    /// The location pattern under a namespace (`<registry path>` is the singular registry id).
    pub fn location(self) -> &'static str {
        match self {
            Self::DynamicRegistry => "data/<ns>/<registry path>/**.json",
            Self::Tags => "data/<ns>/tags/<registry path>/**.json",
            Self::Functions => "data/<ns>/function/**.mcfunction",
            Self::StructureTemplates => "data/<ns>/structure/**.nbt",
        }
    }

    /// The file extension the loader lists for.
    pub fn extension(self) -> &'static str {
        match self {
            Self::DynamicRegistry | Self::Tags => ".json",
            Self::Functions => ".mcfunction",
            Self::StructureTemplates => ".nbt",
        }
    }
}

/// One step of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// World load: configure the pack repository (spec §4.2).
    ConfigurePacks,
    /// `/reload`: pick the selection (spec §4.4).
    SelectPacks,
    /// Open the selected packs in order and build the stack.
    OpenPacksAndBuildStack,
    /// World load: tags of the static registries, held as *pending*.
    LoadStaticTags,
    /// World load: the world-layer registries (elements, own tags, freeze, validate).
    LoadWorldRegistries,
    /// World load: the dimension layer.
    LoadDimensions,
    /// World load: read or create the world data using the loaded dimensions.
    ReadWorldData,
    /// `/reload`: tags of **every** existing registry, held as pending.
    LoadAllTags,
    /// The reloadable layer.
    LoadReloadableRegistries,
    /// Loot validation (warnings; a fatal problem aborts).
    ValidateLootData,
    /// Build data-component initializers.
    BuildComponentInitializers,
    /// Build the command tree, the recipe manager and the advancement tree.
    BuildCommandsRecipesAdvancements,
    /// Run reload listeners (the function library).
    RunReloadListeners,
    /// World load: apply pending static tags and components on the main thread.
    ApplyPendingTagsAndComponents,
    /// `/reload` (main thread): close the old stack, swap in the new one.
    SwapStack,
    /// `/reload`: store the selection and configuration.
    StoreSelection,
    /// `/reload`: apply the pending tags and components.
    ApplyPendingReload,
    /// `/reload`: finalize recipes for the enabled features.
    FinalizeRecipes,
    /// `/reload`: save every player.
    SavePlayers,
    /// `/reload`: resend tags, recipes, advancements, commands… to connected players.
    ResendData,
    /// `/reload`: replace the function library.
    ReplaceFunctionLibrary,
    /// `/reload`: clear the structure-template cache.
    ClearStructureCache,
}

/// Ordered stage lists.
pub fn stages(kind: ReloadKind) -> &'static [Stage] {
    use Stage::*;
    match kind {
        ReloadKind::WorldLoad => &[
            ConfigurePacks,
            OpenPacksAndBuildStack,
            LoadStaticTags,
            LoadWorldRegistries,
            LoadDimensions,
            ReadWorldData,
            LoadReloadableRegistries,
            ValidateLootData,
            BuildComponentInitializers,
            BuildCommandsRecipesAdvancements,
            RunReloadListeners,
            ApplyPendingTagsAndComponents,
        ],
        ReloadKind::Reload => &[
            SelectPacks,
            OpenPacksAndBuildStack,
            LoadAllTags,
            LoadReloadableRegistries,
            ValidateLootData,
            BuildComponentInitializers,
            BuildCommandsRecipesAdvancements,
            RunReloadListeners,
            SwapStack,
            StoreSelection,
            ApplyPendingReload,
            FinalizeRecipes,
            SavePlayers,
            ResendData,
            ReplaceFunctionLibrary,
            ClearStructureCache,
        ],
    }
}

/// The loaders a stage uses (empty for stages that only move state around).
pub fn stage_loaders(stage: Stage) -> &'static [LoaderKind] {
    use Stage::*;
    match stage {
        LoadStaticTags | LoadAllTags => &[LoaderKind::Tags],
        LoadWorldRegistries | LoadDimensions | LoadReloadableRegistries => {
            &[LoaderKind::DynamicRegistry, LoaderKind::Tags]
        }
        RunReloadListeners => &[LoaderKind::Functions, LoaderKind::Tags],
        _ => &[],
    }
}

/// Messages a reload sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadMessage {
    /// "Reload success": sent **before** the reload runs.
    Success,
    /// "Reload failure": follows if the reload then fails.
    Failure,
}

/// A failed stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageFailure {
    /// Description (for registry failures: the grouped report).
    pub message: String,
}

impl fmt::Display for StageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// The host's implementation of the stages.
pub trait ReloadHooks {
    /// Runs one stage. An error aborts the whole pipeline.
    fn run_stage(&mut self, stage: Stage) -> Result<(), StageFailure>;

    /// Sends a `/reload` message to the command source.
    fn send_message(&mut self, _message: ReloadMessage) {}

    /// Called once after a failure: close the opened packs; for `/reload` the previous resources, selection and
    /// stored configuration stay in effect.
    fn on_failure(&mut self, _stage: Stage, _failure: &StageFailure) {}
}

/// How a pipeline run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOutcome {
    /// Stages that completed, in order.
    pub completed: Vec<Stage>,
    /// The failing stage, if any.
    pub failure: Option<(Stage, StageFailure)>,
}

impl PipelineOutcome {
    /// Whether every stage completed.
    pub fn is_success(&self) -> bool {
        self.failure.is_none()
    }
}

/// Runs the stages of `kind` in order. For `/reload` the success message is sent **first**, and a failure sends the
/// failure message afterwards.
pub fn run_pipeline(kind: ReloadKind, hooks: &mut dyn ReloadHooks) -> PipelineOutcome {
    if kind == ReloadKind::Reload {
        hooks.send_message(ReloadMessage::Success);
    }
    let mut completed = Vec::new();
    for &stage in stages(kind) {
        match hooks.run_stage(stage) {
            Ok(()) => completed.push(stage),
            Err(failure) => {
                hooks.on_failure(stage, &failure);
                if kind == ReloadKind::Reload {
                    hooks.send_message(ReloadMessage::Failure);
                }
                return PipelineOutcome {
                    completed,
                    failure: Some((stage, failure)),
                };
            }
        }
    }
    PipelineOutcome {
        completed,
        failure: None,
    }
}

// ----------------------------------------------------------------------------------------------------------
// Reload-listener scheduling (spec §6.3)
// ----------------------------------------------------------------------------------------------------------

/// A reload listener: prepares in parallel, applies in registration order.
pub trait ReloadListener: Send + Sync {
    /// A name for diagnostics.
    fn name(&self) -> &str;
    /// The preparation step (may run on any thread, concurrently with the other listeners' preparation).
    fn prepare(&self) -> Result<Box<dyn Any + Send>, String>;
    /// The apply step, with what `prepare` returned.
    fn apply(&mut self, prepared: Box<dyn Any + Send>) -> Result<(), String>;
}

/// Runs the listeners: every listener **prepares in parallel**; apply steps start only after *every* listener has
/// finished preparing, and run strictly in registration order. The first failure (in registration order) fails the
/// whole reload and later applies do not run.
pub fn run_listeners(listeners: &mut [Box<dyn ReloadListener>]) -> Result<(), String> {
    let prepared: Vec<Result<Box<dyn Any + Send>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = listeners
            .iter()
            .map(|l| {
                let l: &dyn ReloadListener = &**l;
                scope.spawn(move || l.prepare())
            })
            .collect();
        handles
            .into_iter()
            .zip(listeners.iter())
            .map(|(h, l)| {
                h.join().unwrap_or_else(|_| {
                    Err(format!("listener {} panicked while preparing", l.name()))
                })
            })
            .collect()
    });
    let mut ready = Vec::with_capacity(prepared.len());
    for (p, l) in prepared.into_iter().zip(listeners.iter()) {
        match p {
            Ok(v) => ready.push(v),
            Err(m) => return Err(format!("{}: {m}", l.name())),
        }
    }
    for (l, p) in listeners.iter_mut().zip(ready) {
        l.apply(p).map_err(|m| format!("{}: {m}", l.name()))?;
    }
    Ok(())
}

/// Function-file text handling: bytes are read as UTF-8 (malformed bytes become U+FFFD) and split into lines on
/// `\n`, `\r\n` or `\r`.
pub fn split_function_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => lines.push(std::mem::take(&mut cur)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}
