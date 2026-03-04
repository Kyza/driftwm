# Multi-Monitor Design

Multiple monitors = multiple independent viewports on the same infinite canvas.
Each monitor has its own camera position and zoom level. Panning/zooming on one
monitor affects only that monitor's viewport. Windows exist at canvas coordinates
shared across all monitors.

## Core model

Monitors are cameras, not containers. Windows live on the canvas, not on
monitors. The only things that are "per-monitor" are the viewport (where the
camera is pointed and how zoomed in) and rendering. Most compositor code doesn't
need to know about multiple monitors — only the render pipeline and input routing
do.

```
Monitor A: viewport at (0, 0) z=1.0    Monitor B: viewport at (3000, 500) z=0.5
┌──────────────┐                        ┌──────────────┐
│  [terminal]  │                        │ [terminal]   │
│        [vim] │                        │   [vim]      │
└──────────────┘                        │   [browser]  │
                                        └──────────────┘
              ← same infinite canvas →
```

## Per-output state

Each output has independent viewport state (stored via smithay's `UserDataMap`
on the `Output` object):

- camera, zoom, zoom_target, zoom_animation_center, last_rendered_zoom
- overview_return, camera_target
- last_scroll_pan, momentum, panning, edge_pan_velocity
- frame_counter, last_frame_instant, last_rendered_camera
- cached_bg_element
- fullscreen
- lock_surface

Everything else is global: space, seat, config, focus_history, decorations,
protocol states, gesture state, cursor state.

## Pointer crossing between monitors

The cursor crosses between monitors in screen space — move it off the right edge
of monitor A and it appears on the left edge of monitor B. The cursor's canvas
position changes discontinuously because the two viewports are looking at
different canvas areas. This is expected.

### Edge zone as monitor boundary

The existing edge-pan zone (configurable, default 100px) doubles as a sticky
boundary between adjacent monitors:

- **Slow movement into edge zone** → triggers edge-pan as usual (viewport
  scrolls in that direction)
- **Fast movement through edge zone** → breaks through to adjacent monitor (if
  one exists at that edge)

Velocity threshold determines which behavior fires. If no adjacent monitor
exists at that edge, always edge-pan regardless of speed.

### Dragging windows between monitors

When dragging a window and the cursor crosses to another monitor, the window's
canvas position is adjusted to stay under the cursor relative to the new
viewport's canvas space. This is an intentional canvas-space teleport — the user
is explicitly moving a window between viewports.

When both monitors are viewing the same (or overlapping) canvas area, window
drags cross seamlessly with no repositioning needed — the window is already
visible on both screens.

For normal cursor movement (no drag), no window repositioning. The cursor
crosses in screen space, the canvas position jumps, that's it.

## Output configuration

```toml
[[output]]
name = "eDP-1"           # connector name (required)
scale = 1.5              # fractional scale (default: 1.0)
transform = "normal"     # normal, 90, 180, 270, flipped, flipped-90, etc.
position = "auto"        # "auto" (default) or [x, y] in layout coords
mode = "preferred"       # "preferred" (default) or "WxH" or "WxH@Hz"
```

`position = "auto"` arranges outputs left-to-right in connection order. The
winit backend ignores `[[output]]` config (always one virtual output).

The `zwlr-output-management-unstable-v1` protocol enables GUI tools (wdisplays)
and CLI tools (wlr-randr) to read and modify output configuration at runtime.

## Window placement and navigation

- New windows open at the center of the **active output's** viewport (the output
  the pointer is on)
- `center-nearest` direction search uses the active output's viewport
- `zoom-to-fit` fits all windows within the active output's viewport
- `home-toggle` returns the active output to origin / zoom 1.0
- Layer shell surfaces bind to a specific output (protocol includes output
  selection). If no output specified, use the active output.
- Foreign toplevel activation pans the output closest to the target window

## State file

Save/restore per-output camera/zoom keyed by output name. Fall back to origin
for newly connected outputs.

## Screencopy / session lock

Screencopy is already per-output (takes an `Output` parameter). Session lock
needs one lock surface per output.

## Implementation phases

Reference: niri handles multi-monitor well. Clone for reference:
`git clone --depth 1 https://github.com/niri-wm/niri.git /tmp/niri`

The cleanup pass already annotated every field on `DriftWm` as `per-output` or
`global`, and marked all `// single-output assumption` sites (12 total across 8
files). Use these annotations as the guide.

### Phase 1: Per-output state extraction

Extract all fields marked `// -- per-output` into an `OutputState` struct stored
on each `Output` via smithay's `UserDataMap` (wrap in `Mutex` since
`UserDataMap` requires `Sync`).

Add helper methods:
- `output_state(&self, output: &Output) -> MutexGuard<OutputState>`
- `active_output(&self) -> Output` — the output the pointer is currently on
  (for now: first output, fixed in Phase 3)
- Keep `self.camera` / `self.zoom` as convenience accessors that delegate to
  `active_output()`'s state, so existing code doesn't all break at once

Update `update_output_from_camera()` to map each output to its own camera.

