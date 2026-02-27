use std::collections::HashMap;
use std::f64::consts::FRAC_1_SQRT_2;

use serde::Deserialize;
use smithay::input::keyboard::{Keysym, ModifiersState, keysyms, xkb};

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

#[derive(Clone, Debug, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl Direction {
    /// Normalized direction vector for this direction.
    pub fn to_unit_vec(&self) -> (f64, f64) {
        match self {
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
            Direction::UpLeft => (-FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
            Direction::UpRight => (FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
            Direction::DownLeft => (-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
            Direction::DownRight => (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Exec(String),
    CloseWindow,
    NudgeWindow(Direction),
    PanViewport(Direction),
    CenterWindow,
    CenterNearest(Direction),
    CycleWindows { backward: bool },
    HomeToggle,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomToFit,
    ToggleFullscreen,
}

impl Action {
    /// Actions that should auto-repeat when their key is held.
    pub fn is_repeatable(&self) -> bool {
        matches!(
            self,
            Action::ZoomIn
                | Action::ZoomOut
                | Action::NudgeWindow(_)
                | Action::PanViewport(_)
                | Action::CycleWindows { .. }
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

/// Which physical key acts as the window-manager modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModKey {
    Alt,
    Super,
}

impl ModKey {
    /// Base modifier pattern with only the WM mod key set.
    fn base(self) -> Modifiers {
        match self {
            ModKey::Alt => Modifiers {
                alt: true,
                ..Modifiers::EMPTY
            },
            ModKey::Super => Modifiers {
                logo: true,
                ..Modifiers::EMPTY
            },
        }
    }

    /// Check if this mod key is pressed in the given modifier state.
    pub fn is_pressed(self, state: &ModifiersState) -> bool {
        match self {
            ModKey::Alt => state.alt,
            ModKey::Super => state.logo,
        }
    }
}

/// Which modifier must be held during window cycling (Alt-Tab style).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleModifier {
    Alt,
    Ctrl,
}

impl CycleModifier {
    pub fn is_pressed(self, state: &ModifiersState) -> bool {
        match self {
            CycleModifier::Alt => state.alt,
            CycleModifier::Ctrl => state.ctrl,
        }
    }

    fn base(self) -> Modifiers {
        match self {
            CycleModifier::Alt => Modifiers {
                alt: true,
                ..Modifiers::EMPTY
            },
            CycleModifier::Ctrl => Modifiers {
                ctrl: true,
                ..Modifiers::EMPTY
            },
        }
    }
}

impl Modifiers {
    const EMPTY: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        logo: false,
    };

    fn from_state(state: &ModifiersState) -> Self {
        Self {
            ctrl: state.ctrl,
            alt: state.alt,
            shift: state.shift,
            logo: state.logo,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub modifiers: Modifiers,
    pub sym: Keysym,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MouseTrigger {
    Button(u32),
    Scroll,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MouseBinding {
    pub modifiers: Modifiers,
    pub trigger: MouseTrigger,
}

#[derive(Clone, Debug)]
pub enum MouseAction {
    MoveWindow,
    ResizeWindow,
    PanViewport,
    Zoom,
}

/// Built-in dot grid shader — used when no shader_path or tile_path is configured.
pub const DEFAULT_SHADER: &str = include_str!("../assets/shaders/dot_grid.glsl");

#[derive(Clone, Debug, Default)]
pub struct BackgroundConfig {
    /// Path to a GLSL fragment shader. If set, shader is compiled and rendered fullscreen.
    pub shader_path: Option<String>,
    /// Path to a tile image (PNG/JPG). If set, image is tiled across the canvas.
    pub tile_path: Option<String>,
}

pub struct Config {
    pub mod_key: ModKey,
    /// Multiplier for scroll deltas. Higher = faster initial scroll. 1.0 = raw trackpad.
    pub scroll_speed: f64,
    /// Scroll momentum decay factor per frame. 0.92 = snappy, 0.96 = floaty.
    pub friction: f64,
    /// Pixels per keyboard nudge (Mod+Shift+Arrow).
    pub nudge_step: i32,
    /// Pixels per keyboard pan (Mod+Ctrl+Arrow).
    pub pan_step: f64,
    /// Keyboard repeat delay (ms) and rate (keys/sec).
    pub repeat_delay: i32,
    pub repeat_rate: i32,
    /// Edge auto-pan: activation zone width in pixels from viewport edge.
    pub edge_zone: f64,
    /// Edge auto-pan: speed range (px/frame). Quadratic ramp from min to max.
    pub edge_pan_min: f64,
    pub edge_pan_max: f64,
    /// Base lerp factor for camera animation (frame-rate independent). 0.15 = smooth.
    pub animation_speed: f64,
    /// Modifier held during window cycling. Release commits selection.
    pub cycle_modifier: CycleModifier,
    /// Zoom step multiplier per keypress. 1.1 = 10% per press.
    pub zoom_step: f64,
    /// Padding (canvas pixels) around the bounding box for ZoomToFit.
    pub zoom_fit_padding: f64,
    pub background: BackgroundConfig,
    bindings: HashMap<KeyCombo, Action>,
    pub mouse_bindings: HashMap<MouseBinding, MouseAction>,
}

impl Config {
    pub fn lookup(&self, modifiers: &ModifiersState, sym: Keysym) -> Option<&Action> {
        let combo = KeyCombo {
            modifiers: Modifiers::from_state(modifiers),
            sym,
        };
        self.bindings.get(&combo)
    }

    /// Look up a mouse button action by modifier state and button code.
    pub fn mouse_button_lookup(
        &self,
        modifiers: &ModifiersState,
        button: u32,
    ) -> Option<&MouseAction> {
        let binding = MouseBinding {
            modifiers: Modifiers::from_state(modifiers),
            trigger: MouseTrigger::Button(button),
        };
        self.mouse_bindings.get(&binding)
    }

    /// Look up a mouse scroll action by modifier state.
    pub fn mouse_scroll_lookup(&self, modifiers: &ModifiersState) -> Option<&MouseAction> {
        let binding = MouseBinding {
            modifiers: Modifiers::from_state(modifiers),
            trigger: MouseTrigger::Scroll,
        };
        self.mouse_bindings.get(&binding)
    }

    /// Parse a TOML string into a Config. Useful for testing and config reload.
    /// Does NOT set env vars (unlike `load()`).
    pub fn from_toml(toml_str: &str) -> Result<Self, toml::de::Error> {
        let raw: ConfigFile = toml::from_str(toml_str)?;
        Ok(Self::from_raw(raw))
    }

    /// Load config from `$XDG_CONFIG_HOME/driftwm/config.toml` (or `~/.config/driftwm/config.toml`).
    /// Missing file → all defaults. Parse failure → error log + all defaults.
    pub fn load() -> Self {
        let config_path = config_path();
        let raw = match std::fs::read_to_string(&config_path) {
            Ok(contents) => {
                tracing::info!("Loaded config from {}", config_path.display());
                match toml::from_str::<ConfigFile>(&contents) {
                    Ok(cf) => cf,
                    Err(e) => {
                        tracing::error!("Failed to parse config: {e}");
                        ConfigFile::default()
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("No config file found, using defaults");
                ConfigFile::default()
            }
            Err(e) => {
                tracing::error!("Failed to read config: {e}");
                ConfigFile::default()
            }
        };
        // Set cursor env vars before building config (unsafe — process-wide mutation,
        // only safe at startup before threads are spawned)
        if let Some(ref theme) = raw.cursor.theme {
            unsafe { std::env::set_var("XCURSOR_THEME", theme) };
        }
        if let Some(size) = raw.cursor.size {
            unsafe { std::env::set_var("XCURSOR_SIZE", size.to_string()) };
        }

        Self::from_raw(raw)
    }

    /// Build a Config from a parsed (but unvalidated) ConfigFile.
    /// Does not set env vars — that's done in `load()` only.
    fn from_raw(raw: ConfigFile) -> Self {
        let mod_key = match raw.mod_key.as_deref() {
            Some("alt") => ModKey::Alt,
            Some("super") | None => ModKey::Super,
            Some(other) => {
                tracing::warn!("Unknown mod_key '{other}', using super");
                ModKey::Super
            }
        };

        let cycle_modifier = match raw.cycle_modifier.as_deref() {
            Some("ctrl") => CycleModifier::Ctrl,
            Some("alt") | None => CycleModifier::Alt,
            Some(other) => {
                tracing::warn!("Unknown cycle_modifier '{other}', using alt");
                CycleModifier::Alt
            }
        };

        let mut bindings = default_bindings(mod_key, cycle_modifier);

        if let Some(user_bindings) = raw.keybindings {
            for (key_str, action_str) in &user_bindings {
                match parse_key_combo(key_str, mod_key) {
                    Ok(combo) => {
                        if action_str == "none" {
                            bindings.remove(&combo);
                        } else {
                            match parse_action(action_str) {
                                Ok(action) => {
                                    bindings.insert(combo, action);
                                }
                                Err(e) => {
                                    tracing::warn!("Invalid action '{action_str}': {e}");
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!("Invalid key combo '{key_str}': {e}"),
                }
            }
        }

        let mut mouse_bindings = default_mouse_bindings(mod_key);
        if let Some(user_mouse) = raw.mouse {
            for (key_str, action_str) in &user_mouse {
                match parse_mouse_binding(key_str, mod_key) {
                    Ok(binding) => {
                        if action_str == "none" {
                            mouse_bindings.remove(&binding);
                        } else {
                            match parse_mouse_action(action_str) {
                                Ok(action) => {
                                    mouse_bindings.insert(binding, action);
                                }
                                Err(e) => {
                                    tracing::warn!("Invalid mouse action '{action_str}': {e}");
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!("Invalid mouse binding '{key_str}': {e}"),
                }
            }
        }

        let background = BackgroundConfig {
            shader_path: raw.background.shader_path.map(|p| expand_tilde(&p)),
            tile_path: raw.background.tile_path.map(|p| expand_tilde(&p)),
        };

        Self {
            mod_key,
            scroll_speed: raw.input.scroll.speed.unwrap_or(1.5),
            friction: raw.input.scroll.friction.unwrap_or(0.96),
            nudge_step: raw.navigation.nudge_step.unwrap_or(20),
            pan_step: raw.navigation.pan_step.unwrap_or(100.0),
            repeat_delay: raw.input.keyboard.repeat_delay.unwrap_or(200),
            repeat_rate: raw.input.keyboard.repeat_rate.unwrap_or(25),
            edge_zone: raw.navigation.edge_pan.zone.unwrap_or(100.0),
            edge_pan_min: raw.navigation.edge_pan.speed_min.unwrap_or(4.0),
            edge_pan_max: raw.navigation.edge_pan.speed_max.unwrap_or(30.0),
            animation_speed: raw.navigation.animation_speed.unwrap_or(0.3),
            cycle_modifier,
            zoom_step: raw.zoom.step.unwrap_or(1.1),
            zoom_fit_padding: raw.zoom.fit_padding.unwrap_or(100.0),
            background,
            bindings,
            mouse_bindings,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_raw(ConfigFile::default())
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    mod_key: Option<String>,
    cycle_modifier: Option<String>,
    input: InputConfig,
    cursor: CursorConfig,
    navigation: NavigationConfig,
    zoom: ZoomConfig,
    background: BackgroundFileConfig,
    keybindings: Option<HashMap<String, String>>,
    mouse: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct InputConfig {
    keyboard: KeyboardConfig,
    scroll: ScrollConfig,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct KeyboardConfig {
    repeat_rate: Option<i32>,
    repeat_delay: Option<i32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ScrollConfig {
    speed: Option<f64>,
    friction: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct CursorConfig {
    theme: Option<String>,
    size: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct NavigationConfig {
    animation_speed: Option<f64>,
    nudge_step: Option<i32>,
    pan_step: Option<f64>,
    edge_pan: EdgePanConfig,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct EdgePanConfig {
    zone: Option<f64>,
    speed_min: Option<f64>,
    speed_max: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ZoomConfig {
    step: Option<f64>,
    fit_padding: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct BackgroundFileConfig {
    shader_path: Option<String>,
    tile_path: Option<String>,
}

/// Parse modifier names from string parts.
/// "Mod" expands to the configured mod_key. Literal Alt/Super/Ctrl/Shift also work.
fn parse_modifiers(parts: &[&str], mod_key: ModKey) -> Result<Modifiers, String> {
    let mut mods = Modifiers::EMPTY;
    for part in parts {
        match part.to_lowercase().as_str() {
            "mod" => match mod_key {
                ModKey::Alt => mods.alt = true,
                ModKey::Super => mods.logo = true,
            },
            "alt" => mods.alt = true,
            "super" | "logo" => mods.logo = true,
            "ctrl" | "control" => mods.ctrl = true,
            "shift" => mods.shift = true,
            other => return Err(format!("unknown modifier: {other}")),
        }
    }
    Ok(mods)
}

/// Parse a key combo string like "Mod+Shift+Up" into a KeyCombo.
pub fn parse_key_combo(s: &str, mod_key: ModKey) -> Result<KeyCombo, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty key combo".to_string());
    }

    let (keysym_name, modifier_parts) = parts.split_last().unwrap();
    let mods = parse_modifiers(modifier_parts, mod_key)?;

    let sym = xkb::keysym_from_name(keysym_name, xkb::KEYSYM_CASE_INSENSITIVE);
    if sym.raw() == keysyms::KEY_NoSymbol {
        return Err(format!("unknown keysym: {keysym_name}"));
    }

    Ok(KeyCombo {
        modifiers: mods,
        sym,
    })
}

/// Parse a mouse binding string like "Mod+Shift+Left" into a MouseBinding.
/// Last segment is the trigger: Left, Right, Middle, Scroll.
pub fn parse_mouse_binding(s: &str, mod_key: ModKey) -> Result<MouseBinding, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty mouse binding".to_string());
    }

    let (trigger_name, modifier_parts) = parts.split_last().unwrap();
    let mods = parse_modifiers(modifier_parts, mod_key)?;

    let trigger = match trigger_name.to_lowercase().as_str() {
        "left" => MouseTrigger::Button(BTN_LEFT),
        "right" => MouseTrigger::Button(BTN_RIGHT),
        "middle" => MouseTrigger::Button(BTN_MIDDLE),
        "scroll" => MouseTrigger::Scroll,
        other => return Err(format!("unknown mouse trigger: {other}")),
    };

    Ok(MouseBinding {
        modifiers: mods,
        trigger,
    })
}

/// Parse a keyboard action string like "exec foot" or "center-nearest up".
pub fn parse_action(s: &str) -> Result<Action, String> {
    let s = s.trim();
    let (name, arg) = match s.split_once(char::is_whitespace) {
        Some((n, a)) => (n, Some(a.trim())),
        None => (s, None),
    };
    match name {
        "exec" => {
            let cmd = arg.ok_or("exec requires a command argument")?;
            Ok(Action::Exec(cmd.to_string()))
        }
        "close-window" => Ok(Action::CloseWindow),
        "nudge-window" => {
            let dir = parse_direction(arg.ok_or("nudge-window requires a direction")?)?;
            Ok(Action::NudgeWindow(dir))
        }
        "pan-viewport" => {
            let dir = parse_direction(arg.ok_or("pan-viewport requires a direction")?)?;
            Ok(Action::PanViewport(dir))
        }
        "center-window" => Ok(Action::CenterWindow),
        "center-nearest" => {
            let dir = parse_direction(arg.ok_or("center-nearest requires a direction")?)?;
            Ok(Action::CenterNearest(dir))
        }
        "cycle-windows" => {
            let dir_str = arg.ok_or("cycle-windows requires forward or backward")?;
            match dir_str {
                "forward" => Ok(Action::CycleWindows { backward: false }),
                "backward" => Ok(Action::CycleWindows { backward: true }),
                other => Err(format!("cycle-windows: expected forward or backward, got '{other}'")),
            }
        }
        "home-toggle" => Ok(Action::HomeToggle),
        "zoom-in" => Ok(Action::ZoomIn),
        "zoom-out" => Ok(Action::ZoomOut),
        "zoom-reset" => Ok(Action::ZoomReset),
        "zoom-to-fit" => Ok(Action::ZoomToFit),
        "toggle-fullscreen" => Ok(Action::ToggleFullscreen),
        other => Err(format!("unknown action: {other}")),
    }
}

/// Parse a mouse action string like "move-window" or "zoom".
pub fn parse_mouse_action(s: &str) -> Result<MouseAction, String> {
    match s.trim() {
        "move-window" => Ok(MouseAction::MoveWindow),
        "resize-window" => Ok(MouseAction::ResizeWindow),
        "pan-viewport" => Ok(MouseAction::PanViewport),
        "zoom" => Ok(MouseAction::Zoom),
        other => Err(format!("unknown mouse action: {other}")),
    }
}

/// Parse a direction string (case-insensitive).
pub fn parse_direction(s: &str) -> Result<Direction, String> {
    match s.trim().to_lowercase().as_str() {
        "up" => Ok(Direction::Up),
        "down" => Ok(Direction::Down),
        "left" => Ok(Direction::Left),
        "right" => Ok(Direction::Right),
        "up-left" => Ok(Direction::UpLeft),
        "up-right" => Ok(Direction::UpRight),
        "down-left" => Ok(Direction::DownLeft),
        "down-right" => Ok(Direction::DownRight),
        other => Err(format!("unknown direction: {other}")),
    }
}

fn default_bindings(mod_key: ModKey, cycle_mod: CycleModifier) -> HashMap<KeyCombo, Action> {
    let terminal = detect_terminal();
    let launcher = detect_launcher();
    tracing::info!("Terminal command: {terminal}");
    tracing::info!("Launcher command: {launcher}");

    let m = mod_key.base();
    let m_shift = Modifiers {
        shift: true,
        ..m.clone()
    };
    let m_ctrl = Modifiers {
        ctrl: true,
        ..m.clone()
    };
    let cyc = cycle_mod.base();
    let cyc_shift = Modifiers {
        shift: true,
        ..cyc.clone()
    };

    HashMap::from([
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_Return),
            },
            Action::Exec(terminal),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_d),
            },
            Action::Exec(launcher),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_q),
            },
            Action::CloseWindow,
        ),
        (
            KeyCombo {
                modifiers: m_shift.clone(),
                sym: Keysym::from(keysyms::KEY_Up),
            },
            Action::NudgeWindow(Direction::Up),
        ),
        (
            KeyCombo {
                modifiers: m_shift.clone(),
                sym: Keysym::from(keysyms::KEY_Down),
            },
            Action::NudgeWindow(Direction::Down),
        ),
        (
            KeyCombo {
                modifiers: m_shift.clone(),
                sym: Keysym::from(keysyms::KEY_Left),
            },
            Action::NudgeWindow(Direction::Left),
        ),
        (
            KeyCombo {
                modifiers: m_shift,
                sym: Keysym::from(keysyms::KEY_Right),
            },
            Action::NudgeWindow(Direction::Right),
        ),
        (
            KeyCombo {
                modifiers: m_ctrl.clone(),
                sym: Keysym::from(keysyms::KEY_Up),
            },
            Action::PanViewport(Direction::Up),
        ),
        (
            KeyCombo {
                modifiers: m_ctrl.clone(),
                sym: Keysym::from(keysyms::KEY_Down),
            },
            Action::PanViewport(Direction::Down),
        ),
        (
            KeyCombo {
                modifiers: m_ctrl.clone(),
                sym: Keysym::from(keysyms::KEY_Left),
            },
            Action::PanViewport(Direction::Left),
        ),
        (
            KeyCombo {
                modifiers: m_ctrl,
                sym: Keysym::from(keysyms::KEY_Right),
            },
            Action::PanViewport(Direction::Right),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_a),
            },
            Action::HomeToggle,
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_c),
            },
            Action::CenterWindow,
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_Up),
            },
            Action::CenterNearest(Direction::Up),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_Down),
            },
            Action::CenterNearest(Direction::Down),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_Left),
            },
            Action::CenterNearest(Direction::Left),
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_Right),
            },
            Action::CenterNearest(Direction::Right),
        ),
        (
            KeyCombo {
                modifiers: cyc,
                sym: Keysym::from(keysyms::KEY_Tab),
            },
            Action::CycleWindows { backward: false },
        ),
        (
            KeyCombo {
                modifiers: cyc_shift,
                sym: Keysym::from(keysyms::KEY_ISO_Left_Tab),
            },
            Action::CycleWindows { backward: true },
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_equal),
            },
            Action::ZoomIn,
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_minus),
            },
            Action::ZoomOut,
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_0),
            },
            Action::ZoomReset,
        ),
        (
            KeyCombo {
                modifiers: m.clone(),
                sym: Keysym::from(keysyms::KEY_w),
            },
            Action::ZoomToFit,
        ),
        (
            KeyCombo {
                modifiers: m,
                sym: Keysym::from(keysyms::KEY_f),
            },
            Action::ToggleFullscreen,
        ),
    ])
}

