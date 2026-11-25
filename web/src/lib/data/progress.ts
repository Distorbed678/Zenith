export type MilestoneStatus = 'planned' | 'in-progress' | 'done';

export interface ProgressOverall {
  percent: number;
  parityPercent: number;
  milestonesDone: number;
  milestonesTotal: number;
  suitesPassing: number;
  suitesTotal: number;
}

export interface ProgressPhase {
  id: string;
  title: string;
  blurb: string;
  percent: number;
  weight: number;
  milestones: string[];
}

export interface ProgressTask {
  title: string;
  done: boolean;
}

export interface ProgressSuite {
  id: string;
  passed: number;
  total: number;
  percent: number;
  updatedAt: string | null;
}

export interface ProgressMilestone {
  id: string;
  phase: string;
  title: string;
  status: MilestoneStatus;
  weight: number;
  percent: number;
  tasksDone: number;
  tasksTotal: number;
  tasks: ProgressTask[];
  suites: ProgressSuite[];
}

export interface ProgressHistoryPoint {
  date: string;
  percent: number;
  parityPercent: number;
}

export interface ProgressData {
  schemaVersion: number;
  generatedAt: string;
  project: string;
  targetVersion: string;
  protocol: number;
  worldVersion: number;
  overall: ProgressOverall;
  phases: ProgressPhase[];
  milestones: ProgressMilestone[];
  history: ProgressHistoryPoint[];
}

const FALLBACK_PROGRESS: ProgressData = {
  schemaVersion: 1,
  generatedAt: '2026-11-24T18:40:00Z',
  project: 'Zenith',
  targetVersion: '26.3',
  protocol: 777,
  worldVersion: 5023,
  overall: {
    percent: 1.4,
    parityPercent: 0.0,
    milestonesDone: 0,
    milestonesTotal: 33,
    suitesPassing: 0,
    suitesTotal: 40
  },
  phases: [
    {
      id: 'P0',
      title: 'Foundation',
      blurb: 'Repo, rules, plan, hygiene guard, ownership proof, tracker, website.',
      percent: 70.0,
      weight: 2,
      milestones: ['M0']
    },
    {
      id: 'P1',
      title: 'Core data',
      blurb: 'Parity harness, NBT, saves, registries, codecs, data packs. All headless.',
      percent: 0.0,
      weight: 17,
      milestones: ['M1', 'M2', 'M3', 'M4', 'M5']
    },
    {
      id: 'P2',
      title: 'World simulation',
      blurb: 'RNG, noise, worldgen, lighting, ticking, entities, items, commands. Headless server.',
      percent: 0.0,
      weight: 58,
      milestones: ['M6', 'M7', 'M8', 'M9', 'M10', 'M11', 'M12', 'M13', 'M14', 'M15']
    },
    {
      id: 'P3',
      title: 'Networking',
      blurb: 'Protocol 777 end to end; the Zenith dedicated server accepts vanilla clients.',
      percent: 0.0,
      weight: 6,
      milestones: ['M16']
    },
    {
      id: 'P4',
      title: 'Client',
      blurb: 'Vulkan renderer, resources, GUI, audio, singleplayer. Pixel-exact.',
      percent: 0.0,
      weight: 31,
      milestones: ['M17', 'M18', 'M19', 'M20', 'M21', 'M22']
    },
    {
      id: 'P5',
      title: 'Platform',
      blurb: 'Microsoft sign-in, WASM mods, mod index, modpacks, launcher, in-game mod manager.',
      percent: 0.0,
      weight: 18,
      milestones: ['M23', 'M24', 'M25', 'M26', 'M27']
    },
    {
      id: 'P6',
      title: 'Speed & release',
      blurb: 'Performance program and the 1.0 release for 26.3.',
      percent: 0.0,
      weight: 6,
      milestones: ['M28', 'M29']
    },
    {
      id: 'P7',
      title: 'More versions',
      blurb: 'Next-release port via the version diff bot, older versions, research.',
      percent: 0.0,
      weight: 10,
      milestones: ['M30', 'M31', 'R1']
    }
  ],
  milestones: [
    {
      id: 'M0',
      phase: 'P0',
      title: 'Foundation & tooling',
      status: 'in-progress',
      weight: 2,
      percent: 70.0,
      tasksDone: 7,
      tasksTotal: 10,
      tasks: [
        { title: 'Owner Q&A rounds logged (docs/decisions/USER-ANSWERS.md)', done: true },
        { title: 'Master plan written (docs/plan/00–13)', done: true },
        { title: 'ADRs for the founding decisions', done: true },
        { title: 'Cargo workspace + pinned toolchain', done: true },
        { title: 'zenith-install: locate installs + local ownership proof', done: true },
        { title: 'Asset/trademark/decompile guard (xtask + pre-commit + CI)', done: true },
        { title: 'Tracker pipeline (milestones -> progress.json -> SVG)', done: true },
        { title: 'Pixel-crafted design system (packages/ui)', done: false },
        { title: 'Website v1 on GitHub Pages', done: false },
        { title: 'Public GitHub repo + CI', done: false }
      ],
      suites: []
    },
    {
      id: 'M1',
      phase: 'P1',
      title: 'Parity harness & data-gen ingestion',
      status: 'planned',
      weight: 4,
      percent: 0.0,
      tasksDone: 0,
      tasksTotal: 6,
      tasks: [
        { title: 'Java runtime discovery (launcher-bundled Java 25)', done: false },
        { title: 'Run vanilla data generator on the user jar', done: false },
        { title: 'Compile reports into versioned binary tables (zenith-data)', done: false },
        { title: 'Parity oracle Java agent skeleton (local-only, tools/oracle)', done: false },
        { title: 'State-dump format + differ (zenith-parity)', done: false },
        { title: 'Parity results into progress/parity/*.json and tracker', done: false }
      ],
      suites: [{ id: 'datagen-ingest', passed: 0, total: 0, percent: 0.0, updatedAt: null }]
    },
    {
      id: 'M2',
      phase: 'P1',
      title: 'NBT & SNBT',
      status: 'planned',
      weight: 2,
      percent: 0.0,
      tasksDone: 0,
      tasksTotal: 5,
      tasks: [
        { title: 'Binary NBT read/write (file + network variants)', done: false },
        { title: 'Modified UTF-8 + depth/size limits identical to vanilla', done: false },
        { title: 'SNBT parser + printer (vanilla formatting)', done: false },
        { title: 'Fuzz targets (cargo-fuzz)', done: false },
        { title: 'Round-trip every NBT blob in vanilla-generated worlds', done: false }
      ],
      suites: [
        { id: 'nbt-roundtrip', passed: 0, total: 0, percent: 0.0, updatedAt: null },
        { id: 'snbt-format', passed: 0, total: 0, percent: 0.0, updatedAt: null }
      ]
    }
  ],
  history: [{ date: '2026-11-24', percent: 1.4, parityPercent: 0.0 }]
};

const progressModules = import.meta.glob('../../../../progress/progress.json', {
  eager: true,
  import: 'default'
}) as Record<string, ProgressData>;

const loadedEntry = Object.values(progressModules)[0];

export const isFallbackProgress: boolean = !loadedEntry;
export const progressData: ProgressData = loadedEntry ?? FALLBACK_PROGRESS;
