use std::{
    borrow::Cow,
    collections::{
        btree_map::Entry,
        HashMap,
    },
    fs::File,
    io::{
        BufReader,
        Write,
    },
    path::PathBuf,
    sync::{
        atomic::Ordering,
        Arc,
        Mutex,
    },
    thread,
};

use anyhow::Context;
use cs2::StateCurrentMap;
use imgui::{
    Condition,
    ImColor32,
    SelectableFlags,
    StyleColor,
    StyleVar,
    TableColumnSetup,
    TableFlags,
    TreeNodeFlags,
    WindowFlags,
};
use obfstr::obfstr;
use overlay::UnicodeTextRenderer;
use utils_state::StateRegistry;

use super::{
    Color,
    EspColor,
    EspColorType,
    EspConfig,
    EspSelector,
    GrenadeSettings,
    GrenadeSortOrder,
    GrenadeSpotInfo,
    GrenadeType,
    KeyToggleMode,
    TriggerBotWeaponCategory,
};
use crate::{
    enhancements::StateGrenadeHelperPlayerLocation,
    settings::{
        AppSettings,
        EspBoxType,
        EspHeadDot,
        EspHealthBar,
        EspInfoPosition,
        EspPlayerSettings,
        load_app_settings,
        save_app_settings,
    },
    utils::{
        ImGuiKey,
        ImguiComboEnum,
        open_url,
    },
    Application,
};

struct StyleSlider<'a> {
    min: f32,
    max: f32,
    value: &'a mut f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavigationSection {
    TriggerBot,
    Visuals,
    GrenadeHelper,
    Misc,
    Crosshair,
    Hud,
    Info,
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum GrenadeSettingsTarget {
    General,
    MapType(String),
    Map {
        map_name: String,
        display_name: String,
    },
}