fn default_mouse_bindings(mod_key: ModKey) -> HashMap<MouseBinding, MouseAction> {
    let m = mod_key.base();
    let m_shift = Modifiers {
        shift: true,
        ..m.clone()
    };

    HashMap::from([
        (
            MouseBinding {
                modifiers: m_shift.clone(),
                trigger: MouseTrigger::Button(BTN_LEFT),
            },
            MouseAction::MoveWindow,
        ),
        (
            MouseBinding {
                modifiers: m_shift,
                trigger: MouseTrigger::Button(BTN_RIGHT),
            },
            MouseAction::ResizeWindow,
        ),
        (
            MouseBinding {
                modifiers: m.clone(),
                trigger: MouseTrigger::Button(BTN_LEFT),
            },
            MouseAction::PanViewport,
        ),
        (
            MouseBinding {
                modifiers: m,
                trigger: MouseTrigger::Scroll,
            },
            MouseAction::Zoom,
        ),
    ])
}

fn config_path() -> std::path::PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.config")
    });
    std::path::PathBuf::from(config_dir).join("driftwm/config.toml")
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{home}/{rest}");
    }
    path.to_string()
}

fn detect_launcher() -> String {
    if let Ok(launcher) = std::env::var("LAUNCHER")
        && !launcher.is_empty()
    {
        return launcher;
    }
    for cmd in ["fuzzel", "wofi", "bemenu-run", "tofi"] {
        if std::process::Command::new("which")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return cmd.to_string();
        }
    }
    "fuzzel".to_string()
}

fn detect_terminal() -> String {
    if let Ok(term) = std::env::var("TERMINAL")
        && !term.is_empty()
    {
        return term;
    }
    for cmd in ["foot", "alacritty", "ptyxis", "kitty", "wezterm"] {
        if std::process::Command::new("which")
            .arg(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return cmd.to_string();
        }
    }
    "foot".to_string()
}
