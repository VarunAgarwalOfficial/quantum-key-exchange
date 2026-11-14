// Theme choice (auto, light, dark) and the canvas palette, shared by the small projects.
// Usage: initTheme(button) wires a toggle; palette() returns resolved colours;
// onThemeChange(fn) calls fn after the button or the system scheme changes.

const KEY = "theme";
const ORDER = ["auto", "light", "dark"];
const NAMES = ["bg", "ink", "muted", "faint", "accent", "compare", "change", "good", "visit"];
const listeners = new Set();

function stored() {
  try {
    const value = localStorage.getItem(KEY);
    return ORDER.includes(value) ? value : "auto";
  } catch {
    return "auto";
  }
}

function apply(choice) {
  if (choice === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}

function notify() {
  for (const fn of listeners) fn();
}

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

export function onThemeChange(fn) {
  listeners.add(fn);
}

/** Wires a button that cycles auto, light, dark and remembers the choice. */
export function initTheme(button) {
  let choice = stored();
  const show = () => {
    apply(choice);
    button.textContent = `Theme: ${choice}`;
  };
  show();
  button.addEventListener("click", () => {
    choice = ORDER[(ORDER.indexOf(choice) + 1) % ORDER.length];
    try {
      localStorage.setItem(KEY, choice);
    } catch {
      // Private mode: the choice lasts for this page only.
    }
    show();
    notify();
  });
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", notify);
}