impl GrenadeSettingsTarget {
    pub fn ui_token(&self) -> Cow<'static, str> {
        match self {
            Self::General => "_settings".into(),
            Self::MapType(value) => format!("map_type_{}", value).into(),
            Self::Map { map_name: name, .. } => format!("map_{}", name).into(),
        }
    }

    pub fn ident_level(&self) -> usize {
        match self {
            Self::General => 0,
            Self::MapType(_) => 0,
            Self::Map { .. } => 1,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum GrenadeHelperTransferDirection {
    Export,
    Import,
}

enum GrenadeHelperTransferState {
    /// Currently no transfer in progress
    Idle,
    /// A new transfer should be initiated.
    Pending {
        #[allow(dead_code)]
        direction: GrenadeHelperTransferDirection,
    },
    /// A transfer has been initiated.
    /// This might be either an export or import.
    Active {
        #[allow(dead_code)]
        direction: GrenadeHelperTransferDirection,
    },
    /// The current transfer failed.
    Failed {
        #[allow(dead_code)]
        direction: GrenadeHelperTransferDirection,
        message: String,
    },
    /// The source file has been loaded.
    /// Prompting the user, if he wants to replace or add the new items.
    ImportPending {
        elements: HashMap<String, Vec<GrenadeSpotInfo>>,
    },
    ImportSuccess {
        count: usize,
        #[allow(dead_code)]
        replacing: bool,
    },
    ExportSuccess {
        target_path: PathBuf,
    },
}

pub struct SettingsUI {
    theme_accent: [f32; 4],
    theme_accent_hovered: [f32; 4],
    theme_surface: [f32; 4],
    theme_frame: [f32; 4],

    esp_selected_target: EspSelector,
    esp_pending_target: Option<EspSelector>,
    grenade_helper_target: GrenadeSettingsTarget,
    grenade_helper_selected_id: usize,
    grenade_helper_skip_confirmation_dialog: bool,
    grenade_helper_new_item: Option<GrenadeSpotInfo>,
    grenade_helper_transfer_state: Arc<Mutex<GrenadeHelperTransferState>>,

    grenade_helper_pending_target: Option<GrenadeSettingsTarget>,
    grenade_helper_pending_selected_id: Option<usize>,

    active_navigation: NavigationSection,
    trigger_bot_weapon_category: TriggerBotWeaponCategory,
}

impl SettingsUI {
    pub fn new() -> Self {
        Self {
            theme_accent: [0.72, 0.04, 0.04, 1.0],
            theme_accent_hovered: [0.90, 0.06, 0.06, 1.0],
            theme_surface: [0.13, 0.13, 0.15, 1.0],
            theme_frame: [0.12, 0.12, 0.14, 1.0],

            esp_selected_target: EspSelector::PlayerTeam { enemy: true },
            esp_pending_target: None,
            grenade_helper_target: GrenadeSettingsTarget::General,
            grenade_helper_selected_id: 0,
            grenade_helper_new_item: None,
            grenade_helper_skip_confirmation_dialog: false,
            grenade_helper_transfer_state: Arc::new(Mutex::new(GrenadeHelperTransferState::Idle)),

            grenade_helper_pending_target: None,
            grenade_helper_pending_selected_id: None,

            active_navigation: NavigationSection::Visuals,
            trigger_bot_weapon_category: TriggerBotWeaponCategory::Pistol,
        }
    }

    pub fn render(
        &mut self,
        app: &Application,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
    ) {
        let content_font = ui.current_font().id();
        let theme_accent = self.theme_accent;
        let theme_accent_hovered = self.theme_accent_hovered;
        let theme_surface = self.theme_surface;
        let theme_frame = self.theme_frame;

        let _window_bg = ui.push_style_color(StyleColor::WindowBg, [0.025, 0.025, 0.028, 1.0]);
        let _header = ui.push_style_color(StyleColor::Header, theme_accent);
        let _header_hovered = ui.push_style_color(StyleColor::HeaderHovered, theme_accent_hovered);
        let _header_active = ui.push_style_color(StyleColor::HeaderActive, [theme_accent_hovered[0], theme_accent_hovered[1], theme_accent_hovered[2], 1.0]);
        let _button = ui.push_style_color(StyleColor::Button, theme_surface);
        let _button_hovered = ui.push_style_color(StyleColor::ButtonHovered, [theme_accent[0] * 0.65, theme_accent[1] * 0.65, theme_accent[2] * 0.65, 1.0]);
        let _button_active = ui.push_style_color(StyleColor::ButtonActive, theme_accent);
        let _frame_bg = ui.push_style_color(StyleColor::FrameBg, theme_frame);
        let _frame_bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, [theme_frame[0] + 0.07, theme_frame[1] + 0.07, theme_frame[2] + 0.07, 1.0]);
        let _frame_bg_active = ui.push_style_color(StyleColor::FrameBgActive, [0.25, 0.05, 0.05, 1.0]);
        let _title_bg = ui.push_style_color(StyleColor::TitleBg, [0.16, 0.02, 0.02, 1.0]);
        let _title_bg_active = ui.push_style_color(StyleColor::TitleBgActive, [0.55, 0.03, 0.03, 1.0]);
        let _title_bg_collapsed = ui.push_style_color(StyleColor::TitleBgCollapsed, [0.10, 0.015, 0.015, 1.0]);
        let _slider_grab = ui.push_style_color(StyleColor::SliderGrab, [0.90, 0.05, 0.05, 1.0]);
        let _slider_grab_active = ui.push_style_color(StyleColor::SliderGrabActive, [1.00, 0.00, 0.00, 1.0]);
        let _check_mark = ui.push_style_color(StyleColor::CheckMark, [1.00, 0.00, 0.00, 1.0]);
        let _text = ui.push_style_color(StyleColor::Text, [0.94, 0.94, 0.94, 1.0]);
        let _text_disabled = ui.push_style_color(StyleColor::TextDisabled, [0.60, 0.60, 0.60, 1.0]);
        let _child_bg = ui.push_style_color(StyleColor::ChildBg, [0.045, 0.045, 0.050, 1.0]);
        let _popup_bg = ui.push_style_color(StyleColor::PopupBg, [0.08, 0.08, 0.08, 1.0]);
        let _separator = ui.push_style_color(StyleColor::Separator, [0.24, 0.24, 0.24, 1.0]);
        let _plot_histogram = ui.push_style_color(StyleColor::PlotHistogram, [1.00, 0.00, 0.00, 1.0]);
        let _plot_histogram_hovered = ui.push_style_color(StyleColor::PlotHistogramHovered, [1.00, 0.15, 0.15, 1.0]);
        let _window_padding = ui.push_style_var(StyleVar::WindowPadding([18.0, 4.0]));
        let _window_title_align = ui.push_style_var(StyleVar::WindowTitleAlign([0.5, 0.5]));
        let _item_spacing = ui.push_style_var(StyleVar::ItemSpacing([6.0, 5.0]));
        let _frame_padding = ui.push_style_var(StyleVar::FramePadding([6.0, 4.0]));
        let _window_rounding = ui.push_style_var(StyleVar::WindowRounding(8.0));
        let _frame_rounding = ui.push_style_var(StyleVar::FrameRounding(6.0));
        let _child_rounding = ui.push_style_var(StyleVar::ChildRounding(6.0));

        ui.window("SwingApp")
            .size([960.0, 440.0], Condition::Always)
            .size_constraints([960.0, 440.0], [960.0, 440.0])
            .title_bar(true)
            .flags(WindowFlags::NO_RESIZE | WindowFlags::NO_COLLAPSE)
            .build(|| {
                let _content_font = ui.push_font(content_font);
                let mut settings = app.settings_mut();

                let nav_items = [
                    ("Trigger Bot", NavigationSection::TriggerBot),
                    ("Visuals", NavigationSection::Visuals),
                    ("Grenade Helper", NavigationSection::GrenadeHelper),
                    ("Misc", NavigationSection::Misc),
                    ("Crosshair", NavigationSection::Crosshair),
                    ("HUD", NavigationSection::Hud),
                    ("Info", NavigationSection::Info),
                ];

                {
                    let _nav_spacing = ui.push_style_var(StyleVar::ItemSpacing([4.0, 4.0]));
                    let _nav_rounding = ui.push_style_var(StyleVar::FrameRounding(5.0));
                    let _nav_padding = ui.push_style_var(StyleVar::FramePadding([6.0, 4.0]));
                    ui.dummy([0.0, 0.0]);

                    let nav_spacing = 6.0;
                    let tab_width = ((ui.content_region_avail()[0]
                        - nav_spacing * (nav_items.len() as f32 - 1.0))
                        / nav_items.len() as f32)
                        .max(110.0);
                    for (index, (label, section)) in nav_items.into_iter().enumerate() {
                        if index > 0 {
                            ui.same_line();
                        }

                        let is_selected = self.active_navigation == section;
                        let (button_color, button_hovered, button_active, text_color) = if is_selected {
                            (theme_accent, theme_accent_hovered, theme_accent_hovered, [1.0, 1.0, 1.0, 1.0])
                        } else {
                            (theme_surface, [theme_surface[0] + 0.045, theme_surface[1] + 0.045, theme_surface[2] + 0.05, 1.0], theme_accent, [0.72, 0.72, 0.77, 1.0])
                        };

                        let _button = ui.push_style_color(StyleColor::Button, button_color);
                        let _button_hovered = ui.push_style_color(StyleColor::ButtonHovered, button_hovered);
                        let _button_active = ui.push_style_color(StyleColor::ButtonActive, button_active);
                        let _text = ui.push_style_color(StyleColor::Text, text_color);

                        if ui.button_with_size(label, [tab_width, 32.0]) {
                            self.active_navigation = section;
                        }

                        if is_selected {
                            ui.set_item_default_focus();
                        }
                    }
                }

                ui.dummy([0.0, 0.0]);
                ui.separator();
                ui.dummy([0.0, 2.0]);

                ui.child_window("##content")
                    .border(false)
                    .scroll_bar(!matches!(self.active_navigation, NavigationSection::Info))
                    .build(|| {
                        match self.active_navigation {
                            NavigationSection::Crosshair => {
                                ui.set_window_font_scale(1.1);
                                let _crosshair_spacing = ui.push_style_var(StyleVar::ItemSpacing([10.0, 12.0]));
                                let _crosshair_padding = ui.push_style_var(StyleVar::FramePadding([7.0, 5.0]));
                                let content_width = ui.content_region_avail()[0];
                                let controls_width = (content_width * 0.45).max(380.0);
                                let preview_width = (content_width * 0.27).max(240.0);
                                let advanced_width = (content_width - controls_width - preview_width - 12.0).max(200.0);
                                let _compact_spacing = ui.push_style_var(StyleVar::ItemSpacing([5.0, 1.0]));
                                let _compact_padding = ui.push_style_var(StyleVar::FramePadding([5.0, 2.0]));

                                if let Some(_controls) = ui
                                    .child_window("##crosshair_controls")
                                    .size([controls_width, 0.0])
                                    .border(false)
                                    .scroll_bar(false)
                                    .begin()
                                {
                                    ui.text_colored([0.92, 0.92, 0.96, 1.0], "SHAPE");
                                    ui.separator();

                                    let mut crosshair_color = settings.crosshair_color.as_f32();
                                    ui.text("Color");
                                    ui.same_line_with_spacing(0.0, 24.0);
                                    if ui.color_edit4_config("##crosshair_color", &mut crosshair_color)
                                        .alpha_bar(true)
                                        .inputs(true)
                                        .label(false)
                                        .build()
                                    {
                                        settings.crosshair_color = Color::from_f32(crosshair_color);
                                    }
                                    ui.dummy([0.0, 5.0]);

                                    ui.text("Gap");
                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.set_next_item_width(210.0);
                                    ui.slider_config("##crosshair_gap", 0.0, 30.0).display_format("%.1f").build(&mut settings.crosshair_gap);
                                    ui.dummy([0.0, 7.0]);
                                    ui.text("Length");
                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.set_next_item_width(210.0);
                                    ui.slider_config("##crosshair_length", 1.0, 40.0).display_format("%.1f").build(&mut settings.crosshair_length);
                                    ui.dummy([0.0, 7.0]);
                                    ui.text("Thickness");
                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.set_next_item_width(210.0);
                                    ui.slider_config("##crosshair_thickness", 1.0, 8.0).display_format("%.1f").build(&mut settings.crosshair_thickness);
                                    ui.dummy([0.0, 7.0]);
                                    ui.checkbox(obfstr!("Center dot"), &mut settings.crosshair_center_dot);
                                    ui.text("Dot size");
                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.set_next_item_width(210.0);
                                    ui.slider_config("##crosshair_dot_size", 1.0, 8.0).display_format("%.1f").build(&mut settings.crosshair_dot_size);
                                    ui.dummy([0.0, 5.0]);

                                    ui.separator();
                                    ui.text_colored([0.72, 0.73, 0.78, 1.0], "Profile");
                                    if ui.button_with_size("Classic", [92.0, 0.0]) {
                                        settings.crosshair_gap = 8.0;
                                        settings.crosshair_length = 12.0;
                                        settings.crosshair_thickness = 2.0;
                                        settings.crosshair_center_dot = false;
                                        settings.crosshair_t_style = false;
                                        settings.crosshair_opacity = 1.0;
                                    }
                                    ui.same_line();
                                    if ui.button_with_size("Minimal", [92.0, 0.0]) {
                                        settings.crosshair_gap = 3.0;
                                        settings.crosshair_length = 6.0;
                                        settings.crosshair_thickness = 1.0;
                                        settings.crosshair_color = Color::from_f32([0.2, 1.0, 0.6, 1.0]);
                                        settings.crosshair_outline = false;
                                        settings.crosshair_center_dot = false;
                                        settings.crosshair_dot_size = 1.5;
                                        settings.crosshair_t_style = false;
                                        settings.crosshair_opacity = 1.0;
                                    }
                                    ui.same_line();
                                    if ui.button_with_size("Reset", [92.0, 0.0]) {
                                        settings.crosshair_gap = 8.0;
                                        settings.crosshair_length = 12.0;
                                        settings.crosshair_thickness = 2.0;
                                        settings.crosshair_color = Color::from_f32([1.0, 0.0, 0.0, 1.0]);
                                        settings.crosshair_outline = true;
                                        settings.crosshair_lines = true;
                                        settings.crosshair_center_dot = false;
                                        settings.crosshair_dot_size = 1.5;
                                        settings.crosshair_t_style = false;
                                        settings.crosshair_opacity = 1.0;
                                    }
                                    ui.separator();
                                    if ui.button("Save") {
                                        if let Err(error) = save_app_settings(&settings) {
                                            log::warn!("Failed to save crosshair settings: {:#}", error);
                                        }
                                    }
                                    ui.same_line();
                                    if ui.button("Load") {
                                        match load_app_settings() {
                                            Ok(loaded) => *settings = loaded,
                                            Err(error) => log::warn!("Failed to load settings: {:#}", error),
                                        }
                                    }
                                    ui.same_line();
                                }

                                ui.same_line_with_spacing(0.0, 6.0);
                                if let Some(_preview) = ui
                                    .child_window("##crosshair_preview")
                                    .size([preview_width, 0.0])
                                    .border(false)
                                    .scroll_bar(false)
                                    .begin()
                                {
                                    let preview_origin = ui.cursor_screen_pos();
                                    let preview_region = ui.content_region_avail();
                                    let preview_end = [
                                        preview_origin[0] + preview_region[0],
                                        preview_origin[1] + preview_region[1].max(210.0),
                                    ];
                                    ui.get_window_draw_list()
                                        .add_rect(preview_origin, preview_end, [0.12, 0.13, 0.16, 1.0])
                                        .filled(true)
                                        .rounding(6.0)
                                        .build();
                                    ui.text_colored([0.92, 0.92, 0.96, 1.0], "LIVE PREVIEW");
                                    ui.same_line();
                                    ui.text_colored(
                                        if settings.crosshair || settings.sniper_crosshair {
                                            [0.25, 0.95, 0.62, 1.0]
                                        } else {
                                            [0.65, 0.66, 0.70, 1.0]
                                        },
                                        if settings.crosshair || settings.sniper_crosshair { "ACTIVE" } else { "OFF" },
                                    );
                                    ui.dummy([0.0, 12.0]);

                                    let preview_size = ui.content_region_avail();
                                    let center = [
                                        preview_origin[0] + preview_size[0] / 2.0,
                                        preview_origin[1] + preview_size[1] / 2.0,
                                    ];
                                    let mut color = settings.crosshair_color.as_f32();
                                    color[3] *= settings.crosshair_opacity.clamp(0.1, 1.0);
                                    let gap = settings.crosshair_gap.max(0.0);
                                    let length = settings.crosshair_length.max(1.0);
                                    let thickness = settings.crosshair_thickness.max(1.0);
                                    let outline_width = if settings.crosshair_outline { thickness + 2.0 } else { 0.0 };
                                    let draw = ui.get_window_draw_list();

                                    draw.add_circle(center, 42.0, [0.45, 0.47, 0.52, 0.45])
                                        .thickness(1.0)
                                        .build();
                                    draw.add_circle(center, 84.0, [0.35, 0.37, 0.42, 0.35])
                                        .thickness(1.0)
                                        .build();

                                    if settings.crosshair && settings.crosshair_lines {
                                        for (index, (start, end)) in [
                                            ([center[0] - gap - length, center[1]], [center[0] - gap, center[1]]),
                                            ([center[0] + gap, center[1]], [center[0] + gap + length, center[1]]),
                                            ([center[0], center[1] - gap - length], [center[0], center[1] - gap]),
                                            ([center[0], center[1] + gap], [center[0], center[1] + gap + length]),
                                        ].into_iter().enumerate() {
                                            if settings.crosshair_t_style && index == 2 {
                                                continue;
                                            }
                                            if settings.crosshair_outline {
                                                draw.add_line(start, end, [0.0, 0.0, 0.0, color[3]])
                                                    .thickness(outline_width)
                                                    .build();
                                            }
                                            draw.add_line(start, end, color)
                                                .thickness(thickness)
                                                .build();
                                        }
                                    }

                                    if settings.crosshair && settings.crosshair_center_dot {
                                        let dot_size = settings.crosshair_dot_size.max(2.0);
                                        if settings.crosshair_outline {
                                            draw.add_circle(center, dot_size + 1.0, [0.0, 0.0, 0.0, color[3]])
                                                .filled(true)
                                                .build();
                                        }
                                        draw.add_circle(center, dot_size, color)
                                            .filled(true)
                                            .build();
                                    }

                                    if settings.sniper_crosshair {
                                        draw.add_circle(center, 3.5, [0.0, 0.0, 0.0, 0.8])
                                            .filled(true)
                                            .build();
                                        draw.add_circle(center, 2.0, [1.0, 1.0, 1.0, 0.8])
                                            .filled(true)
                                            .build();
                                    }

                                    ui.dummy([0.0, preview_size[1].max(210.0)]);
                                }

                                ui.same_line_with_spacing(0.0, 6.0);
                                if let Some(_advanced) = ui
                                    .child_window("##crosshair_advanced")
                                    .size([advanced_width, 0.0])
                                    .border(false)
                                    .scroll_bar(false)
                                    .begin()
                                {
                                    ui.text_colored([0.92, 0.92, 0.96, 1.0], "CROSSHAIR SETUP");
                                    ui.separator();
                                    if ui.checkbox(obfstr!("Crosshair"), &mut settings.crosshair)
                                        && settings.crosshair
                                    {
                                        settings.sniper_crosshair = false;
                                    }
                                    ui.dummy([0.0, 3.0]);
                                    if ui.checkbox(obfstr!("Sniper Crosshair"), &mut settings.sniper_crosshair)
                                        && settings.sniper_crosshair
                                    {
                                        settings.crosshair = false;
                                    }
                                    ui.dummy([0.0, 3.0]);
                                    ui.checkbox(obfstr!("Lines"), &mut settings.crosshair_lines);
                                    ui.dummy([0.0, 3.0]);
                                    ui.checkbox(obfstr!("Outline"), &mut settings.crosshair_outline);
                                    ui.dummy([0.0, 5.0]);
                                    ui.separator();
                                    ui.text_colored([0.92, 0.92, 0.96, 1.0], "CROSSHAIR OPTIONS");
                                    ui.separator();
                                    ui.checkbox(obfstr!("T-style"), &mut settings.crosshair_t_style);
                                    ui.set_next_item_width(180.0);
                                    ui.slider_config("Opacity", 0.1, 1.0)
                                        .build(&mut settings.crosshair_opacity);
                                    ui.dummy([0.0, 4.0]);
                                    ui.separator();
                                    ui.text_colored([0.72, 0.73, 0.78, 1.0], "TIP");
                                    ui.text_wrapped("Use a small gap and thin lines for a clean competitive profile.");
                                }
                                ui.set_window_font_scale(1.0);
                            }
                            NavigationSection::TriggerBot => {
                                ui.set_next_item_width(170.0);
                                ui.combo_enum(obfstr!("Trigger Bot"), &[
                                    (KeyToggleMode::Off, "Off"),
                                    (KeyToggleMode::Trigger, "Hold"),
                                    (KeyToggleMode::TriggerInverted, "Hold Inverted"),
                                    (KeyToggleMode::Toggle, "Toggle"),
                                    (KeyToggleMode::AlwaysOn, "On"),
                                ], &mut settings.trigger_bot_mode);

                                if !matches!(settings.trigger_bot_mode, KeyToggleMode::Off | KeyToggleMode::AlwaysOn) {
                                    ui.button_key_optional(obfstr!("Trigger bot key"), &mut settings.key_trigger_bot, [150.0, 0.0]);
                                    ui.same_line_with_spacing(0.0, 16.0);
                                }
                                ui.set_next_item_width(170.0);
                                ui.combo_enum(
                                    "Weapon category",
                                    &TriggerBotWeaponCategory::all().map(|category| (category, category.display_name())),
                                    &mut self.trigger_bot_weapon_category,
                                );
                                if !matches!(settings.trigger_bot_mode, KeyToggleMode::Off) {
                                    let weapon_settings = settings
                                        .trigger_bot_weapon_settings
                                        .entry(self.trigger_bot_weapon_category)
                                        .or_default();
                                    ui.checkbox("Enabled", &mut weapon_settings.enabled);
                                    let mut values_updated = false;
                                    let slider_width = ((ui.content_region_avail()[0] - 170.0) / 3.0)
                                        .min(320.0)
                                        .max(160.0);

                                    ui.align_text_to_frame_padding();
                                    ui.text("Min");
                                    ui.same_line();
                                    ui.set_next_item_width(slider_width);
                                    values_updated |= ui.slider_config("##delay_min", 0, 300).display_format("%dms").build(&mut weapon_settings.delay_min);

                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.align_text_to_frame_padding();
                                    ui.text("Max");
                                    ui.same_line();
                                    ui.set_next_item_width(slider_width);
                                    values_updated |= ui.slider_config("##delay_max", 0, 300).display_format("%dms").build(&mut weapon_settings.delay_max);

                                    ui.same_line_with_spacing(0.0, 16.0);
                                    ui.align_text_to_frame_padding();
                                    ui.text("Duration");
                                    ui.same_line();
                                    ui.set_next_item_width(slider_width);
                                    values_updated |= ui.slider_config("##shoot_duration", 0, 1000).display_format("%dms").build(&mut weapon_settings.shot_duration);

                                    if values_updated {
                                        let delay_min = weapon_settings.delay_min.min(weapon_settings.delay_max);
                                        let delay_max = weapon_settings.delay_min.max(weapon_settings.delay_max);

                                        weapon_settings.delay_min = delay_min;
                                        weapon_settings.delay_max = delay_max;
                                    }

                                    ui.checkbox("Aim Assist Recoil", &mut settings.aim_assist_recoil);
                                    ui.same_line_with_spacing(0.0, 12.0);
                                    ui.text("Minimum bullets");
                                    ui.same_line();
                                    ui.set_next_item_width(120.0);
                                    ui.slider_config("##aim_assist_recoil_min_bullets", 1, 10)
                                        .display_format("%d")
                                        .build(&mut settings.aim_assist_recoil_min_bullets);
                                    ui.checkbox(obfstr!("Retest trigger target after delay"), &mut settings.trigger_bot_check_target_after_delay);
                                    ui.checkbox(obfstr!("Team Check"), &mut settings.trigger_bot_team_check);
                                    ui.separator();
                                }

                                ui.separator();
                                ui.set_next_item_width(170.0);
                                ui.combo_enum(obfstr!("RCS Basic"), &[
                                    (KeyToggleMode::Off, "Off"),
                                    (KeyToggleMode::Trigger, "Hold"),
                                    (KeyToggleMode::TriggerInverted, "Hold Inverted"),
                                    (KeyToggleMode::Toggle, "Toggle"),
                                    (KeyToggleMode::AlwaysOn, "On"),
                                ], &mut settings.rcs_mode);

                                if !matches!(settings.rcs_mode, KeyToggleMode::Off | KeyToggleMode::AlwaysOn) {
                                    ui.button_key_optional(obfstr!("RCS key"), &mut settings.key_rcs, [150.0, 0.0]);
                                    ui.same_line_with_spacing(0.0, 16.0);
                                }

                                let rcs_settings = settings
                                    .rcs_weapon_settings
                                    .entry(self.trigger_bot_weapon_category)
                                    .or_default();
                                ui.checkbox("RCS Enabled", &mut rcs_settings.enabled);
                                ui.text("Strength");
                                ui.same_line();
                                ui.set_next_item_width(110.0);
                                ui.slider_config("##rcs_strength", 0.1, 5.0)
                                    .display_format("%.2f")
                                    .build(&mut rcs_settings.strength);

                                ui.same_line_with_spacing(0.0, 16.0);
                                ui.text("Delay");
                                ui.same_line();
                                ui.set_next_item_width(100.0);
                                ui.slider_config("##rcs_delay", 0, 300)
                                    .display_format("%dms")
                                    .build(&mut rcs_settings.delay);

                                ui.same_line_with_spacing(0.0, 16.0);
                                ui.text("Smoothing");
                                ui.same_line();
                                ui.set_next_item_width(110.0);
                                ui.slider_config("##rcs_smoothing", 25, 500)
                                    .display_format("%dms")
                                    .build(&mut rcs_settings.smoothing);
                                ui.same_line_with_spacing(0.0, 16.0);
                                ui.checkbox(obfstr!("Jitter"), &mut rcs_settings.jitter);
                                ui.same_line();
                                ui.set_next_item_width(80.0);
                                ui.slider_config("##rcs_jitter_amount", 0.0, 5.0)
                                    .display_format("%.1f")
                                    .build(&mut rcs_settings.jitter_amount);
                            }
                            NavigationSection::Visuals => {
                                ui.set_next_item_width(170.0);
                                ui.combo_enum(obfstr!("ESP"), &[
                                    (KeyToggleMode::Off, "Off"),
                                    (KeyToggleMode::Trigger, "Hold"),
                                    (KeyToggleMode::TriggerInverted, "Hold Inverted"),
                                    (KeyToggleMode::Toggle, "Toggle"),
                                    (KeyToggleMode::AlwaysOn, "On"),
                                ], &mut settings.esp_mode);

                                ui.same_line_with_spacing(0.0, 22.0);
                                {
                                    let _enabled = ui.begin_enabled(matches!(settings.esp_mode, KeyToggleMode::Toggle | KeyToggleMode::Trigger));
                                    ui.button_key_optional(obfstr!("Toggle/Hold"), &mut settings.esp_toggle, [160.0, 0.0]);
                                }

                                let player_key = EspSelector::Player.config_key();
                                let friendly_key = EspSelector::PlayerTeam { enemy: false }.config_key();
                                let enemy_key = EspSelector::PlayerTeam { enemy: true }.config_key();
                                let mut player_enabled = settings
                                    .esp_settings_enabled
                                    .get(&player_key)
                                    .cloned()
                                    .unwrap_or(false);
                                let mut friendly_enabled = settings
                                    .esp_settings_enabled
                                    .get(&friendly_key)
                                    .cloned()
                                    .unwrap_or(false);
                                let mut enemy_enabled = settings
                                    .esp_settings_enabled
                                    .get(&enemy_key)
                                    .cloned()
                                    .unwrap_or(false);

                                ui.same_line_with_spacing(0.0, 22.0);
                                if ui.checkbox(obfstr!("ESP Player"), &mut player_enabled) {
                                    settings.esp_settings_enabled.insert(player_key, player_enabled);
                                }
                                ui.same_line_with_spacing(0.0, 22.0);
                                if ui.checkbox(obfstr!("ESP Friendly"), &mut friendly_enabled) {
                                    settings.esp_settings_enabled.insert(friendly_key, friendly_enabled);
                                }
                                ui.same_line_with_spacing(0.0, 22.0);
                                if ui.checkbox(obfstr!("ESP Enemy"), &mut enemy_enabled) {
                                    settings.esp_settings_enabled.insert(enemy_key, enemy_enabled);
                                }

                                if settings.esp_mode == KeyToggleMode::Off {
                                    let _style = ui.push_style_color(StyleColor::Text, [1.0, 0.76, 0.03, 1.0]);
                                    ui.text(obfstr!("ESP has been disabled."));
                                    ui.text(obfstr!("Enable ESP to configure player and world visuals."));
                                } else {
                                    ui.new_line();
                                    self.render_esp_settings(&mut *settings, ui);
                                }
                            }
                            NavigationSection::GrenadeHelper => {
                                if settings.grenade_helper.active {
                                    self.render_grenade_helper(&app.app_state, &mut settings.grenade_helper, ui, unicode_text);
                                } else {
                                    let _style = ui.push_style_color(StyleColor::Text, [1.0, 0.76, 0.03, 1.0]);
                                    ui.text(obfstr!("Grenade Helper has been disabled."));
                                    ui.text(obfstr!("Enable the grenade helper to configure throw spots and filters."));
                                }

                                self.render_grenade_helper_transfer(&mut settings.grenade_helper, ui);
                            }
                            NavigationSection::Misc => {
                                ui.button_key_ignore_mouse_left(obfstr!("Toggle Settings"), &mut settings.key_settings, [160.0, 0.0]);
                                if ui.checkbox(obfstr!("Hide overlay from screen capture"), &mut settings.hide_overlay_from_screen_capture) {
                                    app.settings_screen_capture_changed.store(true, Ordering::Relaxed);
                                }

                                ui.checkbox(obfstr!("Auto Accept"), &mut settings.auto_accept);
                                ui.checkbox(obfstr!("Radar"), &mut settings.radar);
                                if settings.radar {
                                    ui.slider_config(obfstr!("Radar opacity"), 0.05, 1.0)
                                        .display_format("%.2f")
                                        .build(&mut settings.radar_opacity);
                                    ui.slider_config(obfstr!("Radar size"), 100.0, 400.0)
                                        .display_format("%.0fpx")
                                        .build(&mut settings.radar_size);
                                    ui.slider_config(obfstr!("Radar map zoom"), 500.0, 4000.0)
                                        .display_format("%.0f")
                                        .build(&mut settings.radar_range);
                                    if ui.button(obfstr!("Save Radar Settings")) {
                                        if let Err(error) = save_app_settings(&settings) {
                                            log::warn!("Failed to save radar settings: {:#}", error);
                                        }
                                    }
                                }
                                ui.checkbox(obfstr!("Anti AFK"), &mut settings.anti_afk);
                                if settings.anti_afk {
                                    ui.slider_config(obfstr!("Anti AFK interval"), 5, 120)
                                        .display_format("%ds")
                                        .build(&mut settings.anti_afk_interval);
                                    ui.slider_config(obfstr!("Anti AFK movement"), 1, 50)
                                        .display_format("%dpx")
                                        .build(&mut settings.anti_afk_move_pixels);
                                    ui.checkbox(
                                        obfstr!("Anti AFK keyboard input"),
                                        &mut settings.anti_afk_use_keyboard,
                                    );
                                }
                                ui.checkbox(obfstr!("Grenade Helper"), &mut settings.grenade_helper.active);
                                ui.separator();
                                if ui.button_with_size(obfstr!("Exit SwingApp"), [160.0, 0.0]) {
                                    app.request_exit();
                                }
                            }
                            NavigationSection::Hud => {
                                ui.checkbox(obfstr!("SwingApp Watermark"), &mut settings.valthrun_watermark);

                                ui.separator();
                                ui.text("Show in HUD");
                                ui.checkbox(obfstr!("FPS"), &mut settings.watermark_show_fps);
                                ui.same_line();
                                ui.checkbox(obfstr!("Map"), &mut settings.watermark_show_map);
                                ui.same_line();
                                ui.checkbox(obfstr!("Trigger"), &mut settings.watermark_show_trigger);
                                ui.same_line();
                                ui.checkbox(obfstr!("RCS"), &mut settings.watermark_show_rcs);
                                ui.same_line();
                                ui.checkbox(obfstr!("ESP"), &mut settings.watermark_show_esp);
                                ui.same_line();
                                if ui.checkbox(obfstr!("Debug Overlay"), &mut settings.render_debug_window) {
                                    app.settings_render_debug_window_changed
                                        .store(true, Ordering::Relaxed);
                                }

                                ui.separator();
                                ui.text_colored([0.72, 0.73, 0.78, 1.0], "RADAR SETUP");
                                ui.text("CS2 radar - values to configure in the game settings:");
                                ui.text("Centers The Player: YES | Is Rotating: NO");
                                ui.text("Map Blends With Background: YES | Blur Background: YES");
                                ui.text("Background Opacity: 1.00 | HUD Size: 0.95");
                                ui.text("Map Zoom: 0.36 | Alternate Zoom: 1.00");
                                ui.text("Square Shape: NO | Dynamic Zoom: NO");
                                ui.text("SwingApp radar - values:");
                                ui.text("Opacity: 0.05 | HUD Size: 0.95");
                                ui.text("Map Zoom: 0.36 | Alternate Zoom: 1.00");
                            }
                            NavigationSection::Info => {
                                ui.dummy([0.0, 12.0]);
                                ui.text("Community, project and credits");
                                ui.separator();

                                ui.text("Useful links");
                                if ui.button_with_size("Valthrun website", [180.0, 36.0]) {
                                    open_url("https://valth.run");
                                }
                                ui.same_line();
                                if ui.button_with_size("Project docs", [180.0, 36.0]) {
                                    open_url("https://wiki.valth.run/getting-started/");
                                }

                                ui.dummy([0.0, 16.0]);
                                ui.text("Thanks to Valthrun");
                                ui.text_wrapped(
                                    "SwingApp is built on top of the Valthrun open-source project. "
                                        .to_owned()
                                        + "Huge thanks to the Valthrun team and contributors for opening the project to the community. "
                                        + "That made it possible to learn from it, experiment, and build something new around it.",
                                );

                                ui.dummy([0.0, 16.0]);
                                ui.text("SwingApp community");
                                ui.text_wrapped(
                                    "Thank you for using SwingApp. Your support, feedback, and time are what keep the project moving. "
                                        .to_owned()
                                        + "I am glad you enjoy the application, and I hope it continues to be useful and fun to use.",
                                );
                                ui.dummy([0.0, 8.0]);
                                if ui.button_with_size("My Discord", [180.0, 36.0]) {
                                    open_url("https://discord.gg/Ugedf5Up6g");
                                }
                                ui.same_line();
                                if ui.button_with_size("My YouTube", [180.0, 36.0]) {
                                    open_url("https://www.youtube.com/@tataswing69");
                                }

                                ui.dummy([0.0, 18.0]);
                            }
                        }
                    });
            });
    }

    fn render_esp_target(
        &mut self,
        settings: &mut AppSettings,
        ui: &imgui::Ui,
        target: &EspSelector,
    ) {
        let config_key = target.config_key();
        let target_enabled = settings
            .esp_settings_enabled
            .get(&config_key)
            .cloned()
            .unwrap_or_default();

        let parent_enabled = target_enabled || {
            let mut current = target.parent();
            while let Some(parent) = current.take() {
                let enabled = settings
                    .esp_settings_enabled
                    .get(&parent.config_key())
                    .cloned()
                    .unwrap_or_default();

                if enabled {
                    current = Some(parent);
                    break;
                }

                current = parent.parent();
            }

            current.is_some()
        };

        {
            let pos_begin = ui.cursor_screen_pos();
            let clicked = ui
                .selectable_config(format!(
                    "{} ##{}",
                    target.config_display(),
                    target.config_key()
                ))
                .selected(target == &self.esp_selected_target)
                .flags(SelectableFlags::SPAN_ALL_COLUMNS)
                .build();

            let indicator_color = if target_enabled {
                ImColor32::from_rgb(0x4C, 0xAF, 0x50)
            } else if parent_enabled {
                ImColor32::from_rgb(0xFF, 0xC1, 0x07)
            } else {
                ImColor32::from_rgb(0xF4, 0x43, 0x36)
            };
            let pos_end = ui.cursor_screen_pos();
            let indicator_radius = ui.current_font_size() * 0.25;

            ui.get_window_draw_list()
                .add_circle(
                    [
                        pos_begin[0] - indicator_radius - 5.0,
                        pos_begin[1] + (pos_end[1] - pos_begin[1]) / 2.0 - indicator_radius / 2.0,
                    ],
                    indicator_radius,
                    indicator_color,
                )
                .filled(true)
                .build();

            if clicked {
                self.esp_pending_target = Some(target.clone());
            }
        }

        let children = target.children();
        if children.len() > 0 {
            ui.indent();
            for child in children.iter() {
                self.render_esp_target(settings, ui, child);
            }
            ui.unindent();
        }
    }

    fn render_esp_settings_player(
        &mut self,
        settings: &mut AppSettings,
        ui: &imgui::Ui,
        target: EspSelector,
    ) {
        let config_key = target.config_key();
        let config_enabled = settings
            .esp_settings_enabled
            .get(&config_key)
            .cloned()
            .unwrap_or_default();

        let config = match settings.esp_settings.entry(config_key.clone()) {
            Entry::Occupied(entry) => {
                let value = entry.into_mut();
                if let EspConfig::Player(value) = value {
                    value
                } else {
                    log::warn!("Detected invalid player config for {}", config_key);
                    *value = EspConfig::Player(EspPlayerSettings::new(&target));
                    if let EspConfig::Player(value) = value {
                        value
                    } else {
                        unreachable!()
                    }
                }
            }
            Entry::Vacant(entry) => {
                if let EspConfig::Player(value) =
                    entry.insert(EspConfig::Player(EspPlayerSettings::new(&target)))
                {
                    value
                } else {
                    unreachable!()
                }
            }
        };
        let mut reset_requested = false;
        let _ui_enable_token = ui.begin_enabled(config_enabled);

        const COMBO_WIDTH: f32 = 170.0;
        let features_width = (ui.content_region_avail()[0] - 280.0).max(400.0);
        let preview_origin = ui.cursor_screen_pos();
        let preview_origin = [preview_origin[0] + features_width + 16.0, preview_origin[1]];
        let preview_height = ui.content_region_avail()[1].max(260.0);
        let preview_end = [preview_origin[0] + 260.0, preview_origin[1] + preview_height];
        let draw = ui.get_window_draw_list();
        draw.add_rect(preview_origin, preview_end, [0.12, 0.13, 0.16, 1.0])
            .filled(true)
            .rounding(6.0)
            .build();
        draw.add_rect(preview_origin, preview_end, [0.02, 0.45, 0.95, 0.85])
            .thickness(1.5)
            .rounding(6.0)
            .build();
        draw.add_text(
            [preview_origin[0] + 12.0, preview_origin[1] + 10.0],
            [0.92, 0.92, 0.96, 1.0],
            "ESP PREVIEW",
        );
        let preview_center = [preview_origin[0] + 130.0, preview_origin[1] + preview_height * 0.55];
        let mut info_lines = [0usize; 4];
        let mut draw_preview_info = |position: EspInfoPosition, color: [f32; 4], text: &str| {
            let [text_width, _] = ui.calc_text_size(text);
            let (x, y, index) = match position {
                EspInfoPosition::Left => (preview_center[0] - 145.0, preview_center[1] - 58.0, 0),
                EspInfoPosition::Right => (preview_center[0] + 75.0, preview_center[1] - 58.0, 1),
                EspInfoPosition::Top => ((preview_center[0] * 2.0 - text_width) / 2.0, preview_center[1] - 112.0, 2),
                EspInfoPosition::Bottom => ((preview_center[0] * 2.0 - text_width) / 2.0, preview_center[1] + 130.0, 3),
            };
            let y = y + info_lines[index] as f32 * 18.0;
            draw.add_text([x, y], color, text);
            info_lines[index] += 1;
        };
        if config.head_dot != EspHeadDot::None {
            let head_dot = draw.add_circle(
                [preview_center[0], preview_center[1] - 82.0 - config.head_dot_z * 4.0],
                config.head_dot_base_radius.max(8.0) * 2.0,
                config.head_dot_color.calculate_color(1.0, 0.0),
            );
            match config.head_dot {
                EspHeadDot::Filled => head_dot.filled(true).build(),
                EspHeadDot::NotFilled => head_dot.thickness(config.head_dot_thickness.max(1.0)).build(),
                EspHeadDot::None => unreachable!(),
            }
        }
        if config.box_type != EspBoxType::None {
            let box_color = config.box_color.calculate_color(1.0, 0.0);
            let box_width = config.box_width.max(1.0);
            let front_min = [preview_center[0] - 72.0, preview_center[1] - 64.0];
            let front_max = [preview_center[0] + 72.0, preview_center[1] + 122.0];
            let draw_preview_corners = |min: [f32; 2], max: [f32; 2]| {
                let width = (max[0] - min[0]).abs();
                let height = (max[1] - min[1]).abs();
                let corner_width = (width * 0.28).min(32.0);
                let corner_height = (height * 0.18).min(32.0);
                for (start, end) in [
                    ([min[0], min[1]], [min[0] + corner_width, min[1]]),
                    ([min[0], min[1]], [min[0], min[1] + corner_height]),
                    ([max[0], min[1]], [max[0] - corner_width, min[1]]),
                    ([max[0], min[1]], [max[0], min[1] + corner_height]),
                    ([min[0], max[1]], [min[0] + corner_width, max[1]]),
                    ([min[0], max[1]], [min[0], max[1] - corner_height]),
                    ([max[0], max[1]], [max[0] - corner_width, max[1]]),
                    ([max[0], max[1]], [max[0], max[1] - corner_height]),
                ] {
                    draw.add_line(start, end, box_color)
                        .thickness(box_width)
                        .build();
                }
            };
            match config.box_type {
                EspBoxType::Box2D => {
                    if config.box_fill {
                        draw.add_rect(
                            front_min,
                            front_max,
                            config.box_fill_color.calculate_color(1.0, 0.0),
                        )
                        .filled(true)
                        .build();
                    }
                    if config.box_corners {
                        draw_preview_corners(front_min, front_max);
                    } else {
                        draw.add_rect(front_min, front_max, box_color)
                            .thickness(box_width)
                            .build();
                    }
                }
                EspBoxType::Box3D => {
                    let offset = [24.0, -18.0];
                    let rear_min = [front_min[0] + offset[0], front_min[1] + offset[1]];
                    let rear_max = [front_max[0] + offset[0], front_max[1] + offset[1]];
                    draw.add_rect(front_min, front_max, box_color)
                        .thickness(box_width)
                        .build();
                    draw.add_rect(rear_min, rear_max, box_color)
                        .thickness(box_width)
                        .build();
                    for (start, end) in [
                        ([front_min[0], front_min[1]], [rear_min[0], rear_min[1]]),
                        ([front_max[0], front_min[1]], [rear_max[0], rear_min[1]]),
                        ([front_min[0], front_max[1]], [rear_min[0], rear_max[1]]),
                        ([front_max[0], front_max[1]], [rear_max[0], rear_max[1]]),
                    ] {
                        draw.add_line(start, end, box_color)
                            .thickness(box_width)
                            .build();
                    }
                }
                EspBoxType::None => {}
            }
        }
        if config.skeleton {
            for (start, end) in [
                ([preview_center[0], preview_center[1] - 62.0], [preview_center[0], preview_center[1] + 45.0]),
                ([preview_center[0], preview_center[1] - 35.0], [preview_center[0] - 64.0, preview_center[1] + 18.0]),
                ([preview_center[0], preview_center[1] - 35.0], [preview_center[0] + 64.0, preview_center[1] + 18.0]),
                ([preview_center[0], preview_center[1] + 45.0], [preview_center[0] - 54.0, preview_center[1] + 118.0]),
                ([preview_center[0], preview_center[1] + 45.0], [preview_center[0] + 54.0, preview_center[1] + 118.0]),
            ] {
                draw.add_line(start, end, config.skeleton_color.calculate_color(1.0, 0.0))
                    .thickness(config.skeleton_width.max(1.0))
                    .build();
            }
        }
        if config.health_bar != EspHealthBar::None {
            let health_width = config.health_bar_width.max(3.0);
            let preview_health = 1.0_f32;
            let health_color = config.health_bar_color.calculate_color(preview_health, 0.0);
            let left = preview_center[0] - 72.0;
            let top = preview_center[1] - 64.0;
            let right = preview_center[0] + 72.0;
            let bottom = preview_center[1] + 122.0;
            let health_rect = match config.health_bar {
                EspHealthBar::Top => [[left, top], [right, top + health_width]],
                EspHealthBar::Left => [[left, top], [left + health_width, bottom]],
                EspHealthBar::Bottom => [[left, bottom - health_width], [right, bottom]],
                EspHealthBar::Right => [[right - health_width, top], [right, bottom]],
                EspHealthBar::None => unreachable!(),
            };
            draw.add_rect(health_rect[0], health_rect[1], [0.08, 0.08, 0.08, 0.9])
                .filled(true)
                .build();
            let health_rect = match config.health_bar {
                EspHealthBar::Top => [
                    health_rect[0],
                    [health_rect[0][0] + (health_rect[1][0] - health_rect[0][0]) * preview_health, health_rect[1][1]],
                ],
                EspHealthBar::Left => [
                    [health_rect[0][0], health_rect[1][1] - (health_rect[1][1] - health_rect[0][1]) * preview_health],
                    health_rect[1],
                ],
                EspHealthBar::Bottom => [
                    [health_rect[0][0] + (health_rect[1][0] - health_rect[0][0]) * (1.0 - preview_health), health_rect[0][1]],
                    health_rect[1],
                ],
                EspHealthBar::Right => [
                    health_rect[0],
                    [health_rect[1][0], health_rect[0][1] + (health_rect[1][1] - health_rect[0][1]) * preview_health],
                ],
                EspHealthBar::None => unreachable!(),
            };
            draw.add_rect(health_rect[0], health_rect[1], health_color)
                .filled(true)
                .build();
        }
        if config.info_name {
            draw_preview_info(
                config.info_name_position,
                config.info_name_color.calculate_color(1.0, 0.0),
                "Enemy",
            );
        }
        if config.info_weapon {
            draw_preview_info(
                config.info_weapon_position,
                config.info_weapon_color.calculate_color(1.0, 0.0),
                "AK-47",
            );
        }
        if config.info_ammo {
            draw_preview_info(
                config.info_ammo_position,
                config.info_ammo_color.calculate_color(1.0, 0.0),
                "30 / 90",
            );
        }
        if config.info_hp_text {
            draw_preview_info(
                config.info_hp_text_position,
                config.info_hp_text_color.calculate_color(1.0, 0.0),
                "100 HP",
            );
        }
        if config.info_distance {
            draw_preview_info(
                config.info_distance_position,
                config.info_distance_color.calculate_color(1.0, 0.0),
                "12m",
            );
        }
        if config.info_flag_kit {
            draw_preview_info(
                config.info_flags_position,
                config.info_flags_color.calculate_color(1.0, 0.0),
                "KIT",
            );
        }
        if config.info_flag_scoped {
            draw_preview_info(config.info_flags_position, config.info_flags_color.calculate_color(1.0, 0.0), "SCOPED");
        }
        if config.info_flag_flashed {
            draw_preview_info(config.info_flags_position, config.info_flags_color.calculate_color(1.0, 0.0), "FLASHED");
        }
        if config.info_flag_bomb {
            draw_preview_info(config.info_flags_position, config.info_flags_color.calculate_color(1.0, 0.0), "BOMB");
        }
        if config.info_grenades {
            draw_preview_info(config.info_grenades_position, config.info_grenades_color.calculate_color(1.0, 0.0), "GRENADE");
        }
        if settings.bomb_timer {
            draw_preview_info(config.info_grenades_position, config.info_grenades_color.calculate_color(1.0, 0.0), "TIMER");
        }
        if settings.spectators_list {
            draw_preview_info(config.info_flags_position, config.info_grenades_color.calculate_color(1.0, 0.0), "SPECTATORS");
        }
        if config.offscreen_arrows {
            draw_preview_info(EspInfoPosition::Right, config.offscreen_arrows_color.calculate_color(1.0, 0.0), "ARROWS");
        }

        if let Some(_features_grid) = ui.begin_table_with_sizing(
            "##visuals_grid",
            3,
            TableFlags::SIZING_STRETCH_PROP,
            [features_width, 0.0],
            0.0,
        ) {
            ui.text_disabled("Visuals");
            ui.table_next_row();
            unsafe {
                imgui::sys::igSetNextItemOpen(true, 0);
            }
            if true {
                ui.table_next_column();
                let mut box_enabled = !matches!(config.box_type, EspBoxType::None);
                if ui.checkbox("Box", &mut box_enabled) {
                    if box_enabled && matches!(config.box_type, EspBoxType::None) {
                        config.box_type = EspBoxType::Box2D;
                    } else if !box_enabled {
                        config.box_type = EspBoxType::None;
                    }
                }
                if let Some(_popup) = ui.begin_popup_context_with_label("##player_box_context") {
                    const ESP_BOX_TYPES: [(EspBoxType, &'static str); 3] = [
                        (EspBoxType::None, "Off"),
                        (EspBoxType::Box2D, "2D"),
                        (EspBoxType::Box3D, "3D"),
                    ];
                    ui.text("Player box");
                    ui.set_next_item_width(COMBO_WIDTH);
                    ui.combo_enum("##player_box_popup", &ESP_BOX_TYPES, &mut config.box_type);
                    ui.slider_config("Width", 1.0, 10.0)
                        .display_format("%.1f")
                        .build(&mut config.box_width);
                    ui.checkbox("Corners only", &mut config.box_corners);
                    Self::render_esp_color_popup(ui, "Color", &mut config.box_color);
                }

                ui.table_next_column();
                ui.checkbox("Box fill", &mut config.box_fill);
                if let Some(_popup) = ui.begin_popup_context_with_label("##player_box_fill_context") {
                    ui.checkbox("Enabled", &mut config.box_fill);
                    Self::render_esp_color_popup(ui, "Color", &mut config.box_fill_color);
                }

                ui.table_next_column();
                ui.checkbox("Box shadow", &mut config.box_shadow);

                ui.table_next_column();
                ui.checkbox("Skeleton", &mut config.skeleton);
                if let Some(_popup) = ui.begin_popup_context_with_label("##player_skeleton_context") {
                    ui.checkbox("Enabled", &mut config.skeleton);
                    ui.slider_config("Width", 1.0, 10.0)
                        .display_format("%.1f")
                        .build(&mut config.skeleton_width);
                    Self::render_esp_color_popup(ui, "Color", &mut config.skeleton_color);
                }

                ui.table_next_column();
                let mut head_dot_enabled = !matches!(config.head_dot, EspHeadDot::None);
                if ui.checkbox("Head dot", &mut head_dot_enabled) {
                    if head_dot_enabled && matches!(config.head_dot, EspHeadDot::None) {
                        config.head_dot = EspHeadDot::Filled;
                    } else if !head_dot_enabled {
                        config.head_dot = EspHeadDot::None;
                    }
                }
                if let Some(_popup) = ui.begin_popup_context_with_label("##head_dot_context") {
                    const HEAD_DOT_TYPES: [(EspHeadDot, &'static str); 3] = [
                        (EspHeadDot::None, "Off"),
                        (EspHeadDot::Filled, "Filled"),
                        (EspHeadDot::NotFilled, "Outlined"),
                    ];
                    ui.set_next_item_width(COMBO_WIDTH);
                    ui.combo_enum("##head_dot_popup", &HEAD_DOT_TYPES, &mut config.head_dot);
                    ui.slider_config("Thickness", 1.0, 10.0)
                        .display_format("%.1f")
                        .build(&mut config.head_dot_thickness);
                    ui.slider_config("Radius", 0.0, 10.0)
                        .display_format("%.1f")
                        .build(&mut config.head_dot_base_radius);
                    ui.slider_config("Height", 0.0, 10.0)
                        .display_format("%.1f")
                        .build(&mut config.head_dot_z);
                    Self::render_esp_color_popup(ui, "Color", &mut config.head_dot_color);
                }

                ui.table_next_column();
                let mut health_bar_enabled = !matches!(config.health_bar, EspHealthBar::None);
                if ui.checkbox("Health", &mut health_bar_enabled) {
                    config.health_bar = if health_bar_enabled {
                        EspHealthBar::Bottom
                    } else {
                        EspHealthBar::None
                    };
                }
                if let Some(_popup) = ui.begin_popup_context_with_label("##health_bar_context") {
                    const HEALTH_BAR_TYPES: [(EspHealthBar, &'static str); 4] = [
                        (EspHealthBar::Top, "Top"),
                        (EspHealthBar::Left, "Left"),
                        (EspHealthBar::Bottom, "Bottom"),
                        (EspHealthBar::Right, "Right"),
                    ];
                    ui.set_next_item_width(COMBO_WIDTH);
                    ui.combo_enum("##health_bar_popup", &HEALTH_BAR_TYPES, &mut config.health_bar);
                    ui.slider_config("Width", 5.0, 30.0).build(&mut config.health_bar_width);
                    Self::render_esp_color_popup(ui, "Health bar color", &mut config.health_bar_color);
                }

                ui.table_next_column();
                ui.checkbox("HP", &mut config.info_hp_text);
                if let Some(_popup) = ui.begin_popup_context_with_label("##info_hp_text_context") {
                    Self::render_esp_color_popup(ui, "HP text color", &mut config.info_hp_text_color);
                    Self::render_esp_info_position_popup(ui, "Position", &mut config.info_hp_text_position);
                }

            }

            ui.table_next_column();
            ui.checkbox("Name", &mut config.info_name);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_name_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_name_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_name_position);
            }
            ui.table_next_column();
            ui.checkbox("Weapon", &mut config.info_weapon);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_weapon_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_weapon_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_weapon_position);
            }
            ui.table_next_column();
            ui.checkbox("Ammo", &mut config.info_ammo);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_ammo_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_ammo_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_ammo_position);
            }
            ui.table_next_column();
            ui.checkbox("Distance", &mut config.info_distance);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_distance_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_distance_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_distance_position);
                ui.checkbox("Near only", &mut config.near_players);
                if config.near_players {
                    ui.slider_config("Distance", 0.0, 50.0)
                        .build(&mut config.near_players_distance);
                }
            }

            ui.table_next_column();
            ui.checkbox("Kit", &mut config.info_flag_kit);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_kit_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_flags_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_flags_position);
            }
            ui.table_next_column();
            ui.checkbox("Scoped", &mut config.info_flag_scoped);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_scoped_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_flags_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_flags_position);
            }
            ui.table_next_column();
            ui.checkbox("Flashed", &mut config.info_flag_flashed);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_flashed_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_flags_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_flags_position);
            }
            ui.table_next_column();
            ui.checkbox("Bomb Carrier", &mut config.info_flag_bomb);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_bomb_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_flags_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_flags_position);
            }
            ui.table_next_column();
            ui.checkbox("Grenades", &mut config.info_grenades);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_grenades_context") {
                Self::render_esp_color_popup(ui, "Color", &mut config.info_grenades_color);
                Self::render_esp_info_position_popup(ui, "Position", &mut config.info_grenades_position);
            }
            ui.table_next_column();
            ui.checkbox("Timer", &mut settings.bomb_timer);
            ui.table_next_column();
            ui.checkbox("Label", &mut settings.bomb_label);
            ui.table_next_column();
            ui.checkbox("Spectators", &mut settings.spectators_list);
            ui.table_next_column();
            ui.checkbox("Arrows", &mut config.offscreen_arrows);
            if let Some(_popup) = ui.begin_popup_context_with_label("##info_arrows_context") {
                ui.slider_config("Size", 5.0, 40.0).build(&mut config.offscreen_arrows_size);
                ui.slider_config("Radius", 20.0, 500.0).build(&mut config.offscreen_arrows_radius_from_center);
                Self::render_esp_color_popup(ui, "Color", &mut config.offscreen_arrows_color);
            }
            if !matches!(self.esp_selected_target, EspSelector::None) {
                let target_key = self.esp_selected_target.config_key();
                let target_enabled = settings.esp_settings_enabled.get(&target_key).cloned().unwrap_or(false);
                let _enabled = ui.begin_enabled(target_enabled);
                if ui.button("Reset") {
                    reset_requested = true;
                }
            }
        }

        if false {
            ui.same_line();
        if let Some(_style_panel) = ui
            .child_window("##style_panel")
            .size([0.0, 0.0])
            .border(false)
            .scroll_bar(true)
            .begin()
        {
        let content_height = ui.content_region_avail()[1] - 16.0;
        unsafe {
            imgui::sys::igSetNextItemOpen(true, 0);
        }
        if ui.collapsing_header("Colors", TreeNodeFlags::empty()) {
            if let Some(_token) = {
                ui.child_window("styles")
                    .size([0.0, content_height])
                    .begin()
            } {
                ui.indent_by(5.0);
                ui.dummy([0.0, 5.0]);

                if let Some(_token) = {
                    ui.begin_table_header_with_flags(
                        "styles_table",
                        [TableColumnSetup::new("Name"), TableColumnSetup::new("Value")],
                        TableFlags::ROW_BG
                            | TableFlags::BORDERS
                            | TableFlags::SIZING_STRETCH_PROP
                            | TableFlags::SCROLL_Y,
                    )
                } {
                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("ESP box color"),
                        &mut config.box_color,
                        &mut [StyleSlider {
                            min: 1.0,
                            max: 10.0,
                            value: &mut config.box_width,
                        }],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Player skeleton color"),
                        &mut config.skeleton_color,
                        &mut [StyleSlider {
                            min: 1.0,
                            max: 10.0,
                            value: &mut config.skeleton_width,
                        }],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Head dot color"),
                        &mut config.head_dot_color,
                        &mut [
                            StyleSlider {
                                min: 0.0,
                                max: 10.0,
                                value: &mut config.head_dot_base_radius,
                            },
                            StyleSlider {
                                min: 0.0,
                                max: 10.0,
                                value: &mut config.head_dot_z,
                            },
                        ],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info name"),
                        &mut config.info_name_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info distance"),
                        &mut config.info_distance_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info weapon"),
                        &mut config.info_weapon_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info ammo"),
                        &mut config.info_ammo_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info health"),
                        &mut config.info_hp_text_color,
                        &mut [StyleSlider {
                            min: 5.0,
                            max: 30.0,
                            value: &mut config.health_bar_width,
                        }],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info player flags"),
                        &mut config.info_flags_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Color info grenades"),
                        &mut config.info_grenades_color,
                        &mut [],
                    );

                    ui.table_next_row();
                    Self::render_esp_settings_player_style_color(
                        ui,
                        obfstr!("Offscreen arrow color"),
                        &mut config.offscreen_arrows_color,
                        &mut [
                            StyleSlider {
                                min: 5.0,
                                max: 40.0,
                                value: &mut config.offscreen_arrows_size,
                            },
                            StyleSlider {
                                min: 20.0,
                                max: 500.0,
                                value: &mut config.offscreen_arrows_radius_from_center,
                            },
                        ],
                    );
                }
            }
        }
        }
        }


        drop(_ui_enable_token);
        drop(config);
        if reset_requested {
            settings.esp_settings.remove(&config_key);
        }
    }

    fn render_esp_color_popup(ui: &imgui::Ui, label: &str, color: &mut EspColor) {
        if !matches!(color, EspColor::Static { .. }) {
            *color = EspColor::from_rgba(1.0, 1.0, 1.0, 1.0);
        }

        let mut color_type = EspColorType::Static;
        ui.text(label);
        ui.set_next_item_width(150.0);
        if ui.combo_enum(
            &format!("##{}_type", label),
            &[(EspColorType::Static, "Static")],
            &mut color_type,
        ) {
            *color = EspColor::from_rgba(1.0, 1.0, 1.0, 1.0);
        }

        match color {
            EspColor::Static { value } => {
                let mut value = value.as_f32();
                if ui
                    .color_edit4_config("Value", &mut value)
                    .alpha_bar(true)
                    .inputs(true)
                    .build()
                {
                    *color = EspColor::Static {
                        value: Color::from_f32(value),
                    };
                }
            }
            _ => unreachable!(),
        }
    }

    fn render_esp_info_position_popup(
        ui: &imgui::Ui,
        label: &str,
        position: &mut EspInfoPosition,
    ) {
        const INFO_POSITION_TYPES: [(EspInfoPosition, &'static str); 4] = [
            (EspInfoPosition::Left, "Left"),
            (EspInfoPosition::Right, "Right"),
            (EspInfoPosition::Top, "Top"),
            (EspInfoPosition::Bottom, "Bottom"),
        ];
        ui.text(label);
        ui.set_next_item_width(110.0);
        ui.combo_enum("##info_position_popup", &INFO_POSITION_TYPES, position);
    }

    fn render_esp_settings_player_style_color(
        ui: &imgui::Ui,
        label: &str,
        color: &mut EspColor,
        sliders: &mut [StyleSlider<'_>],
    ) {
        ui.table_next_column();
        ui.text(label);

        ui.table_next_column();
        {
            let mut color_type = EspColorType::from_esp_color(color);
            ui.set_next_item_width(110.0);
            let color_type_changed = ui.combo_enum(
                &format!("##{}_color_type", ui.table_row_index()),
                &[
                    (EspColorType::Static, "Static"),
                    (EspColorType::HealthBased, "Health based"),
                    (EspColorType::HealthBasedRainbow, "Rainbow"),
                    (EspColorType::DistanceBased, "Distance"),
                ],
                &mut color_type,
            );

            if color_type_changed {
                *color = match color_type {
                    EspColorType::Static => EspColor::Static {
                        value: Color::from_f32([1.0, 1.0, 1.0, 1.0]),
                    },
                    EspColorType::HealthBased => EspColor::HealthBased {
                        max: Color::from_f32([0.0, 1.0, 0.0, 1.0]),
                        mid: Color::from_f32([1.0, 1.0, 0.0, 1.0]),
                        min: Color::from_f32([1.0, 0.0, 0.0, 1.0]),
                    },
                    EspColorType::HealthBasedRainbow => EspColor::HealthBasedRainbow { alpha: 1.0 },
                    EspColorType::DistanceBased => EspColor::DistanceBased {
                        near: Color::from_f32([1.0, 0.0, 0.0, 1.0]),
                        mid: Color::from_f32([1.0, 1.0, 0.0, 1.0]),
                        far: Color::from_f32([0.0, 1.0, 0.0, 1.0]),
                    },
                }
            }
            ui.same_line();

            match color {
                EspColor::HealthBasedRainbow { alpha } => {
                    ui.text("Alpha:");
                    ui.same_line();
                    ui.set_next_item_width(100.0);
                    ui.slider_config(
                        &format!("##{}_rainbow_alpha", ui.table_row_index()),
                        0.1,
                        1.0,
                    )
                    .display_format("%.2f")
                    .build(alpha);
                }
                EspColor::Static { value } => {
                    let mut color_value = value.as_f32();

                    if {
                        ui.color_edit4_config(
                            &format!("##{}_static_value", ui.table_row_index()),
                            &mut color_value,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    } {
                        *value = Color::from_f32(color_value);
                    }
                }
                EspColor::HealthBased { max, mid, min } => {
                    let mut max_value = max.as_f32();
                    if {
                        ui.color_edit4_config(
                            &format!("##{}_health_max", ui.table_row_index()),
                            &mut max_value,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    } {
                        *max = Color::from_f32(max_value);
                    }
                    ui.same_line();
                    ui.text(" => ");
                    ui.same_line();

                    let mut mid_value = mid.as_f32();
                    if {
                        ui.color_edit4_config(
                            &format!("##{}_health_mid", ui.table_row_index()),
                            &mut mid_value,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    } {
                        *mid = Color::from_f32(mid_value);
                    }
                    ui.same_line();
                    ui.text(" => ");
                    ui.same_line();

                    let mut min_value = min.as_f32();
                    if {
                        ui.color_edit4_config(
                            &format!("##{}_health_min", ui.table_row_index()),
                            &mut min_value,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    } {
                        *min = Color::from_f32(min_value);
                    }
                }
                EspColor::DistanceBased { near, mid, far } => {
                    let mut near_color = near.as_f32();
                    if ui
                        .color_edit4_config(
                            &format!("##{}_near", ui.table_row_index()),
                            &mut near_color,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    {
                        *near = Color::from_f32(near_color);
                    }

                    ui.same_line();
                    ui.text(" => ");
                    ui.same_line();
                    let mut mid_color = mid.as_f32();
                    if ui
                        .color_edit4_config(
                            &format!("##{}_mid", ui.table_row_index()),
                            &mut mid_color,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    {
                        *mid = Color::from_f32(mid_color);
                    }

                    ui.same_line();
                    ui.text(" => ");
                    ui.same_line();
                    let mut far_color = far.as_f32();
                    if ui
                        .color_edit4_config(
                            &format!("##{}_far", ui.table_row_index()),
                            &mut far_color,
                        )
                        .alpha_bar(true)
                        .inputs(false)
                        .label(false)
                        .build()
                    {
                        *far = Color::from_f32(far_color);
                    }
                }
            }
        }

        for (index, slider) in sliders.iter_mut().enumerate() {
            if index > 0 {
                ui.same_line();
            }
            ui.set_next_item_width(95.0);
            ui.slider_config(
                &format!("##{}_style_slider_{}", ui.table_row_index(), index),
                slider.min,
                slider.max,
            )
            .display_format("%.2f")
            .build(slider.value);
        }
    }

    fn render_esp_settings_chicken(
        &mut self,
        _settings: &mut AppSettings,
        ui: &imgui::Ui,
        _target: EspSelector,
    ) {
        ui.text("Chicken!");
    }

    fn render_esp_settings_weapon(
        &mut self,
        _settings: &mut AppSettings,
        ui: &imgui::Ui,
        _target: EspSelector,
    ) {
        ui.text("Weapon!");
    }

    fn render_esp_settings(&mut self, settings: &mut AppSettings, ui: &imgui::Ui) {
        if let Some(target) = self.esp_pending_target.take() {
            self.esp_selected_target = target;
        }

        /* the left tree */
        let content_region = ui.content_region_avail();
        let original_style = ui.clone_style();
        let tree_width = (content_region[0] * 0.14).max(130.0);
        let middle_width = (content_region[0] - tree_width - 8.0).max(560.0);

        if let (Some(_token), _padding) = {
            let padding = ui.push_style_var(StyleVar::WindowPadding([
                0.0,
                original_style.window_padding[1],
            ]));
            let window = ui
                .child_window("ESP Target")
                .size([tree_width, 0.0])
                .border(false)
                .draw_background(true)
                .scroll_bar(true)
                .begin();

            (window, padding)
        } {
            ui.indent_by(
                original_style.window_padding[0] +
                    /* for the indicator */
                    ui.current_font_size() * 0.5 + 4.0,
            );

            self.render_esp_target(settings, ui, &EspSelector::Player);
            // self.render_esp_target(settings, ui, &EspSelector::Chicken);
            // self.render_esp_target(settings, ui, &EspSelector::Weapon)
        }
        ui.same_line();
        if let Some(_token) = {
            ui.child_window("Content")
                .size([middle_width, 0.0])
                .scroll_bar(false)
                .begin()
        } {
            match &self.esp_selected_target {
                EspSelector::None => {}
                EspSelector::Player
                | EspSelector::PlayerTeam { .. }
                | EspSelector::PlayerTeamVisibility { .. } => {
                    self.render_esp_settings_player(settings, ui, self.esp_selected_target.clone())
                }
                EspSelector::Chicken => {
                    self.render_esp_settings_chicken(settings, ui, self.esp_selected_target.clone())
                }
                EspSelector::Weapon
                | EspSelector::WeaponGroup { .. }
                | EspSelector::WeaponSingle { .. } => {
                    self.render_esp_settings_weapon(settings, ui, self.esp_selected_target.clone())
                }
            }
        }
    }

    fn render_grenade_target(
        &mut self,
        settings: &mut GrenadeSettings,
        ui: &imgui::Ui,
        target: &GrenadeSettingsTarget,
    ) {
        let ident = ui.clone_style().indent_spacing * target.ident_level() as f32;
        if ident > 0.0 {
            ui.indent_by(ident);
        }

        let item_text = match target {
            GrenadeSettingsTarget::General => "Settings".to_string(),
            GrenadeSettingsTarget::MapType(value) => value.clone(),
            GrenadeSettingsTarget::Map {
                map_name,
                display_name,
            } => {
                let location_count = settings.map_spots.get(map_name).map(Vec::len).unwrap_or(0);
                format!(
                    "{} ({}) ##{}",
                    display_name,
                    location_count,
                    target.ui_token()
                )
            }
        };

        let clicked = ui
            .selectable_config(item_text)
            .selected(target == &self.grenade_helper_target)
            .flags(SelectableFlags::SPAN_ALL_COLUMNS)
            .build();

        if clicked && !matches!(target, GrenadeSettingsTarget::MapType(_)) {
            self.grenade_helper_pending_target = Some(target.clone());
        }

        if ident > 0.0 {
            ui.unindent_by(ident);
        }
    }

    fn render_grenade_helper(
        &mut self,
        states: &StateRegistry,
        settings: &mut GrenadeSettings,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
    ) {
        if let Some(target) = self.grenade_helper_pending_target.take() {
            self.grenade_helper_target = target;
            self.grenade_helper_selected_id = 0;
            self.grenade_helper_new_item = None;
        }

        if let Some(target) = self.grenade_helper_pending_selected_id.take() {
            self.grenade_helper_selected_id = target;
            self.grenade_helper_new_item = None;
        }

        /* the left tree */
        let content_region = ui.content_region_avail();
        let original_style = ui.clone_style();
        let tree_width = (content_region[0] * 0.25).max(150.0);
        let content_width = content_region[0] - tree_width - 5.0;

        {
            let mut grenade_helper_transfer_state =
                self.grenade_helper_transfer_state.lock().unwrap();
            let _buttons_disabled = ui.begin_disabled(!matches!(
                &*grenade_helper_transfer_state,
                GrenadeHelperTransferState::Idle
            ));
            if ui.button("Export") {
                *grenade_helper_transfer_state = GrenadeHelperTransferState::Pending {
                    direction: GrenadeHelperTransferDirection::Export,
                };
            }
            ui.same_line();
            if ui.button("Import") {
                *grenade_helper_transfer_state = GrenadeHelperTransferState::Pending {
                    direction: GrenadeHelperTransferDirection::Import,
                };
            }
        }
        ui.separator();

        //ui.dummy([0.0, 10.0]);

        if let (Some(_token), _padding) = {
            let padding = ui.push_style_var(StyleVar::WindowPadding([
                0.0,
                original_style.window_padding[1],
            ]));
            let window = ui
                .child_window("Helper Target")
                .size([tree_width, 0.0])
                .border(false)
                .draw_background(true)
                .scroll_bar(true)
                .begin();

            (window, padding)
        } {
            ui.indent_by(original_style.window_padding[0] + 4.0);

            for target in [
                GrenadeSettingsTarget::General,
                GrenadeSettingsTarget::MapType("Competitive Maps".to_owned()),
                GrenadeSettingsTarget::Map {
                    map_name: "de_ancient".to_owned(),
                    display_name: "Ancient".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_anubis".to_owned(),
                    display_name: "Anubis".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_dust2".to_owned(),
                    display_name: "Dust 2".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_inferno".to_owned(),
                    display_name: "Inferno".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_mills".to_owned(),
                    display_name: "Mills".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_mirage".to_owned(),
                    display_name: "Mirage".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_nuke".to_owned(),
                    display_name: "Nuke".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "cs_office".to_owned(),
                    display_name: "Office".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_overpass".to_owned(),
                    display_name: "Overpass".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_thera".to_owned(),
                    display_name: "Thera".to_owned(),
                },
                GrenadeSettingsTarget::Map {
                    map_name: "de_vertigo".to_owned(),
                    display_name: "Vertigo".to_owned(),
                },
            ] {
                self.render_grenade_target(settings, ui, &target);
            }
        }
        ui.same_line();
        if let Some(_token) = {
            ui.child_window("Content")
                .size([content_width, 0.0])
                .scroll_bar(true)
                .begin()
        } {
            match &self.grenade_helper_target {
                GrenadeSettingsTarget::General => {
                    self.render_grenade_helper_target_settings(states, settings, ui);
                }
                GrenadeSettingsTarget::MapType(_) => { /* Nothing to render */ }
                GrenadeSettingsTarget::Map { map_name, .. } => {
                    self.render_grenade_helper_target_map(
                        states,
                        settings,
                        ui,
                        &map_name.clone(),
                        unicode_text,
                    );
                }
            }
        }
    }

    fn render_grenade_helper_target_map(
        &mut self,
        states: &StateRegistry,
        settings: &mut GrenadeSettings,
        ui: &imgui::Ui,
        map_name: &str,
        unicode_text: &UnicodeTextRenderer,
    ) {
        /* the left tree */
        let content_region = ui.content_region_avail();
        let original_style = ui.clone_style();
        let tree_width = (content_region[0] * 0.25).max(150.0);
        let content_width = content_region[0] - tree_width - original_style.item_spacing[0];

        /* The list itself */
        {
            ui.text("Available spots");
            let text_width = ui.calc_text_size("Available spots")[0];
            let button_width = tree_width - text_width - original_style.item_spacing[0];

            ui.same_line();

            ui.set_next_item_width(button_width);
            ui.combo_enum(
                "##sort_type",
                &[
                    (GrenadeSortOrder::Alphabetical, "A-z"),
                    (GrenadeSortOrder::AlphabeticalReverse, "Z-a"),
                ],
                &mut settings.ui_sort_order,
            );

            if let (Some(_token), _padding) = {
                let padding = ui.push_style_var(StyleVar::WindowPadding([
                    0.0,
                    original_style.window_padding[1],
                ]));
                let window = ui
                    .child_window("Map Target")
                    .size([
                        tree_width,
                        content_region[1]
                            - ui.text_line_height_with_spacing() * 2.0
                            - original_style.frame_padding[1] * 4.0,
                    ])
                    .border(false)
                    .draw_background(true)
                    .scroll_bar(true)
                    .begin();

                (window, padding)
            } {
                ui.indent_by(original_style.window_padding[0]);

                if let Some(grenades) = settings.map_spots.get(map_name) {
                    let mut sorted_grenades = grenades.iter().collect::<Vec<_>>();
                    settings.ui_sort_order.sort(&mut sorted_grenades);

                    for grenade in sorted_grenades.iter() {
                        let grenade_types = grenade
                            .grenade_types
                            .iter()
                            .map(GrenadeType::display_name)
                            .collect::<Vec<_>>()
                            .join(", ");

                        let clicked = ui
                            .selectable_config(format!(
                                "{} ({}) ##{}",
                                grenade.name, grenade_types, grenade.id
                            ))
                            .selected(grenade.id == self.grenade_helper_selected_id)
                            .flags(SelectableFlags::SPAN_ALL_COLUMNS)
                            .build();
                        unicode_text.register_unicode_text(&grenade.name);

                        if clicked {
                            self.grenade_helper_pending_selected_id = Some(grenade.id);
                        }
                    }
                }
            }

            /* Add / delete buttons */
            {
                let mut delete_current_grenade = false;
                let current_grenade_position = settings
                    .map_spots
                    .get(map_name)
                    .map(|spots| {
                        spots
                            .iter()
                            .position(|spot| spot.id == self.grenade_helper_selected_id)
                    })
                    .flatten();

                let button_width = (tree_width - original_style.item_spacing[0]) / 2.0;
                ui.set_cursor_pos([
                    0.0,
                    content_region[1]
                        - ui.text_line_height()
                        - original_style.frame_padding[1] * 2.0,
                ]);
                if ui.button_with_size("New", [button_width, 0.0]) {
                    self.grenade_helper_new_item = Some(Default::default());
                    self.grenade_helper_selected_id = 0;
                }

                let _button_disabled = ui.begin_disabled(current_grenade_position.is_none());
                ui.same_line();
                if ui.button_with_size("Delete", [button_width, 0.0]) {
                    if self.grenade_helper_skip_confirmation_dialog {
                        delete_current_grenade = true;
                    } else {
                        ui.open_popup("Delete item? ##delete_grenade_helper_spot");
                    }
                }

                if let Some(_token) = ui
                    .modal_popup_config("Delete item? ##delete_grenade_helper_spot")
                    .resizable(false)
                    .movable(false)
                    .always_auto_resize(true)
                    .begin_popup()
                {
                    ui.text("Are you sure you want to delete this item?");
                    ui.spacing();
                    ui.separator();
                    ui.spacing();
                    ui.checkbox(
                        "do not ask again",
                        &mut self.grenade_helper_skip_confirmation_dialog,
                    );

                    let button_width =
                        (ui.content_region_avail()[0] - original_style.item_spacing[0]) / 2.0;
                    if ui.button_with_size("Yes", [button_width, 0.0]) {
                        ui.close_current_popup();
                        delete_current_grenade = true;
                    }

                    ui.same_line();
                    if ui.button_with_size("No", [button_width, 0.0]) {
                        ui.close_current_popup();
                    }
                }

                if delete_current_grenade {
                    if let Some(grenades) = settings.map_spots.get_mut(map_name) {
                        grenades.remove(current_grenade_position.unwrap());
                    }
                }
            }
        }

        /* grenade info */
        ui.set_cursor_pos([tree_width + original_style.item_spacing[0], 0.0]);
        if let Some(_token) = {
            ui.child_window("Content")
                .size([content_width, 0.0])
                .scroll_bar(true)
                .begin()
        } {
            if let Some(current_grenade) = {
                settings
                    .map_spots
                    .get_mut(map_name)
                    .map(|spots| {
                        spots
                            .iter_mut()
                            .find(|spot| spot.id == self.grenade_helper_selected_id)
                    })
                    .flatten()
                    .or(self.grenade_helper_new_item.as_mut())
            } {
                let mut assign_current_position = false;
                let _full_width = ui.push_item_width(-1.0);

                if current_grenade.id > 0 {
                    ui.text("Grenade Info");
                } else {
                    ui.text("Add a new grenade spot");
                }

                ui.text("Name");
                ui.input_text("##grenade_helper_spot_name", &mut current_grenade.name)
                    .build();
                unicode_text.register_unicode_text(&current_grenade.name);

                ui.text("Description");
                ui.input_text_multiline(
                    "##grenade_helper_spot_description",
                    &mut current_grenade.description,
                    [0.0, 100.0],
                )
                .build();
                unicode_text.register_unicode_text(&current_grenade.description);

                ui.text("Eye position");
                ui.input_float3(
                    "##grenade_helper_spot_eye_position",
                    &mut current_grenade.eye_position,
                )
                .display_format("%.3f")
                .build();

                ui.text("Pitch/Yaw");
                ui.input_float2(
                    "##grenade_helper_spot_ptch_yaw",
                    &mut current_grenade.eye_direction,
                )
                .display_format("%.3f")
                .build();

                let current_map = states
                    .get::<StateCurrentMap>(())
                    .map(|value| value.current_map.clone())
                    .flatten();

                let current_player_position = states
                    .resolve::<StateGrenadeHelperPlayerLocation>(())
                    .map(|value| {
                        if let StateGrenadeHelperPlayerLocation::Valid {
                            eye_position,
                            eye_direction,
                        } = *value
                        {
                            Some((eye_position, eye_direction))
                        } else {
                            None
                        }
                    });

                {
                    let button_enabled =
                        current_player_position.as_ref().unwrap_or(&None).is_some();
                    let _enabled_token = ui.begin_enabled(button_enabled);
                    if ui.button("Use current") {
                        if current_map
                            .as_ref()
                            .map(|current_map| current_map == map_name)
                            .unwrap_or(false)
                        {
                            assign_current_position = true;
                        } else {
                            /* Map differs */
                            ui.open_popup(
                                "Are you sure?##grenade_helper_use_current_map_different",
                            );
                        }
                    }

                    if ui.is_item_hovered() {
                        match &current_player_position {
                            Ok(Some(_)) => {
                                ui.tooltip_text("Copy your current location and direction")
                            }
                            Ok(None) => ui.tooltip_text("You don't have a valid player position"),
                            Err(err) => ui.tooltip_text(format!("Error: {:#}", err)),
                        }
                    }
                }

                if let Some(_token) = ui
                    .modal_popup_config("Are you sure?##grenade_helper_use_current_map_different")
                    .resizable(false)
                    .always_auto_resize(true)
                    .begin_popup()
                {
                    ui.text("The current map does not match the selected map.");
                    ui.text(format!("Selected map: {}", map_name));
                    ui.text(format!(
                        "Current map: {}",
                        current_map
                            .as_ref()
                            .map(String::as_str)
                            .unwrap_or("unknown")
                    ));
                    ui.new_line();
                    ui.text("Do you want to copy the location anyways?");

                    ui.spacing();
                    ui.separator();
                    ui.spacing();

                    let button_width =
                        (ui.content_region_avail()[0] - original_style.item_spacing[0]) / 2.0;
                    if ui.button_with_size("Yes", [button_width, 0.0]) {
                        ui.close_current_popup();
                        assign_current_position = true;
                    }

                    ui.same_line();
                    if ui.button_with_size("No", [button_width, 0.0]) {
                        ui.close_current_popup();
                    }
                }

                if assign_current_position {
                    if let Some((eye_position, eye_direction)) =
                        current_player_position.ok().flatten()
                    {
                        current_grenade.eye_position = eye_position.as_slice().try_into().unwrap();
                        current_grenade.eye_direction =
                            eye_direction.as_slice().try_into().unwrap();
                    }
                }

                for grenade_type in [
                    GrenadeType::Smoke,
                    GrenadeType::Flashbang,
                    GrenadeType::Explosive,
                    GrenadeType::Molotov,
                ] {
                    let current_position = current_grenade
                        .grenade_types
                        .iter()
                        .position(|value| *value == grenade_type);

                    let mut enabled = current_position.is_some();
                    if ui.checkbox(grenade_type.display_name(), &mut enabled) {
                        if let Some(current_position) = current_position {
                            current_grenade.grenade_types.remove(current_position);
                        } else {
                            current_grenade.grenade_types.push(grenade_type);
                        }
                    }
                }

                if current_grenade.id == 0 {
                    let region_avail = ui.content_region_max();
                    ui.set_cursor_pos([region_avail[0] - 100.0, ui.cursor_pos()[1]]);
                    if ui.button_with_size("Create", [100.0, 0.0]) {
                        if let Some(mut grenade) = self.grenade_helper_new_item.take() {
                            let grenades =
                                settings.map_spots.entry(map_name.to_string()).or_default();

                            grenade.id = GrenadeSpotInfo::new_id();
                            self.grenade_helper_pending_selected_id = Some(grenade.id);

                            grenades.push(grenade);
                        }
                    }
                }
            } else {
                let text = "Please select an item";
                let text_bounds = ui.calc_text_size(text);
                let region_avail = ui.content_region_avail();

                ui.set_cursor_pos([
                    (region_avail[0] - text_bounds[0]) / 2.0,
                    (region_avail[1] - text_bounds[1]) / 2.0,
                ]);

                ui.text_colored(
                    ui.style_color(StyleColor::TextDisabled),
                    "Please select a grenade",
                );
            }
        }
    }

    fn render_grenade_helper_target_settings(
        &mut self,
        _states: &StateRegistry,
        settings: &mut GrenadeSettings,
        ui: &imgui::Ui,
    ) {
        fn render_color(ui: &imgui::Ui, label: &str, value: &mut Color) {
            let mut color_value = value.as_f32();

            if {
                ui.color_edit4_config(label, &mut color_value)
                    .alpha_bar(true)
                    .inputs(false)
                    .label(true)
                    .build()
            } {
                *value = Color::from_f32(color_value);
            }
        }

        ui.text("UI Settings");
        ui.spacing();

        ui.input_float("Circle distance", &mut settings.circle_distance)
            .build();
        ui.input_float("Circle radius", &mut settings.circle_radius)
            .build();
        ui.input_scalar("Circle segments", &mut settings.circle_segments)
            .build();

        ui.input_float("Angle threshold yar", &mut settings.angle_threshold_yaw)
            .build();
        ui.input_float("Angle threshold pitch", &mut settings.angle_threshold_pitch)
            .build();

        render_color(ui, "Color position", &mut settings.color_position);
        render_color(
            ui,
            "Color position (active)",
            &mut settings.color_position_active,
        );
        render_color(ui, "Color angle", &mut settings.color_angle);
        render_color(
            ui,
            "Color angle  (active)",
            &mut settings.color_angle_active,
        );

        ui.checkbox(
            obfstr!("ViewBox Background"),
            &mut settings.grenade_background,
        );
    }

    fn render_grenade_helper_transfer(&mut self, settings: &mut GrenadeSettings, ui: &imgui::Ui) {
        let mut transfer_state = self.grenade_helper_transfer_state.lock().unwrap();
        match &*transfer_state {
            GrenadeHelperTransferState::Idle => return,

            GrenadeHelperTransferState::Pending { direction } => {
                let executor: Box<
                    dyn FnOnce() -> anyhow::Result<GrenadeHelperTransferState> + Send,
                > = match direction {
                    GrenadeHelperTransferDirection::Export => {
                        let items = settings.map_spots.clone();
                        Box::new(move || {
                            // GrenadeHelperTransferState
                            let Some(target_path) = rfd::FileDialog::new()
                                .add_filter("SwingApp Grenade Spots", &["vgs"])
                                .save_file()
                            else {
                                return Ok(GrenadeHelperTransferState::Idle);
                            };

                            let items = serde_json::to_string(&items)?;
                            let mut output = File::options()
                                .create(true)
                                .truncate(true)
                                .write(true)
                                .open(&target_path)
                                .context("open destination")?;
                            output.write_all(items.as_bytes()).context("write")?;

                            Ok(GrenadeHelperTransferState::ExportSuccess { target_path })
                        })
                    }
                    GrenadeHelperTransferDirection::Import => {
                        Box::new(move || {
                            // GrenadeHelperTransferState
                            let Some(target_path) = rfd::FileDialog::new()
                                .add_filter("SwingApp Grenade Spots", &["vgs"])
                                .pick_file()
                            else {
                                return Ok(GrenadeHelperTransferState::Idle);
                            };

                            let input = File::options()
                                .read(true)
                                .open(target_path)
                                .context("open file")?;

                            let elements = serde_json::from_reader(&mut BufReader::new(input))
                                .context("parse file")?;

                            Ok(GrenadeHelperTransferState::ImportPending { elements })
                        })
                    }
                };

                thread::spawn({
                    let direction = direction.clone();
                    let grenade_helper_transfer_state = self.grenade_helper_transfer_state.clone();
                    move || {
                        let result = executor();
                        let mut transfer_state = grenade_helper_transfer_state.lock().unwrap();
                        match result {
                            Ok(new_state) => {
                                *transfer_state = new_state;
                            }
                            Err(err) => {
                                *transfer_state = GrenadeHelperTransferState::Failed {
                                    direction,
                                    message: format!("{:#}", err),
                                };
                            }
                        }
                    }
                });
                *transfer_state = GrenadeHelperTransferState::Active {
                    direction: direction.clone(),
                };
            }
            GrenadeHelperTransferState::Active { .. } => {
                /* Just waiting for the file picker to finish. */
            }

            GrenadeHelperTransferState::ImportPending { elements } => {
                let mut popup_open = true;
                if let Some(_popup) = ui
                    .modal_popup_config("Data Import")
                    .always_auto_resize(true)
                    .opened(&mut popup_open)
                    .begin_popup()
                {
                    let total_count = elements.values().map(|spots| spots.len()).sum::<usize>();

                    ui.text(format!(
                        "The following locations have been loaded ({})",
                        total_count
                    ));
                    for (map_name, spots) in elements.iter() {
                        ui.text(format!("- {} ({} spots)", map_name, spots.len()));
                    }

                    ui.new_line();
                    ui.text("Would you like to replace the current configuration?");

                    ui.spacing();
                    ui.separator();
                    ui.spacing();

                    let button_width =
                        (ui.content_region_avail()[0] - ui.clone_style().item_spacing[0]) / 2.0;

                    if ui.button_with_size("Cancel", [button_width, 0.0]) {
                        *transfer_state = GrenadeHelperTransferState::Idle;
                        return;
                    }

                    ui.same_line();
                    if ui.button_with_size("Yes", [button_width, 0.0]) {
                        settings.map_spots = elements.clone();
                        *transfer_state = GrenadeHelperTransferState::ImportSuccess {
                            count: total_count,
                            replacing: false,
                        };
                    }
                } else {
                    ui.open_popup("Data Import");
                }
            }

            GrenadeHelperTransferState::Failed { direction, message } => {
                let mut popup_open = true;
                let popup_name = format!(
                    "{} failed",
                    match direction {
                        GrenadeHelperTransferDirection::Export => "Export",
                        GrenadeHelperTransferDirection::Import => "Import",
                    }
                );
                if let Some(_popup) = ui
                    .modal_popup_config(&popup_name)
                    .opened(&mut popup_open)
                    .always_auto_resize(true)
                    .begin_popup()
                {
                    ui.text("A fatal error occurred:");
                    ui.spacing();

                    ui.text(message);

                    ui.spacing();
                    ui.separator();
                    ui.spacing();
                    if ui.button_with_size("Close", [100.0, 0.0]) {
                        popup_open = false;
                    }
                } else {
                    ui.open_popup(&popup_name);
                }

                if !popup_open {
                    *transfer_state = GrenadeHelperTransferState::Idle;
                }
            }
            GrenadeHelperTransferState::ExportSuccess { target_path } => {
                let mut popup_open = true;
                if let Some(_popup) = ui
                    .modal_popup_config("Export successful")
                    .opened(&mut popup_open)
                    .always_auto_resize(true)
                    .begin_popup()
                {
                    ui.text("All grenade helper spots have been exported to");
                    ui.text(format!("{}", target_path.display()));

                    ui.spacing();
                    ui.separator();
                    ui.spacing();
                    if ui.button_with_size("Close", [100.0, 0.0]) {
                        popup_open = false;
                    }
                } else {
                    ui.open_popup("Export successful");
                }

                if !popup_open {
                    *transfer_state = GrenadeHelperTransferState::Idle;
                }
            }
            GrenadeHelperTransferState::ImportSuccess { count, .. } => {
                let mut popup_open = true;
                if let Some(_popup) = ui
                    .modal_popup_config("Import successful")
                    .opened(&mut popup_open)
                    .always_auto_resize(true)
                    .begin_popup()
                {
                    ui.text(format!("{} grenade helper spots have been imported", count));

                    ui.spacing();
                    ui.separator();
                    ui.spacing();
                    if ui.button_with_size("Close", [100.0, 0.0]) {
                        popup_open = false;
                    }
                } else {
                    ui.open_popup("Import successful");
                }

                if !popup_open {
                    *transfer_state = GrenadeHelperTransferState::Idle;
                }
            }
        }
    }
}
