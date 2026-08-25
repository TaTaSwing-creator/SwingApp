use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    fs::File,
    io::{
        BufReader,
        BufWriter,
    },
    path::PathBuf,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

use anyhow::Context;
use imgui::Key;
use serde::{
    Deserialize,
    Serialize,
};
use serde_with::with_prefix;
use utils_state::{
    State,
    StateCacheType,
};

use super::{
    Color,
    EspConfig,
    EspPlayerSettings,
    EspSelector,
    HotKey,
};

fn bool_true() -> bool {
    true
}
fn bool_false() -> bool {
    false
}
fn default_u32<const V: u32>() -> u32 {
    V
}
fn default_i32<const V: i32>() -> i32 {
    V
}
fn default_usize<const V: usize>() -> usize {
    V
}
fn default_f32<const N: usize, const D: usize>() -> f32 {
    N as f32 / D as f32
}
fn default_color<const R: u8, const G: u8, const B: u8, const A: u8>() -> Color {
    Color::from_u8([R, G, B, A])
}

fn default_key_settings() -> HotKey {
    Key::Pause.into()
}
fn default_key_trigger_bot() -> Option<HotKey> {
    Some(Key::MouseMiddle.into())
}
fn default_key_none() -> Option<HotKey> {
    None
}

fn default_esp_mode() -> KeyToggleMode {
    KeyToggleMode::AlwaysOn
}

fn default_trigger_bot_mode() -> KeyToggleMode {
    KeyToggleMode::Trigger
}

fn default_trigger_bot_weapon_settings() -> BTreeMap<TriggerBotWeaponCategory, TriggerBotWeaponSettings> {
    TriggerBotWeaponCategory::all()
        .into_iter()
        .map(|category| (category, TriggerBotWeaponSettings::default()))
        .collect()
}

fn default_rcs_weapon_settings() -> BTreeMap<TriggerBotWeaponCategory, RcsWeaponSettings> {
    TriggerBotWeaponCategory::all()
        .into_iter()
        .map(|category| (category, RcsWeaponSettings::default()))
        .collect()
}

fn default_rcs_mode() -> KeyToggleMode {
    KeyToggleMode::Off
}

fn default_esp_configs() -> BTreeMap<String, EspConfig> {
    let mut result: BTreeMap<String, EspConfig> = Default::default();
    result.insert(
        "player.enemy".to_string(),
        EspConfig::Player(EspPlayerSettings::new(&EspSelector::PlayerTeam {
            enemy: true,
        })),
    );
    result
}

fn default_esp_configs_enabled() -> BTreeMap<String, bool> {
    let mut result: BTreeMap<String, bool> = Default::default();
    result.insert("player.enemy".to_string(), true);
    result
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, PartialOrd)]
pub enum KeyToggleMode {
    AlwaysOn,
    Toggle,
    Trigger,
    TriggerInverted,
    Off,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TriggerBotWeaponCategory {
    Pistol,
    Deagle,
    Shotgun,
    Rifle,
    SMG,
    SSG08,
    AWP,
    #[serde(alias = "Sniper")]
    Other,
}

impl TriggerBotWeaponCategory {
    pub fn all() -> [Self; 8] {
        [
            Self::Pistol,
            Self::Deagle,
            Self::Shotgun,
            Self::Rifle,
            Self::SMG,
            Self::SSG08,
            Self::AWP,
            Self::Other,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Pistol => "Pistols",
            Self::Deagle => "Deagle",
            Self::Shotgun => "Shotguns",
            Self::Rifle => "Rifles",
            Self::SMG => "SMGs",
            Self::SSG08 => "SSG 08",
            Self::AWP => "AWP",
            Self::Other => "Other",
        }
    }

