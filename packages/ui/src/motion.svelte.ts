const STORAGE_KEY = 'zenith:motion-paused';

function readStoredPause(): boolean {
  if (typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function writeStoredPause(paused: boolean): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(STORAGE_KEY, paused ? '1' : '0');
  } catch {
    // Ignore storage errors in restricted environments
  }
}

function readSystemReduced(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false;
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch {
    return false;
  }
}

class MotionStore {
  userPaused = $state(false);
  systemReduced = $state(false);
  #initialized = false;

  get paused(): boolean {
    return this.userPaused || this.systemReduced;
  }

  init(): void {
    if (this.#initialized || typeof window === 'undefined') return;
    this.#initialized = true;
    this.userPaused = readStoredPause();
    this.systemReduced = readSystemReduced();

    if (window.matchMedia) {
      const mql = window.matchMedia('(prefers-reduced-motion: reduce)');
      const onChange = (event: MediaQueryListEvent) => {
        this.systemReduced = event.matches;
      };
      mql.addEventListener?.('change', onChange);
    }
  }

  toggle(): void {
    this.init();
    this.setUserPaused(!this.userPaused);
  }

  setUserPaused(paused: boolean): void {
    this.userPaused = paused;
    writeStoredPause(paused);
  }
}

export const motionState = new MotionStore();
