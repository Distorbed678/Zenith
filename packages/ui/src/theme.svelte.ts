export type ZenithTheme = 'night' | 'day';

const STORAGE_KEY = 'zenith:theme';

function readStoredTheme(): ZenithTheme {
  if (typeof window === 'undefined') return 'night';
  try {
    const val = window.localStorage.getItem(STORAGE_KEY);
    return val === 'day' ? 'day' : 'night';
  } catch {
    return 'night';
  }
}

function applyTheme(theme: ZenithTheme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.theme = theme;
}

class ThemeStore {
  current = $state<ZenithTheme>('night');
  #initialized = false;

  init(): void {
    if (this.#initialized || typeof window === 'undefined') return;
    this.#initialized = true;
    this.current = readStoredTheme();
    applyTheme(this.current);
  }

  set(theme: ZenithTheme): void {
    this.current = theme;
    applyTheme(theme);
    if (typeof window !== 'undefined') {
      try {
        window.localStorage.setItem(STORAGE_KEY, theme);
      } catch {
        // Ignore storage errors
      }
    }
  }

  toggle(): void {
    this.init();
    this.set(this.current === 'night' ? 'day' : 'night');
  }
}

export const themeState = new ThemeStore();
