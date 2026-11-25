// Pixel critters for each tool. Our own drawings in each tool's colours, not
// the official mascots (the repo ships installers). 16×16 grid, crisp edges.

const svg = (cls: string, body: string) =>
  `<svg class="critter-svg ${cls}" viewBox="0 0 16 16" shape-rendering="crispEdges" aria-hidden="true">${body}</svg>`;

/** Orange, wide, four legs. `awake` false closes its eyes. */
export function claudeCritter(opts: { small?: boolean; awake?: boolean } = {}): string {
  const { small = false, awake = true } = opts;
  const fill = small ? "#ffb08f" : "#ff8f70";
  const legs = small
    ? `<rect class="leg-a" x="4" y="11" width="1" height="3"/><rect class="leg-b" x="11" y="11" width="1" height="3"/>`
    : `<rect class="leg-a" x="4" y="11" width="1" height="3"/><rect class="leg-b" x="6" y="11" width="1" height="3"/>
       <rect class="leg-a" x="9" y="11" width="1" height="3"/><rect class="leg-b" x="11" y="11" width="1" height="3"/>`;
  const arms = small ? "" : `<rect x="1" y="6" width="2" height="2"/><rect x="13" y="6" width="2" height="2"/>`;
  const eyes = awake
    ? `<g class="eyes"><rect x="5" y="6" width="1" height="2"/><rect x="10" y="6" width="1" height="2"/></g>`
    : `<g><rect x="5" y="7" width="2" height="1"/><rect x="9" y="7" width="2" height="1"/></g>`;
  return svg(
    `critter-claude${small ? " is-small" : ""}`,
    `<g fill="${fill}"><rect x="3" y="4" width="10" height="7"/>${arms}${legs}</g><g fill="#20140f">${eyes}</g>`,
  );
}

/** The boss face: same body, determined brows. */
export function bossCritter(awake: boolean): string {
  const face = awake
    ? `<rect x="5" y="6" width="2" height="1"/><rect x="9" y="6" width="2" height="1"/><rect x="6" y="7" width="1" height="1"/><rect x="9" y="7" width="1" height="1"/>`
    : `<rect x="5" y="7" width="2" height="1"/><rect x="9" y="7" width="2" height="1"/>`;
  return svg(
    "critter-claude critter-boss",
    `<g fill="#ff8f70"><rect x="3" y="4" width="10" height="7"/><rect x="1" y="6" width="2" height="2"/><rect x="13" y="6" width="2" height="2"/>
     <rect x="4" y="11" width="1" height="3"/><rect x="6" y="11" width="1" height="3"/><rect x="9" y="11" width="1" height="3"/><rect x="11" y="11" width="1" height="3"/></g>
     <g fill="#20140f">${face}</g>`,
  );
}

/** Blue, rounded top, a `>_` prompt for a face. */
export function codexCritter(opts: { small?: boolean; awake?: boolean } = {}): string {
  const { small = false, awake = true } = opts;
  const face = awake
    ? `<g class="eyes"><rect x="5" y="6" width="1" height="1"/><rect x="6" y="7" width="1" height="1"/><rect x="5" y="8" width="1" height="1"/></g><rect class="cursor" x="8" y="8" width="3" height="1"/>`
    : `<rect x="5" y="7" width="2" height="1"/><rect x="9" y="7" width="2" height="1"/>`;
  return svg(
    `critter-codex${small ? " is-small" : ""}`,
    `<g fill="${small ? "#8fdcff" : "#5cc8ff"}"><rect x="4" y="3" width="8" height="1"/><rect x="3" y="4" width="10" height="7"/>
     <rect x="2" y="6" width="1" height="4"/><rect x="13" y="6" width="1" height="4"/>
     <rect class="leg-a" x="5" y="11" width="2" height="2"/><rect class="leg-b" x="9" y="11" width="2" height="2"/></g>
     <g fill="#0d1a24">${face}</g>`,
  );
}

/** A cartoon whip; the crack is a small flash at its tip. */
export function whip(): string {
  return `<svg class="whip" viewBox="0 0 46 22" aria-hidden="true"><path d="M2 20 Q 12 2, 26 8 T 44 6"/><circle class="crack" cx="44" cy="6" r="3"/></svg>`;
}

/**
 * Compact-mode gauge: the critter drained of colour, refilled from the
 * bottom with what is left. At 70 % used, the lower 30 % is coloured.
 * The body spans rows 3–14 of the 16-row grid, so the fill maps onto that.
 */
export function fillCritter(provider: "claude" | "codex", usedPercent: number | null): string {
  const art = provider === "claude" ? claudeCritter() : codexCritter();
  const left = usedPercent == null ? 0 : Math.max(0, Math.min(100, 100 - usedPercent));
  const top = ((3 + 11 * (1 - left / 100)) / 16) * 100; // % of the box hidden from the top
  return `<span class="fill-critter fill-${provider}${usedPercent == null ? " is-unknown" : ""}" aria-hidden="true">
    <span class="fc-empty">${art}</span>
    <span class="fc-full" style="clip-path: inset(${top.toFixed(1)}% 0 0 0)">${art}</span>
  </span>`;
}
