// Browser view: every number comes from qkx.wasm (the Rust library); this file only draws.
import { onThemeChange, palette } from "./theme.js";

const $ = (id) => document.getElementById(id);
const SVG = "http://www.w3.org/2000/svg";
const SHOWN = 24;
const TRAVEL_MS = 1100;
const EVE_NAMES = ["none", "Eve, random basis", "Eve, always +", "Eve, always x"];
const VERDICTS = {
  key: "key agreed",
  errors: "stopped: too many errors",
  hash: "stopped: hashes differ",
  empty: "stopped: no key left",
};
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

let wasm;
const state = { eve: 0, fraction: 100, noise: 1, photons: 20000, seed: 1 };
let run;
let revealed = 0;
let playing = false;
let flight = null;
let colours;

/** Runs one exchange in Rust and returns its JSON description. */
function simulate(eve, fraction, noise, enforce, shown) {
  const { memory, qkx_run, qkx_free } = wasm;
  const pointer = qkx_run(
    state.photons,
    noise / 100,
    eve,
    fraction / 100,
    state.seed >>> 0,
    enforce ? 1 : 0,
    shown,
  );
  const length = new DataView(memory.buffer).getUint32(pointer, true);
  const json = new TextDecoder().decode(new Uint8Array(memory.buffer, pointer + 4, length));
  qkx_free(pointer, 4 + length);
  return JSON.parse(json);
}

/** One photon's flags, unpacked. */
function photon(flags) {
  return {
    aliceBit: flags & 1,
    aliceBasis: (flags >> 1) & 1,
    tapped: (flags & 4) !== 0,
    eveBasis: (flags >> 3) & 1,
    eveBit: (flags >> 4) & 1,
    bobBasis: (flags >> 5) & 1,
    bobBit: (flags >> 6) & 1,
  };
}

const basisName = (b) => (b ? "x" : "+");
/** The polarisation a basis and bit stand for, as a character. */
const glyph = (basis, bit) => (basis ? (bit ? "\\" : "/") : bit ? "|" : "–");
const angle = (basis, bit) => (basis ? (bit ? 135 : 45) : bit ? 90 : 0);
const percent = (x, digits = 1) => `${(100 * x).toFixed(digits)}%`;
const number = (n) => n.toLocaleString("en");

function element(name, attributes = {}, text) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  return node;
}

function clear(svg) {
  for (const child of [...svg.children]) if (child.tagName !== "title") child.remove();
}

// The line: Alice, maybe Eve, Bob, and one photon in flight.

const STATION = { alice: 48, eve: 360, bob: 672 };

/** Drawing width in CSS pixels, so that text stays the same size on a phone. */
function fit(svg, min, max, height) {
  const width = Math.round(Math.max(min, Math.min(max, svg.parentElement.clientWidth)));
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("width", width);
  svg.setAttribute("height", height);
  return width;
}
const Y = 62;

function station(svg, x, name, colour) {
  svg.append(
    element("rect", {
      x: x - 34,
      y: Y - 22,
      width: 68,
      height: 44,
      rx: 8,
      fill: colours.bg,
      stroke: colour,
      "stroke-width": 1.5,
    }),
    element(
      "text",
      { x, y: Y + 5, "text-anchor": "middle", fill: colours.ink, "font-size": 14 },
      name,
    ),
  );
}

function photonMark(x, basis, bit, colour) {
  const g = element("g", { transform: `translate(${x} ${Y})` });
  g.append(
    element("circle", { r: 13, fill: colours.bg, stroke: colour, "stroke-width": 2 }),
    element("line", {
      x1: -8,
      x2: 8,
      y1: 0,
      y2: 0,
      stroke: colour,
      "stroke-width": 2.5,
      "stroke-linecap": "round",
      transform: `rotate(${-angle(basis, bit)})`,
    }),
  );
  return g;
}