Update `compose_frame()` — already receives `output` as a parameter, use
`output_state(output)` for camera/zoom instead of `self.camera`/`self.zoom`.

Update `animation.rs` — tick animations for all outputs, not just a global
camera. Each output has independent momentum, camera targets, zoom animation.

**Verification**: single monitor works identically. All 12
`// single-output assumption` sites compile but may still use `active_output()`
fallback.

### Phase 2: Output configuration

Add `[[output]]` config section (see Output configuration above).

- Parse in `config/toml.rs` and `config/mod.rs`
- Apply in udev backend when output is connected (match by connector name)
- `position = "auto"` arranges outputs left-to-right in connection order
- Winit backend ignores `[[output]]` (always one virtual output)
- Add to `config.example.toml` with commented-out examples

### Phase 3: Input routing

The pointer moves across outputs. Determine which output it's on.

- `active_output(&self) -> Output` — find which output contains the current
  pointer position (screen coordinates)
- `output_at_screen_pos(&self, pos) -> Option<Output>` — helper
- Update all `// single-output assumption` sites in `input/mod.rs`:
  - `position_transformed` → use the output the pointer event came from
  - Pointer clamping → clamp within output boundaries, handle crossing
  - Layer surface lookup → check layers on the output the pointer is on
- Update `pointer.rs` context detection → use active output
- Update `gestures.rs` → momentum/pan affects active output only
- Update `grabs/move_grab.rs` → edge detection uses active output size

**Edge-zone monitor boundaries**: The existing edge-pan zone doubles as a sticky
boundary between adjacent monitors. Slow movement triggers edge-pan, fast
movement breaks through to the adjacent monitor. Add a velocity threshold. When
dragging a window across the boundary, adjust the window's canvas position to
stay under the cursor relative to the new viewport's canvas space.

### Phase 4: Multiple output creation (udev backend)

Currently `udev.rs` creates one output on the first connected connector. Extend:

- On startup: enumerate all connected connectors, create an output for each
- Apply `[[output]]` config (scale, transform, mode, position) per connector
- Each output gets its own `DrmCompositor` surface
- Each output gets its own VBlank handler — `frame_submitted()` is already
  per-CRTC via `active_crtcs`
- Output layout: compute non-overlapping positions. `position = "auto"` arranges
  left-to-right sorted by connector name.
- Initialize `OutputState` for each new output (camera at center of output's
  viewport region, or at origin for the primary)

### Phase 5: Hotplug

Extend existing hotplug handling:

- Monitor connected → create output, apply config, add to space, initialize
  `OutputState`, trigger render
- Monitor disconnected → remove output from space, clean up `OutputState`, if
  pointer was on it move pointer to nearest output
- VT switch (suspend/resume) → re-enumerate connectors, reconcile

### Phase 6: wlr-output-management protocol

Implement `zwlr-output-management-unstable-v1`:

- Advertise current output configuration (name, mode list, current mode,
  position, scale, transform)
- Handle configuration requests (apply, test)
- Send configuration events on hotplug changes
- Check niri's implementation for reference

### Phase 7: Window placement and navigation updates

- New windows open at center of **active output's** viewport
- `center-nearest` direction search uses active output's viewport
- `zoom-to-fit` fits all windows within active output's viewport
- `home-toggle` returns active output to origin / zoom 1.0
- Layer shell: respect output selection from protocol, fall back to active output
- Foreign toplevel: activation pans the output closest to target window
- State file: save/restore per-output camera/zoom keyed by output name

### Files modified

- `src/state/mod.rs` — `OutputState` struct, extraction, accessors
- `src/state/animation.rs` — per-output animation ticks
- `src/state/navigation.rs` — active output awareness
- `src/state/fullscreen.rs` — per-output fullscreen
- `src/render.rs` — per-output camera/zoom in compose_frame
- `src/input/mod.rs` — pointer routing, output detection
- `src/input/pointer.rs` — active output for context
- `src/input/gestures.rs` — momentum/pan target active output
- `src/grabs/move_grab.rs` — edge detection per output
- `src/backend/udev.rs` — multi-connector, output config, hotplug
- `src/handlers/layer_shell.rs` — output selection
- `src/handlers/xdg_shell.rs` — window placement on active output
- `src/handlers/compositor.rs` — window centering on active output
- `src/handlers/mod.rs` — wlr-output-management delegates
- `src/config/mod.rs` — output config parsing
- `src/config/toml.rs` — `[[output]]` serde
- `src/config/types.rs` — OutputConfig type
- `config.example.toml` — output configuration docs

### Verification

After each phase:
1. `cargo build` — clean compile
2. `cargo test` — all tests pass
3. `cargo clippy` — no warnings
4. Single monitor: everything works as before (regression test)

After Phase 4+:
5. Connect second monitor → appears with independent viewport
6. Pan/zoom on one monitor doesn't affect the other
7. Pointer moves between monitors
8. New windows open on the monitor with pointer focus
9. `wdisplays` can see and rearrange outputs (after Phase 6)
