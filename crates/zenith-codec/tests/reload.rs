use std::any::Any;
use std::sync::{Arc, Mutex};

use zenith_codec::reload::*;

#[derive(Default)]
struct Recorder {
    events: Vec<String>,
    fail_at: Option<Stage>,
}

impl ReloadHooks for Recorder {
    fn run_stage(&mut self, stage: Stage) -> Result<(), StageFailure> {
        self.events.push(format!("{stage:?}"));
        if self.fail_at == Some(stage) {
            return Err(StageFailure {
                message: "boom".into(),
            });
        }
        Ok(())
    }
    fn send_message(&mut self, message: ReloadMessage) {
        self.events.push(format!("msg:{message:?}"));
    }
    fn on_failure(&mut self, stage: Stage, _failure: &StageFailure) {
        self.events.push(format!("failed:{stage:?}"));
    }
}

#[test]
fn world_load_stage_order() {
    use Stage::*;
    assert_eq!(
        stages(ReloadKind::WorldLoad),
        [
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
        ]
    );
    let mut r = Recorder::default();
    let out = run_pipeline(ReloadKind::WorldLoad, &mut r);
    assert!(out.is_success());
    assert_eq!(out.completed.len(), 12);
    assert_eq!(r.events.first().unwrap(), "ConfigurePacks");
}

#[test]
fn reload_stage_order_skips_world_and_dimension_elements() {
    use Stage::*;
    let s = stages(ReloadKind::Reload);
    assert_eq!(s[0], SelectPacks);
    assert!(
        !s.contains(&LoadWorldRegistries)
            && !s.contains(&LoadDimensions)
            && !s.contains(&ReadWorldData)
    );
    assert!(s.contains(&LoadAllTags) && s.contains(&LoadReloadableRegistries));
    // Tags are loaded before any element; the swap comes after the function library is prepared.
    let pos = |st: Stage| s.iter().position(|x| *x == st).unwrap();
    assert!(pos(LoadAllTags) < pos(LoadReloadableRegistries));
    assert!(pos(ValidateLootData) < pos(BuildComponentInitializers));
    assert!(pos(RunReloadListeners) < pos(SwapStack));
    assert!(pos(SwapStack) < pos(StoreSelection) && pos(StoreSelection) < pos(ApplyPendingReload));
    assert_eq!(*s.last().unwrap(), ClearStructureCache);
    // The static layer has no data-driven elements; /reload reloads reloadable elements only, all layers' tags.
    for kind in [ReloadKind::WorldLoad, ReloadKind::Reload] {
        assert!(!RegistryLayer::Static.reloads_elements(kind));
        assert!(RegistryLayer::Static.reloads_tags(kind));
        assert!(RegistryLayer::World.reloads_tags(kind));
    }
    assert!(RegistryLayer::World.reloads_elements(ReloadKind::WorldLoad));
    assert!(RegistryLayer::Dimension.reloads_elements(ReloadKind::WorldLoad));
    assert!(!RegistryLayer::World.reloads_elements(ReloadKind::Reload));
    assert!(!RegistryLayer::Dimension.reloads_elements(ReloadKind::Reload));
    assert!(RegistryLayer::Reloadable.reloads_elements(ReloadKind::Reload));
    assert!(RegistryLayer::Reloadable.reloads_elements(ReloadKind::WorldLoad));
}

#[test]
fn loaders_use_their_lookup_modes() {
    assert_eq!(
        LoaderKind::DynamicRegistry.lookup_mode(),
        LookupMode::ListingOneWinner
    );
    assert_eq!(
        LoaderKind::Functions.lookup_mode(),
        LookupMode::ListingOneWinner
    );
    assert_eq!(LoaderKind::Tags.lookup_mode(), LookupMode::ListingStacks);
    assert_eq!(
        LoaderKind::StructureTemplates.lookup_mode(),
        LookupMode::OnDemand
    );
    assert_eq!(
        LoaderKind::Tags.location(),
        "data/<ns>/tags/<registry path>/**.json"
    );
    assert_eq!(LoaderKind::Functions.extension(), ".mcfunction");
    assert!(stage_loaders(Stage::LoadStaticTags).contains(&LoaderKind::Tags));
    assert!(stage_loaders(Stage::LoadWorldRegistries).contains(&LoaderKind::DynamicRegistry));
    assert!(stage_loaders(Stage::RunReloadListeners).contains(&LoaderKind::Functions));
    assert!(stage_loaders(Stage::SwapStack).is_empty());
}

#[test]
fn any_failure_aborts_the_pipeline_and_runs_the_failure_hook() {
    let mut r = Recorder {
        fail_at: Some(Stage::LoadDimensions),
        ..Default::default()
    };
    let out = run_pipeline(ReloadKind::WorldLoad, &mut r);
    assert!(!out.is_success());
    assert_eq!(out.failure.as_ref().unwrap().0, Stage::LoadDimensions);
    assert_eq!(out.completed.last(), Some(&Stage::LoadWorldRegistries));
    assert_eq!(r.events.last().unwrap(), "failed:LoadDimensions");
    assert!(!r.events.iter().any(|e| e == "ReadWorldData"));
}

