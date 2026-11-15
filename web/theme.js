// The canvas palette, shared by the small projects (byte-identical in each). The colours are the
// site's tokens, aliased in base.css; dark follows the system colour scheme, as the site does.
// Usage: palette() returns resolved colours; onThemeChange(fn) calls fn when the scheme changes.

const NAMES = ["bg", "ink", "muted", "faint", "accent", "compare", "change", "good", "visit"];

/** Resolved canvas colours as CSS colour strings, read through a probe element. */
export function palette() {
  const probe = document.createElement("span");
  probe.style.display = "none";
  document.body.append(probe);
  const get = (name) => {
    probe.style.color = `var(--canvas-${name})`;
    return getComputedStyle(probe).color;
  };
  const colours = Object.fromEntries(NAMES.map((name) => [name, get(name)]));
  // The site's four meaning colours; a picture needing more groups adds a shape or pattern.
  colours.series = [1, 2, 3, 4].map((i) => get(`series-${i}`));
  probe.remove();
  return colours;
}

/** Calls fn after the system colour scheme changes. */
export function onThemeChange(fn) {
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => fn());
}