/** Draws the line with the current photon at `progress` (0 at Alice, 1 at Bob), or none. */
function drawLine(progress) {
  const svg = $("line");
  clear(svg);
  const width = fit(svg, 260, 720, 132);
  STATION.alice = 40;
  STATION.bob = width - 40;
  STATION.eve = width / 2;
  const eveOn = state.eve !== 0;
  svg.append(
    element("line", {
      x1: STATION.alice + 34,
      x2: STATION.bob - 34,
      y1: Y,
      y2: Y,
      stroke: colours.faint,
      "stroke-width": 2,
    }),
  );
  station(svg, STATION.alice, "Alice", colours.accent);
  station(svg, STATION.bob, "Bob", colours.accent);
  if (eveOn) station(svg, STATION.eve, "Eve", colours.change);
  const fibreEnd = eveOn ? STATION.eve - 34 : STATION.bob - 34;
  svg.append(
    element(
      "text",
      {
        x: (STATION.alice + 34 + fibreEnd) / 2,
        y: Y - 30,
        "text-anchor": "middle",
        fill: colours.muted,
        "font-size": 12,
      },
      "optical fibre",
    ),
  );

  const index = progress === null ? revealed - 1 : revealed;
  const p = run.records[index] === undefined ? null : photon(run.records[index]);
  const caption = (x, text) =>
    svg.append(
      element(
        "text",
        { x, y: Y + 50, "text-anchor": "middle", fill: colours.muted, "font-size": 12 },
        text,
      ),
    );
  if (revealed === 0 && progress === null) {
    caption(STATION.eve, "Nothing has run yet.");
    svg.append(
      element(
        "text",
        {
          x: STATION.eve,
          y: Y + 66,
          "text-anchor": "middle",
          fill: colours.muted,
          "font-size": 12,
        },
        `${run.records.length} photons wait at Alice.`,
      ),
    );
  }
  if (p) {
    caption(STATION.alice, `sends ${p.aliceBit} in ${basisName(p.aliceBasis)}`);
    if (eveOn) {
      caption(STATION.eve, p.tapped ? `measures in ${basisName(p.eveBasis)}` : "lets it pass");
    }
    if (progress === null || progress >= 1) {
      caption(STATION.bob, `reads ${p.bobBit} in ${basisName(p.bobBasis)}`);
    }
  }
  if (!p || progress === null) return;

  const x = STATION.alice + 34 + progress * (STATION.bob - STATION.alice - 68);
  // After Eve has measured, the photon carries what her measurement left behind.
  const pastEve = eveOn && p.tapped && x >= STATION.eve;
  const basis = pastEve ? p.eveBasis : p.aliceBasis;
  const bit = pastEve ? p.eveBit : p.aliceBit;
  svg.append(
    photonMark(x, basis, bit, pastEve ? colours.change : colours.accent),
    element(
      "text",
      { x, y: Y + 31, "text-anchor": "middle", fill: colours.ink, "font-size": 12 },
      "photon",
    ),
  );
}

// The strip: one row per photon sent so far.

function keptText(p) {
  if (p.aliceBasis !== p.bobBasis) return "no";
  return p.aliceBit === p.bobBit ? "yes" : "yes, wrong";
}

function drawStrip() {
  const table = $("strip");
  table.replaceChildren();
  const columns = [
    ["Photon", (_, i) => (i === revealed - 1 ? `${i + 1}, latest` : String(i + 1))],
    ["Alice sends", (p) => `${p.aliceBit} as ${glyph(p.aliceBasis, p.aliceBit)}`],
  ];
  if (state.eve !== 0) {
    columns.push(["Eve's basis", (p) => (p.tapped ? basisName(p.eveBasis) : "none")]);
  }
  columns.push(
    ["Bob's basis", (p) => basisName(p.bobBasis)],
    ["Bob reads", (p) => p.bobBit],
    ["Kept", keptText],
  );
  const head = document.createElement("tr");
  for (const [name] of columns) {
    const th = document.createElement("th");
    th.scope = "col";
    th.textContent = name;
    head.append(th);
  }
  table.append(head);
  if (revealed === 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = columns.length;
    td.textContent = "No photons sent yet.";
    tr.append(td);
    table.append(tr);
    return;
  }
  run.records.slice(0, revealed).forEach((flags, i) => {
    const p = photon(flags);
    const tr = document.createElement("tr");
    if (i === revealed - 1) tr.className = "current";
    for (const [, value] of columns) {
      const td = document.createElement("td");
      td.textContent = String(value(p, i));
      tr.append(td);
    }
    if (p.aliceBasis === p.bobBasis)
      tr.lastChild.className = p.aliceBit === p.bobBit ? "kept" : "wrong";
    table.append(tr);
  });
}