    pub fn from_weapon(weapon: cs2::WeaponId) -> Self {
        match weapon {
            cs2::WeaponId::Deagle => Self::Deagle,
            cs2::WeaponId::Ssg08 => Self::SSG08,
            cs2::WeaponId::AWP => Self::AWP,
            weapon if weapon.flags() & cs2::WEAPON_FLAG_TYPE_PISTOL != 0 => Self::Pistol,
            weapon if weapon.flags() & cs2::WEAPON_FLAG_TYPE_SHOTGUN != 0 => Self::Shotgun,
            weapon if weapon.flags() & cs2::WEAPON_FLAG_TYPE_SMG != 0 => Self::SMG,
            weapon if weapon.flags() & (cs2::WEAPON_FLAG_TYPE_RIFLE | cs2::WEAPON_FLAG_TYPE_MACHINE_GUN) != 0 => Self::Rifle,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct TriggerBotWeaponSettings {
    pub enabled: bool,
    pub delay_min: u32,
    pub delay_max: u32,
    pub shot_duration: u32,
}

impl Default for TriggerBotWeaponSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_min: 10,
            delay_max: 20,
            shot_duration: 400,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct RcsWeaponSettings {
    pub enabled: bool,
    pub strength: f32,
    pub delay: u32,
    pub smoothing: u32,
    pub jitter: bool,
    pub jitter_amount: f32,
}

impl Default for RcsWeaponSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 2.0,
            delay: 80,
            smoothing: 150,
            jitter: false,
            jitter_amount: 1.0,
        }
    }
}

impl KeyToggleMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AlwaysOn => "On",
            Self::Toggle => "Toggle",
            Self::Trigger => "Hold",
            Self::TriggerInverted => "Hold Inverted",
            Self::Off => "Off",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::AlwaysOn => "ON",
            Self::Toggle => "T",
            Self::Trigger => "H",
            Self::TriggerInverted => "HI",
            Self::Off => "OFF",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GrenadeType {
    Smoke,
    Molotov,
    Flashbang,
    Explosive,
}

impl GrenadeType {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Smoke => "Smoke",
            Self::Molotov => "Molotov",
            Self::Flashbang => "Flashbang",
            Self::Explosive => "Explosive",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GrenadeSortOrder {
    Alphabetical,
    AlphabeticalReverse,
}

impl GrenadeSortOrder {
    pub fn default() -> Self {
        Self::Alphabetical
    }

    pub fn sort(&self, values: &mut Vec<&GrenadeSpotInfo>) {
        match self {
            Self::Alphabetical => {
                values.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            }
            Self::AlphabeticalReverse => {
                values.sort_unstable_by(|a, b| b.name.cmp(&a.name));
            }
        }
    }
}

static GRENADE_SPOT_ID_INDEX: AtomicUsize = AtomicUsize::new(1);
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct GrenadeSpotInfo {
    #[serde(skip, default = "GrenadeSpotInfo::new_id")]
    pub id: usize,
    pub grenade_types: Vec<GrenadeType>,

    pub name: String,
    pub description: String,

    /// The eye position of the player
    pub eye_position: [f32; 3],
    pub eye_direction: [f32; 2],
}
impl GrenadeSpotInfo {
    pub fn new_id() -> usize {
        GRENADE_SPOT_ID_INDEX.fetch_add(1, Ordering::Relaxed)
    }
}
#[derive(Clone, Deserialize, Serialize)]
pub struct GrenadeSettings {
    #[serde(default = "bool_true")]
    pub active: bool,

    #[serde(default = "GrenadeSortOrder::default")]
    pub ui_sort_order: GrenadeSortOrder,

    #[serde(default = "default_f32::<150, 1>")]
    pub circle_distance: f32,

    #[serde(default = "default_f32::<20, 1>")]
    pub circle_radius: f32,

    #[serde(default = "default_usize::<32>")]
    pub circle_segments: usize,

    #[serde(default = "default_f32::<1, 10>")]
    pub angle_threshold_yaw: f32,

    #[serde(default = "default_f32::<5, 10>")]
    pub angle_threshold_pitch: f32,

    #[serde(default = "default_color::<255, 255, 255, 255>")]
    pub color_position: Color,

    #[serde(default = "default_color::<0, 255, 0, 255>")]
    pub color_position_active: Color,

    #[serde(default = "default_color::<255, 0, 0, 255>")]
    pub color_angle: Color,

    #[serde(default = "default_color::<0, 255, 0, 255>")]
    pub color_angle_active: Color,

