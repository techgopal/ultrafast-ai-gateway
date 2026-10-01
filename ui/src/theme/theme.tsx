import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { Button } from "@/components/ui/button";

export type Theme = "light" | "dark" | "system";

/** The same key `public/theme.js` reads before the first paint. */
export const THEME_STORAGE_KEY = "uf-theme";

const DEVICE_PREFERS_DARK = "(prefers-color-scheme: dark)";

interface ThemeContextValue {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readSavedTheme(): Theme {
  try {
    const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  } catch {
    // Storage is blocked: follow the device.
    return "system";
  }
}

function saveTheme(theme: Theme): void {
  try {
    if (theme === "system") window.localStorage.removeItem(THEME_STORAGE_KEY);
    else window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Storage is blocked: the choice lasts for this visit only.
  }
}

function applyTheme(dark: boolean): void {
  const root = document.documentElement;
  root.classList.toggle("dark", dark);
  root.style.colorScheme = dark ? "dark" : "light";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readSavedTheme);

  useEffect(() => {
    if (theme !== "system") {
      applyTheme(theme === "dark");
      return;
    }
    const device = window.matchMedia(DEVICE_PREFERS_DARK);
    const follow = () => {
      applyTheme(device.matches);
    };
    follow();
    device.addEventListener("change", follow);
    return () => {
      device.removeEventListener("change", follow);
    };
  }, [theme]);

  const setTheme = useCallback((next: Theme) => {
    saveTheme(next);
    setThemeState(next);
  }, []);

  const value = useMemo(() => ({ theme, setTheme }), [theme, setTheme]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (value === null) {
    throw new Error("useTheme must be used within a ThemeProvider.");
  }
  return value;
}

const choices = [
  { theme: "light", label: "Light", Icon: SunIcon },
  { theme: "dark", label: "Dark", Icon: MoonIcon },
  { theme: "system", label: "System", Icon: MonitorIcon },
] as const;

export function ThemeSwitch() {
  const { theme, setTheme } = useTheme();
  return (
    <div role="group" aria-label="Theme" className="flex gap-1">
      {choices.map(({ theme: choice, label, Icon }) => (
        <Button
          key={choice}
          type="button"
          size="sm"
          variant={theme === choice ? "secondary" : "ghost"}
          aria-pressed={theme === choice}
          className="min-h-11 flex-1 md:min-h-8"
          onClick={() => {
            setTheme(choice);
          }}
        >
          <Icon aria-hidden="true" />
          {label}
        </Button>
      ))}
    </div>
  );
}