function drawSifted() {
  const kept = run.records
    .slice(0, revealed)
    .map(photon)
    .filter((p) => p.aliceBasis === p.bobBasis);
  const alice = $("alice-key");
  const bob = $("bob-key");
  alice.replaceChildren();
  bob.replaceChildren();
  let errors = 0;
  for (const p of kept) {
    const wrong = p.aliceBit !== p.bobBit;
    errors += wrong ? 1 : 0;
    const a = document.createElement("span");
    const b = document.createElement("span");
    a.textContent = String(p.aliceBit);
    b.textContent = String(p.bobBit);
    if (wrong) {
      a.className = "wrong";
      b.className = "wrong";
      b.title = "differs from Alice";
    }
    alice.append(a);
    bob.append(b);
  }
  if (kept.length === 0) {
    alice.textContent = "—";
    bob.textContent = "—";
  }
  $("sifted-note").textContent =
    revealed === 0
      ? "Nothing sent yet, so nothing kept. Over a whole exchange about half the photons are kept. Alice and Bob tell each other only which basis they used, never the bits."
      : `${kept.length} of ${revealed} photons kept so far, ${errors} wrong${errors ? " (underlined)" : ""}. ` +
        "Over a whole exchange about half are kept. Alice and Bob tell each other only which basis they used, never the bits.";
}

function describePhoton(i) {
  if (i < 0) {
    const who =
      state.eve === 0
        ? "No one is listening on the line."
        : `${EVE_NAMES[state.eve]} is listening on the line.`;
    return `Nothing has run yet. Alice has ${run.records.length} photons ready. ${who} Press Play to send them one after another, or Send one photon to go one at a time.`;
  }
  const p = photon(run.records[i]);
  let text = `Photon ${i + 1}: Alice sends ${p.aliceBit} in the ${basisName(p.aliceBasis)} basis (${glyph(p.aliceBasis, p.aliceBit)}). `;
  if (state.eve !== 0) {
    text += p.tapped
      ? `Eve measures in ${basisName(p.eveBasis)}, reads ${p.eveBit} and sends ${glyph(p.eveBasis, p.eveBit)} on. `
      : "Eve lets it pass. ";
  }
  text += `Bob measures in ${basisName(p.bobBasis)} and reads ${p.bobBit}. `;
  if (p.aliceBasis !== p.bobBasis)
    return `${text}Different bases, so Bob's bit is a coin flip: thrown away.`;
  return `${text}Same basis: kept${p.aliceBit === p.bobBit ? "." : ", but the bit is wrong, from noise or Eve."}`;
}

function drawChain() {
  const r = run;
  const items = [
    `${number(r.photons)} photons sent; ${number(r.sifted)} kept after comparing bases (${percent(r.sifted / r.photons)}).`,
    `${number(r.sample)} kept bits said out loud to check for errors: ${percent(r.qber)} wrong (+ basis ${percent(r.qberRect)}, x basis ${percent(r.qberDiag)}). Above 11% they give up.`,
  ];
  if (r.verdict === "errors") {
    items.push("Above the limit, so both stop. Someone may be listening; no key, no message.");
  } else {
    const efficiency = r.efficiency > 0 ? `, ${r.efficiency.toFixed(2)} times the minimum` : "";
    items.push(
      `Cascade, an error-correcting method, finds and fixes ${number(r.corrected)} wrong bits in the ${number(r.rawLen)}-bit key. It does this by comparing ${number(r.leaked)} parities (whether a group of bits holds an odd or even number of 1s)${efficiency}.`,
    );
    if (r.verdict === "hash")
      items.push(
        "The 64-bit fingerprints (hashes) of the two keys differ: errors survived. Both stop.",
      );
    else {
      items.push(
        "A 64-bit fingerprint (a Toeplitz hash) of the two keys matches, so the keys are the same.",
      );
      items.push(
        r.finalLen > 0
          ? `Privacy amplification shortens the key with a hash so that what Eve may know is useless: ${number(r.finalLen)} secret bits are left, ${(r.finalLen / r.photons).toFixed(4)} per photon.`
          : "Privacy amplification, shortening the key so that what Eve may know is useless, leaves nothing: the errors cost more than the key holds.",
      );
    }
  }
  if (r.key) items.push(`Final key begins ${r.key.slice(0, 48)}…`);
  $("chain").replaceChildren(
    ...items.map((text) => {
      const li = document.createElement("li");
      li.textContent = text;
      return li;
    }),
  );
}