    #[serde(default)]
    pub map_spots: HashMap<String, Vec<GrenadeSpotInfo>>,

    #[serde(default = "bool_true")]
    pub grenade_background: bool,
}

with_prefix!(serde_prefix_grenade_helper "grenade_helper");

#[derive(Clone, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default = "default_key_settings")]
    pub key_settings: HotKey,

    #[serde(default = "default_esp_mode")]
    pub esp_mode: KeyToggleMode,

    #[serde(default = "default_key_none")]
    pub esp_toggle: Option<HotKey>,

    #[serde(default = "default_esp_configs")]
    pub esp_settings: BTreeMap<String, EspConfig>,

    #[serde(default = "default_esp_configs_enabled")]
    pub esp_settings_enabled: BTreeMap<String, bool>,

    #[serde(default = "bool_true")]
    pub bomb_timer: bool,

    #[serde(default = "default_f32::<1730, 2560>")]
    pub bomb_timer_position_x: f32,

    #[serde(default = "default_f32::<4, 1000>")]
    pub bomb_timer_position_y: f32,

    #[serde(skip)]
    pub bomb_timer_edit_mode: bool,

    #[serde(default = "bool_true")]
    pub bomb_label: bool,

    #[serde(default = "bool_false")]
    pub spectators_list: bool,

    #[serde(default = "bool_true")]
    pub valthrun_watermark: bool,

    #[serde(default = "bool_true")]
    pub watermark_show_fps: bool,

    #[serde(default = "bool_true")]
    pub watermark_show_map: bool,

    #[serde(default = "bool_true")]
    pub watermark_show_trigger: bool,

    #[serde(default = "bool_true")]
    pub watermark_show_rcs: bool,

    #[serde(default = "bool_true")]
    pub watermark_show_esp: bool,

    #[serde(skip)]
    pub esp_active: bool,

    #[serde(skip)]
    pub trigger_bot_active: bool,

    #[serde(skip)]
    pub rcs_active: bool,

    #[serde(default = "default_i32::<16364>")]
    pub mouse_x_360: i32,

    #[serde(default = "default_trigger_bot_mode")]
    pub trigger_bot_mode: KeyToggleMode,

    #[serde(default = "default_trigger_bot_weapon_settings")]
    pub trigger_bot_weapon_settings: BTreeMap<TriggerBotWeaponCategory, TriggerBotWeaponSettings>,

    #[serde(default = "default_key_trigger_bot")]
    pub key_trigger_bot: Option<HotKey>,

    #[serde(default = "bool_true")]
    pub trigger_bot_team_check: bool,

    #[serde(default = "default_u32::<10>")]
    pub trigger_bot_delay_min: u32,

    #[serde(default = "default_u32::<20>")]
    pub trigger_bot_delay_max: u32,

    #[serde(default = "default_u32::<400>")]
    pub trigger_bot_shot_duration: u32,

    #[serde(default = "bool_false")]
    pub trigger_bot_check_target_after_delay: bool,

    #[serde(default = "default_rcs_mode")]
    pub rcs_mode: KeyToggleMode,

    #[serde(default = "default_rcs_weapon_settings")]
    pub rcs_weapon_settings: BTreeMap<TriggerBotWeaponCategory, RcsWeaponSettings>,

    #[serde(default = "default_key_none")]
    pub key_rcs: Option<HotKey>,

    #[serde(default = "default_f32::<2, 1>")]
    pub rcs_strength: f32,

    #[serde(default = "default_u32::<80>")]
    pub rcs_delay: u32,

    #[serde(default = "default_u32::<150>")]
    pub rcs_smoothing: u32,

    #[serde(default = "bool_false")]
    pub rcs_jitter: bool,

    #[serde(default = "default_f32::<1, 1>")]
    pub rcs_jitter_amount: f32,

    #[serde(default = "bool_false")]
    pub aim_assist_recoil: bool,

    #[serde(default = "bool_false")]
    pub aim_assist: bool,

    #[serde(default = "default_f32::<5, 1>")]
    pub aim_assist_fov: f32,

    #[serde(default = "default_u32::<1>")]
    pub aim_assist_recoil_min_bullets: u32,