#[test]
fn reload_sends_success_before_running_and_failure_after_a_failed_reload() {
    let mut ok = Recorder::default();
    assert!(run_pipeline(ReloadKind::Reload, &mut ok).is_success());
    assert_eq!(ok.events[0], "msg:Success");
    assert_eq!(ok.events[1], "SelectPacks");
    assert!(!ok.events.iter().any(|e| e == "msg:Failure"));

    let mut bad = Recorder {
        fail_at: Some(Stage::LoadReloadableRegistries),
        ..Default::default()
    };
    let out = run_pipeline(ReloadKind::Reload, &mut bad);
    assert!(!out.is_success());
    assert_eq!(bad.events[0], "msg:Success");
    let n = bad.events.len();
    assert_eq!(bad.events[n - 2], "failed:LoadReloadableRegistries");
    assert_eq!(bad.events[n - 1], "msg:Failure");
    // The swap/store stages never ran: the old state stays.
    assert!(
        !bad.events
            .iter()
            .any(|e| e == "SwapStack" || e == "StoreSelection")
    );
}

struct Listener {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
    fail_prepare: bool,
    fail_apply: bool,
    barrier: Arc<std::sync::Barrier>,
}

impl ReloadListener for Listener {
    fn name(&self) -> &str {
        self.name
    }
    fn prepare(&self) -> Result<Box<dyn Any + Send>, String> {
        // Every listener must be able to prepare concurrently, or this barrier would deadlock.
        self.barrier.wait();
        self.log
            .lock()
            .unwrap()
            .push(format!("prepared:{}", self.name));
        if self.fail_prepare {
            return Err("prepare failed".into());
        }
        Ok(Box::new(self.name.to_owned()))
    }
    fn apply(&mut self, prepared: Box<dyn Any + Send>) -> Result<(), String> {
        let name = *prepared.downcast::<String>().unwrap();
        let prepared_count = self
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.starts_with("prepared:"))
            .count();
        self.log
            .lock()
            .unwrap()
            .push(format!("apply:{name}:after_{prepared_count}_prepared"));
        if self.fail_apply {
            Err("apply failed".into())
        } else {
            Ok(())
        }
    }
}

type Listeners = (Vec<Box<dyn ReloadListener>>, Arc<Mutex<Vec<String>>>);

fn listeners(n: usize, fail_prepare: Option<usize>, fail_apply: Option<usize>) -> Listeners {
    let log = Arc::new(Mutex::new(Vec::new()));
    let barrier = Arc::new(std::sync::Barrier::new(n));
    let names = ["a", "b", "c"];
    let v = (0..n)
        .map(|i| {
            Box::new(Listener {
                name: names[i],
                log: log.clone(),
                fail_prepare: fail_prepare == Some(i),
                fail_apply: fail_apply == Some(i),
                barrier: barrier.clone(),
            }) as Box<dyn ReloadListener>
        })
        .collect();
    (v, log)
}

#[test]
fn listeners_prepare_in_parallel_and_apply_in_registration_order() {
    let (mut ls, log) = listeners(3, None, None);
    run_listeners(&mut ls).unwrap();
    let events = log.lock().unwrap().clone();
    let applies: Vec<&String> = events.iter().filter(|e| e.starts_with("apply:")).collect();
    // Applies start only after *every* listener finished preparing, strictly in registration order.
    assert_eq!(
        applies,
        [
            "apply:a:after_3_prepared",
            "apply:b:after_3_prepared",
            "apply:c:after_3_prepared"
        ]
    );
}

#[test]
fn first_listener_failure_fails_the_whole_reload() {
    let (mut ls, log) = listeners(3, Some(1), None);
    let err = run_listeners(&mut ls).unwrap_err();
    assert!(err.starts_with("b:"), "{err}");
    assert!(!log.lock().unwrap().iter().any(|e| e.starts_with("apply:")));
    let (mut ls, log) = listeners(3, None, Some(1));
    let err = run_listeners(&mut ls).unwrap_err();
    assert!(err.starts_with("b:"), "{err}");
    let events = log.lock().unwrap().clone();
    assert!(events.iter().any(|e| e.starts_with("apply:a")));
    assert!(events.iter().any(|e| e.starts_with("apply:b")));
    assert!(
        !events.iter().any(|e| e.starts_with("apply:c")),
        "{events:?}"
    );
}

#[test]
fn function_files_split_on_all_line_endings_and_replace_bad_bytes() {
    assert_eq!(split_function_lines(b"a\nb\r\nc\rd"), ["a", "b", "c", "d"]);
    assert_eq!(split_function_lines(b"a\n\nb\n"), ["a", "", "b"]);
    assert_eq!(split_function_lines(b"x\xffy"), ["x\u{fffd}y"]);
    assert!(split_function_lines(b"").is_empty());
}
