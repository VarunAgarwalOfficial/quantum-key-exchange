// The canvas palette, shared by the small projects. The page follows the system colour scheme.
// Usage: palette() returns resolved colours; onThemeChange(fn) calls fn after the scheme changes.

const NAMES = ["bg", "ink", "muted", "faint", "accent", "compare", "change", "good", "visit"];
const listeners = new Set();

/** Resolved canvas colours as CSS colour strings. light-dark() needs a probe element. */
export function palette() {
  const probe = document.createElement("span");
  probe.style.display = "none";
  document.body.append(probe);
  const get = (name) => {
    probe.style.color = `var(--canvas-${name})`;
    return getComputedStyle(probe).color;
  };
  const colours = Object.fromEntries(NAMES.map((name) => [name, get(name)]));
  colours.series = [1, 2, 3, 4, 5, 6].map((i) => get(`series-${i}`));
  probe.remove();
  return colours;
}

/** Calls fn after the system colour scheme changes; the page follows it. */
export function onThemeChange(fn) {
  listeners.add(fn);
}

matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  for (const fn of listeners) fn();
});
