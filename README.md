# NuclearSim

The Nuclear Game: a sandbox for nuclear physics with real nuclide data
(NUBASE2020): build nuclei, fission and fusion, decay chains, working
reactors (heavy-water, molten-salt, nuclear thermal rocket), fusion plasmas
and accelerator beams.

Physics, rendering and the panel GUI live in the shared
[SimCore](https://github.com/Plutonium239ohno/SimCore) library.

```
cargo run --release
```

Tab hides the panel, Space pauses, Z opens the magnifier; hover anything
for details.

## Tabs

- **Build** — tools: walls (freehand, line, rectangle, circle, polygon;
  grid and angle snapping), nuclei, fill, spawners (feeds, or beams with an energy
  per nucleon, width, energy spread, divergence and pulses), pumps, drains,
  electric/magnetic field regions, control rods, fuel channels, neutron
  sources, the inspector and detectors, and Select: box-select walls,
  matter and parts (or any of the three), then copy, cut, paste, duplicate,
  drag to move, turn (90° or by the angle step), mirror, align to the grid
  or snap every point to it. Tools that place a nuclide show the
  nuclide picker right under them (picking one never switches tools).
- **Controls** — everything you can adjust while it runs, in collapsible
  sections: every spawner and beam (nuclide or mix, rate, energy, direction,
  width, spread, pulses; an opposing beam in one click), every field region
  (B and E), control rods, fuel, neutron sources, pumps, and **Automation**
  (with an on/off switch): a controller that holds a reading (fission or
  fusion rate, power, decay heat, neutrons, temperature, period, a
  detector's rate) with the rods or by throttling the spawners, and rules —
  at a time, every so often, above/below a level, or at the first reaction
  of a kind → SCRAM, move the rods, throttle, fire a burst, set the
  sources, pause.
- **Reactions** — the results: reaction power (released, absorbed, net,
  beam power in, gain), decay heat, rates, graphs, the busiest reactions and
  their energy regime, the plant's state, drains, and what the scene is made
  of (fuel, bred fuel, fission products, poisons, fusion fuel and ash) with
  the decay heat of each, plus the spent-fuel pool.
- **Measure** — the inspector (a nucleus's properties, the reaction or decay
  that made it, its ancestry and what it became; follow it with the zoom)
  and detectors (box or line; everything, neutral, charged, positive,
  negative, one element or one isotope; energy spectrum).
- **Isotopes** — the chart of nuclides with what's in the scene lit up;
  click a square to choose it.
- **View**, **Scenes**, **Physics**, **Log** — colouring and overlays,
  scenarios and saved scenes, the physics settings, the event log.

## Keys

| Key | | Key | |
|---|---|---|---|
| 1–9 | tools | Space | pause |
| I | inspect | . | step |
| K | detector | [ / ] | speed |
| M | field region | Ctrl+Z / Ctrl+Y | undo / redo |
| F | push / pull | ↑ / ↓, X | rods, SCRAM |
| V, L, O | colour, trails, overlay | P | point nuclei |
| Z | magnifier | C | clear |
| G | grid snap | Ctrl (hold) | snap angles / squares |
| S | select | Ctrl+C / X / V / D | copy, cut, paste, duplicate |
| R / Shift+R | turn 90° right / left | Q / E | turn by the angle step |
| H / Shift+H | mirror ↔ / ↕ | Del, Esc | delete, cancel / deselect |