function drawCompare() {
  const eve = state.eve || 1;
  const fraction = state.eve ? state.fraction : 100;
  const rows = [
    ["No eavesdropper", simulate(0, 0, state.noise, true, 0)],
    [`${EVE_NAMES[eve]}, ${fraction}% intercepted`, simulate(eve, fraction, state.noise, true, 0)],
  ];
  $("compare").replaceChildren(
    ...rows.map(([name, r]) => {
      const tr = document.createElement("tr");
      const cells = [
        name,
        percent(r.qber),
        percent(r.qberRect),
        percent(r.qberDiag),
        r.intercepted ? percent(r.eveKnown / Math.max(1, r.sifted), 0) : "—",
        r.verdict === "key" ? `${number(r.finalLen)} bits` : "—",
        VERDICTS[r.verdict],
      ];
      for (const [i, text] of cells.entries()) {
        const cell = document.createElement(i === 0 ? "th" : "td");
        if (i === 0) cell.scope = "row";
        cell.textContent = text;
        tr.append(cell);
      }
      return tr;
    }),
  );
}

// Key rate chart.

const CHART = { w: 720, h: 300, left: 48, right: 16, top: 30, bottom: 44 };
let rates = [];

function entropy(p) {
  if (p <= 0 || p >= 1) return 0;
  return -p * Math.log2(p) - (1 - p) * Math.log2(1 - p);
}

function computeRates() {
  const noises = Array.from({ length: 25 }, (_, i) => i / 2);
  rates = noises.map((noise) => ({
    noise,
    quiet: simulate(0, 0, noise, false, 0).finalLen / state.photons,
    tapped: state.eve
      ? simulate(state.eve, state.fraction, noise, false, 0).finalLen / state.photons
      : null,
    ideal: Math.max(0, 0.375 * (1 - 2 * entropy(noise / 100))),
  }));
}

function series() {
  const list = [
    { key: "quiet", name: "No eavesdropper", colour: colours.series[0] },
    { key: "ideal", name: "Ideal, no eavesdropper", colour: colours.muted, dash: "5 4" },
  ];
  if (state.eve) {
    list.splice(1, 0, {
      key: "tapped",
      name: `${EVE_NAMES[state.eve]}, ${state.fraction}%`,
      colour: colours.series[1],
    });
  }
  return list;
}

const sx = (noise) => CHART.left + (noise / 12) * (CHART.w - CHART.left - CHART.right);
const sy = (rate) => CHART.h - CHART.bottom - (rate / 0.4) * (CHART.h - CHART.top - CHART.bottom);