    #[serde(default = "bool_true")]
    pub hide_overlay_from_screen_capture: bool,

    #[serde(default = "bool_false")]
    pub auto_accept: bool,

    #[serde(default = "bool_false")]
    pub radar: bool,

    #[serde(default = "default_f32::<35, 100>")]
    pub radar_opacity: f32,

    #[serde(default = "default_f32::<220, 1>")]
    pub radar_size: f32,

    #[serde(default = "default_f32::<1800, 1>")]
    pub radar_range: f32,

    #[serde(default = "default_f32::<10, 1>")]
    pub radar_position_x: f32,

    #[serde(default = "default_f32::<10, 1>")]
    pub radar_position_y: f32,

    #[serde(default = "bool_false")]
    pub anti_afk: bool,

    #[serde(default = "default_u32::<25>")]
    pub anti_afk_interval: u32,

    #[serde(default = "default_u32::<8>")]
    pub anti_afk_move_pixels: u32,

    #[serde(default = "bool_true")]
    pub anti_afk_use_keyboard: bool,

    #[serde(default = "bool_false")]
    pub render_debug_window: bool,

    #[serde(default = "bool_true")]
    pub metrics: bool,

    #[serde(default = "bool_true")]
    pub sniper_crosshair: bool,

    #[serde(default = "bool_false")]
    pub crosshair: bool,

    #[serde(default = "default_f32::<8, 1>")]
    pub crosshair_gap: f32,

    #[serde(default = "default_f32::<12, 1>")]
    pub crosshair_length: f32,

    #[serde(default = "default_f32::<2, 1>")]
    pub crosshair_thickness: f32,

    #[serde(default = "default_color::<255, 0, 0, 255>")]
    pub crosshair_color: Color,

    #[serde(default = "bool_true")]
    pub crosshair_outline: bool,

    #[serde(default = "bool_true")]
    pub crosshair_lines: bool,

    #[serde(default = "bool_false")]
    pub crosshair_center_dot: bool,

    #[serde(default = "default_f32::<3, 2>")]
    pub crosshair_dot_size: f32,

    #[serde(default)]
    pub crosshair_t_style: bool,

    #[serde(default = "default_f32::<1, 1>")]
    pub crosshair_opacity: f32,

    #[serde(flatten, with = "serde_prefix_grenade_helper")]
    pub grenade_helper: GrenadeSettings,

    #[serde(default)]
    pub imgui: Option<String>,
}

impl State for AppSettings {
    type Parameter = ();

    fn cache_type() -> StateCacheType {
        StateCacheType::Persistent
    }
}

pub fn get_settings_path() -> anyhow::Result<PathBuf> {
    let exe_file = std::env::current_exe().context("missing current exe path")?;
    let base_dir = exe_file.parent().context("could not get exe directory")?;

    Ok(base_dir.join("config.yaml"))
}

pub fn load_app_settings() -> anyhow::Result<AppSettings> {
    let config_path = get_settings_path()?;
    if !config_path.is_file() {
        log::info!(
            "App config file {} does not exist.",
            config_path.to_string_lossy()
        );
        log::info!("Using default config.");
        let config: AppSettings =
            serde_yaml::from_str("").context("failed to parse empty config")?;

        return Ok(config);
    }

    let config = File::open(&config_path).with_context(|| {
        format!(
            "failed to open app config at {}",
            config_path.to_string_lossy()
        )
    })?;
    let mut config = BufReader::new(config);

    let config: AppSettings =
        serde_yaml::from_reader(&mut config).context("failed to parse app config")?;

    log::info!("Loaded app config from {}", config_path.to_string_lossy());
    Ok(config)
}

pub fn save_app_settings(settings: &AppSettings) -> anyhow::Result<()> {
    let config_path = get_settings_path()?;
    let config = File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&config_path)
        .with_context(|| {
            format!(
                "failed to open app config at {}",
                config_path.to_string_lossy()
            )
        })?;
    let mut config = BufWriter::new(config);

    serde_yaml::to_writer(&mut config, settings).context("failed to serialize config")?;

    log::debug!("Saved app config.");
    Ok(())
}
