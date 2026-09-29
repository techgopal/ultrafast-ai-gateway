// jsdom has no layout and no media queries. The app learns the screen width and
// the device colour scheme from `window.matchMedia` and `window.innerWidth`,
// so the tests control those two.

type Listener = (event: MediaQueryListEvent) => void;

interface Device {
  width: number;
  prefersDark: boolean;
}

const device: Device = { width: 1280, prefersDark: false };
const queries = new Set<FakeMediaQueryList>();

function evaluate(query: string): boolean {
  const maxWidth = /\(max-width:\s*(\d+)px\)/.exec(query);
  if (maxWidth?.[1] !== undefined) return device.width <= Number(maxWidth[1]);
  const minWidth = /\(min-width:\s*(\d+)px\)/.exec(query);
  if (minWidth?.[1] !== undefined) return device.width >= Number(minWidth[1]);
  if (query.includes("prefers-color-scheme: dark")) return device.prefersDark;
  if (query.includes("prefers-color-scheme: light")) return !device.prefersDark;
  return false;
}

class FakeMediaQueryList {
  readonly media: string;
  matches: boolean;
  onchange: Listener | null = null;
  private readonly listeners = new Set<Listener>();

  constructor(media: string) {
    this.media = media;
    this.matches = evaluate(media);
  }

  addEventListener(_type: string, listener: Listener): void {
    this.listeners.add(listener);
  }

  removeEventListener(_type: string, listener: Listener): void {
    this.listeners.delete(listener);
  }

  addListener(listener: Listener): void {
    this.listeners.add(listener);
  }

  removeListener(listener: Listener): void {
    this.listeners.delete(listener);
  }

  dispatchEvent(): boolean {
    return true;
  }

  refresh(): void {
    const matches = evaluate(this.media);
    if (matches === this.matches) return;
    this.matches = matches;
    const event = { matches, media: this.media } as MediaQueryListEvent;
    for (const listener of this.listeners) listener(event);
  }
}

/** jsdom has no layout, so nothing ever changes its size. */
class FakeResizeObserver {
  observe(): void {
    return undefined;
  }

  unobserve(): void {
    return undefined;
  }

  disconnect(): void {
    return undefined;
  }
}

function install(): void {
  // Radix measures its radio buttons and checkboxes with it.
  Object.defineProperty(window, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: FakeResizeObserver,
  });
  // jsdom has no scrolling; the router resets the scroll position after it navigates.
  Object.defineProperty(window, "scrollTo", {
    configurable: true,
    writable: true,
    value: () => undefined,
  });
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => {
      const list = new FakeMediaQueryList(query);
      queries.add(list);
      return list;
    },
  });
  Object.defineProperty(window, "innerWidth", {
    configurable: true,
    writable: true,
    value: device.width,
  });
}

/** Sets the device and tells every live media query about the change. */
export function setDevice(change: Partial<Device>): void {
  Object.assign(device, change);
  install();
  for (const list of queries) list.refresh();
}

export function resetDevice(): void {
  queries.clear();
  device.width = 1280;
  device.prefersDark = false;
  install();
}