function drawRates() {
  const svg = $("rate");
  clear(svg);
  CHART.w = fit(svg, 300, 900, CHART.h);
  const text = (x, y, value, anchor = "middle", fill = colours.muted) =>
    element("text", { x, y, "text-anchor": anchor, fill, "font-size": 12 }, value);
  for (const r of [0, 0.1, 0.2, 0.3, 0.4]) {
    svg.append(
      element("line", {
        x1: CHART.left,
        x2: CHART.w - CHART.right,
        y1: sy(r),
        y2: sy(r),
        stroke: colours.faint,
      }),
      text(CHART.left - 8, sy(r) + 4, r.toFixed(1), "end"),
    );
  }
  for (let n = 0; n <= 12; n += 2) svg.append(text(sx(n), CHART.h - CHART.bottom + 18, `${n}%`));
  svg.append(
    text((CHART.left + CHART.w) / 2, CHART.h - 6, "line noise"),
    text(4, 12, "secret bits per photon", "start"),
  );
  // Current setting.
  if (state.noise <= 12) {
    svg.append(
      element("line", {
        x1: sx(state.noise),
        x2: sx(state.noise),
        y1: CHART.top,
        y2: CHART.h - CHART.bottom,
        stroke: colours.ink,
        "stroke-dasharray": "2 3",
      }),
      text(sx(state.noise) + 4, CHART.top + 10, "noise now set", "start", colours.ink),
    );
  }
  for (const s of series()) {
    const points = rates.map((r) => `${sx(r.noise).toFixed(1)},${sy(r[s.key]).toFixed(1)}`);
    svg.append(
      element("polyline", {
        points: points.join(" "),
        fill: "none",
        stroke: s.colour,
        "stroke-width": 2,
        "stroke-dasharray": s.dash ?? "none",
        "stroke-linejoin": "round",
      }),
    );
  }
  const cursor = element("line", {
    y1: CHART.top,
    y2: CHART.h - CHART.bottom,
    stroke: colours.muted,
    visibility: "hidden",
  });
  svg.append(cursor);

  $("rate-legend").replaceChildren(
    ...series().map((s) => {
      const li = document.createElement("li");
      const swatch = document.createElement("span");
      swatch.className = s.dash ? "swatch dashed" : "swatch line";
      swatch.style.color = s.colour;
      li.append(swatch, s.name);
      return li;
    }),
  );
  const head = $("rate-head");
  head.replaceChildren();
  for (const name of ["Line noise", ...series().map((s) => s.name)]) {
    const th = document.createElement("th");
    th.scope = "col";
    th.textContent = name;
    head.append(th);
  }
  $("rate-table").replaceChildren(
    ...rates.map((r) => {
      const tr = document.createElement("tr");
      for (const value of [`${r.noise.toFixed(1)}%`, ...series().map((s) => r[s.key].toFixed(4))]) {
        const td = document.createElement("td");
        td.textContent = value;
        tr.append(td);
      }
      return tr;
    }),
  );

  const tip = $("tip");
  svg.onpointermove = (event) => {
    const box = svg.getBoundingClientRect();
    const x = ((event.clientX - box.left) / box.width) * CHART.w;
    const noise = Math.round(((x - CHART.left) / (CHART.w - CHART.left - CHART.right)) * 24) / 2;
    const r = rates.find((row) => row.noise === noise);
    if (!r) return hideTip();
    cursor.setAttribute("x1", sx(noise));
    cursor.setAttribute("x2", sx(noise));
    cursor.setAttribute("visibility", "visible");
    tip.replaceChildren();
    const title = document.createElement("strong");
    title.textContent = `Line noise ${noise.toFixed(1)}%`;
    tip.append(title);
    for (const s of series()) {
      const line = document.createElement("div");
      const swatch = document.createElement("span");
      swatch.className = s.dash ? "swatch dashed" : "swatch line";
      swatch.style.color = s.colour;
      line.append(swatch, `${s.name}: ${r[s.key].toFixed(4)}`);
      tip.append(line);
    }
    tip.hidden = false;
    const left = (sx(noise) / CHART.w) * box.width;
    tip.style.left = `${Math.min(left + 12, box.width - tip.offsetWidth - 4)}px`;
  };
  const hideTip = () => {
    tip.hidden = true;
    cursor.setAttribute("visibility", "hidden");
  };
  svg.onpointerleave = hideTip;
}

// Playback.

function drawPlayback(progress = null) {
  drawLine(progress);
  drawStrip();
  drawSifted();
  $("status").textContent = describePhoton(progress === null ? revealed - 1 : revealed);
  $("back").setAttribute("aria-disabled", String(revealed === 0));
  const done = revealed >= run.records.length;
  $("step").setAttribute("aria-disabled", String(done));
  $("all").setAttribute("aria-disabled", String(done));
  $("play").setAttribute("aria-disabled", String(done && !playing));
  $("play").textContent = playing ? "Pause" : "Play";
}

function stopFlight() {
  if (flight) cancelAnimationFrame(flight.frame);
  flight = null;
}

/** Sends the next photon down the line, animated unless motion is reduced. */
function sendNext(then) {
  if (revealed >= run.records.length) return;
  if (reducedMotion.matches) {
    revealed += 1;
    drawPlayback();
    then?.();
    return;
  }
  const start = performance.now();
  const frame = (now) => {
    const progress = Math.min(1, (now - start) / TRAVEL_MS);
    if (progress < 1) {
      drawLine(progress);
      flight.frame = requestAnimationFrame(frame);
      return;
    }
    flight = null;
    revealed += 1;
    drawPlayback();
    then?.();
  };
  flight = { frame: requestAnimationFrame(frame) };
  $("status").textContent = describePhoton(revealed);
}

function playLoop() {
  if (!playing) return;
  if (revealed >= run.records.length) {
    playing = false;
    drawPlayback();
    return;
  }
  sendNext(() => {
    if (playing) setTimeout(playLoop, reducedMotion.matches ? 700 : 150);
  });
}

function pause() {
  playing = false;
  stopFlight();
}

// State, URL and controls.

function readUrl() {
  const params = new URLSearchParams(location.hash.slice(1));
  const int = (name, lo, hi) => {
    const value = Number(params.get(name));
    return params.has(name) && Number.isFinite(value) ? Math.min(hi, Math.max(lo, value)) : null;
  };
  state.eve = int("eve", 0, 3) ?? state.eve;
  state.fraction = int("fraction", 0, 100) ?? state.fraction;
  state.noise = int("noise", 0, 15) ?? state.noise;
  const photons = int("photons", 2000, 200000);
  if ([2000, 20000, 200000].includes(photons)) state.photons = photons;
  state.seed = Math.floor(int("seed", 0, 4294967295) ?? state.seed);
}

function writeUrl() {
  const params = new URLSearchParams(Object.entries(state).map(([k, v]) => [k, String(v)]));
  history.replaceState(null, "", `#${params}`);
}

function showControls() {
  $("eve").value = String(state.eve);
  $("fraction").value = String(state.fraction);
  $("fraction").disabled = state.eve === 0;
  $("fraction-out").textContent = `${state.fraction}%`;
  $("noise").value = String(state.noise);
  $("noise-out").textContent = `${state.noise.toFixed(1)}%`;
  $("photons").value = String(state.photons);
  $("seed").value = String(state.seed);
}

function recompute({ rates: withRates = true } = {}) {
  pause();
  showControls();
  writeUrl();
  run = simulate(state.eve, state.fraction, state.noise, true, SHOWN);
  revealed = Math.min(revealed, run.records.length);
  drawPlayback();
  drawChain();
  drawCompare();
  if (withRates) computeRates();
  drawRates();
}

function wire() {
  const debounce = (fn, ms) => {
    let timer = 0;
    return () => {
      clearTimeout(timer);
      timer = setTimeout(fn, ms);
    };
  };
  const slow = debounce(() => recompute(), 120);
  const noiseLater = debounce(() => recompute({ rates: false }), 120);
  $("eve").addEventListener("change", () => {
    state.eve = Number($("eve").value);
    recompute();
  });
  $("fraction").addEventListener("input", () => {
    state.fraction = Number($("fraction").value);
    $("fraction-out").textContent = `${state.fraction}%`;
    slow();
  });
  $("noise").addEventListener("input", () => {
    state.noise = Number($("noise").value);
    $("noise-out").textContent = `${state.noise.toFixed(1)}%`;
    noiseLater();
  });
  $("photons").addEventListener("change", () => {
    state.photons = Number($("photons").value);
    recompute();
  });
  $("seed").addEventListener("change", () => {
    const value = Math.floor(Number($("seed").value));
    state.seed = Number.isFinite(value) ? Math.min(4294967295, Math.max(0, value)) : 1;
    revealed = 0;
    recompute();
  });
  $("reseed").addEventListener("click", () => {
    state.seed = crypto.getRandomValues(new Uint32Array(1))[0];
    revealed = 0;
    recompute();
  });
  $("play").addEventListener("click", () => {
    if (playing) {
      pause();
      drawPlayback();
      return;
    }
    if (revealed >= run.records.length) return;
    playing = true;
    drawPlayback();
    playLoop();
  });
  $("step").addEventListener("click", () => {
    if (flight || revealed >= run.records.length) return;
    pause();
    sendNext();
  });
  $("back").addEventListener("click", () => {
    pause();
    revealed = Math.max(0, revealed - 1);
    drawPlayback();
  });
  $("all").addEventListener("click", () => {
    pause();
    revealed = run.records.length;
    drawPlayback();
  });
  addEventListener("hashchange", () => {
    readUrl();
    revealed = 0;
    recompute();
  });
}

function onResize() {
  let last = 0;
  addEventListener("resize", () => {
    cancelAnimationFrame(last);
    last = requestAnimationFrame(() => {
      if (!flight) drawLine(null);
      drawRates();
    });
  });
}

async function main() {
  colours = palette();
  onThemeChange(() => {
    colours = palette();
    drawPlayback();
    drawRates();
  });
  const { instance } = await WebAssembly.instantiateStreaming(fetch("qkx.wasm"));
  wasm = instance.exports;
  readUrl();
  wire();
  $("rate")
    .closest("details")
    .addEventListener("toggle", () => drawRates());
  onResize();
  recompute();
}

main().catch((error) => {
  $("status").textContent = `The simulation could not start: ${error.message}`;
});
