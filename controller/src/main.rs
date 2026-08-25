#![windows_subsystem = "windows"]

use std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    error::Error,
    fmt::Debug,
    fs,
    path::PathBuf,
    rc::Rc,
    str::FromStr,
    sync::{
        atomic::{
            AtomicBool,
            Ordering,
        },
        Arc,
    },
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
    time::{
        Duration,
        Instant,
    },
};

use anyhow::Context;
use clap::Parser;
use cs2::{
    schema_runtime::{
        self,
        SetupOptions,
    },
    CS2Handle,
    ConVars,
    InterfaceError,
    StateBuildInfo,
    StateCS2Handle,
    StateCurrentMap,
    StateCS2Memory,
};
use enhancements::{
    Enhancement,
    GrenadeHelper,
};
use imgui::{
    Condition,
    FontConfig,
    FontId,
    FontSource,
    Ui,
};
use obfstr::obfstr;
use overlay::{
    LoadingError,
    OverlayError,
    OverlayOptions,
    OverlayTarget,
    SystemRuntimeController,
    UnicodeTextRenderer,
    VulkanError,
};
use settings::{
    cloud,
    load_app_settings,
    AppSettings,
    SettingsUI,
};
use tokio::runtime;
use utils::show_critical_error;
use utils_state::StateRegistry;
use view::ViewController;
use windows::Win32::UI::Shell::IsUserAnAdmin;

use crate::{
    enhancements::{
        AntiAfk,
        AutoAccept,
        Radar,
        sniper_crosshair::SniperCrosshair,
        AntiAimPunsh,
        BombInfoIndicator,
        BombLabelIndicator,
        PlayerESP,
        SpectatorsListIndicator,
        TriggerBot,
    },
    settings::save_app_settings,
    utils::TextWithShadowUi,
    winver::version_info,
};

mod dialog;
mod enhancements;
mod settings;
mod utils;
mod view;
mod winver;

pub trait MetricsClient {
    fn add_metrics_record(&self, record_type: &str, record_payload: &str);
}

impl MetricsClient for CS2Handle {
    fn add_metrics_record(&self, record_type: &str, record_payload: &str) {
        self.add_metrics_record(record_type, record_payload)
    }
}

pub trait KeyboardInput {
    fn is_key_down(&self, key: imgui::Key) -> bool;
    fn is_key_pressed(&self, key: imgui::Key, repeating: bool) -> bool;
}

impl KeyboardInput for imgui::Ui {
    fn is_key_down(&self, key: imgui::Key) -> bool {
        Ui::is_key_down(self, key)
    }

    fn is_key_pressed(&self, key: imgui::Key, repeating: bool) -> bool {
        if repeating {
            Ui::is_key_pressed(self, key)
        } else {
            Ui::is_key_pressed_no_repeat(self, key)
        }
    }
}

pub struct UpdateContext<'a> {
    pub input: &'a dyn KeyboardInput,
    pub states: &'a StateRegistry,

    pub cs2: &'a Arc<CS2Handle>,
    pub settings_visible: bool,
    pub app_focus_lost: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FontReference {
    inner: Arc<RefCell<Option<FontId>>>,
}

impl FontReference {
    pub fn font_id(&self) -> Option<FontId> {
        self.inner.borrow().clone()
    }

    pub fn set_id(&self, font_id: FontId) {
        *self.inner.borrow_mut() = Some(font_id);
    }
}

#[derive(Clone, Default)]
pub struct AppFonts {
    valthrun: FontReference,
}

pub struct Application {
    pub fonts: AppFonts,
    pub app_state: StateRegistry,

    pub cs2: Arc<CS2Handle>,
    pub enhancements: Vec<Rc<RefCell<dyn Enhancement>>>,

    pub frame_read_calls: usize,
    pub last_total_read_calls: usize,

    pub settings_visible: bool,

    pub settings_dirty: bool,
    pub cloud_username: Option<String>,
    pub settings_ui: RefCell<SettingsUI>,
    pub settings_screen_capture_changed: AtomicBool,
    pub settings_render_debug_window_changed: AtomicBool,
    pub exit_requested: AtomicBool,
}

impl Application {
    pub fn settings(&self) -> Ref<'_, AppSettings> {
        self.app_state
            .get::<AppSettings>(())
            .expect("app settings to be present")
    }

    pub fn settings_mut(&self) -> RefMut<'_, AppSettings> {
        self.app_state
            .get_mut::<AppSettings>(())
            .expect("app settings to be present")
    }

    pub fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Relaxed);
    }

    pub fn pre_update(&mut self, controller: &mut SystemRuntimeController) -> anyhow::Result<()> {
        if self.settings_dirty {
            self.settings_dirty = false;
            let mut settings = self.settings_mut();

            settings.imgui = None;
            if let Ok(value) = serde_json::to_string(&*settings) {
                self.cs2.add_metrics_record("settings-updated", &value);
            }

            let mut imgui_settings = String::new();
            controller.imgui.save_ini_settings(&mut imgui_settings);
            settings.imgui = Some(imgui_settings);

            if let Err(error) = save_app_settings(&*settings) {
                log::warn!("Failed to save user settings: {}", error);
            };

            if let Some(username) = self.cloud_username.clone() {
                let cloud_settings = settings.clone();
                tokio::spawn(async move {
                    if let Err(error) = cloud::save_config(&username, &cloud_settings).await {
                        log::warn!("Failed to save cloud settings: {:#}", error);
                    }
                });
            }
        }

        if self
            .settings_screen_capture_changed
            .swap(false, Ordering::Relaxed)
        {
            let settings = self.settings();
            controller.toggle_screen_capture_visibility(!settings.hide_overlay_from_screen_capture);
            log::debug!(
                "Updating screen capture visibility to {}",
                !settings.hide_overlay_from_screen_capture
            );
        }

        if self
            .settings_render_debug_window_changed
            .swap(false, Ordering::Relaxed)
        {
            let settings = self.settings();
            controller.toggle_debug_overlay(settings.render_debug_window);
        }

        Ok(())
    }

    pub fn update(&mut self, ui: &imgui::Ui) -> anyhow::Result<()> {
        {
            for enhancement in self.enhancements.iter() {
                let mut hack = enhancement.borrow_mut();
                if hack.update_settings(ui, &mut *self.settings_mut())? {
                    self.settings_dirty = true;
                }
            }
        }

        if ui.is_key_pressed_no_repeat(self.settings().key_settings.0) {
            log::debug!("Toggle settings");
            self.settings_visible = !self.settings_visible;
            self.settings_mut().bomb_timer_edit_mode = self.settings_visible;
            self.cs2.add_metrics_record(
                "settings-toggled",
                &format!("visible: {}", self.settings_visible),
            );

            if !self.settings_visible {
                /* overlay has just been closed */
                self.settings_dirty = true;
            }
        }

        self.app_state.invalidate_states();
        if let Ok(mut view_controller) = self.app_state.resolve_mut::<ViewController>(()) {
            view_controller.update_screen_bounds(mint::Vector2::from_slice(&ui.io().display_size));
        }

        let update_context = UpdateContext {
            cs2: &self.cs2,

            states: &self.app_state,
            input: ui,
            settings_visible: self.settings_visible,
            app_focus_lost: ui.io().app_focus_lost,
        };

        for enhancement in self.enhancements.iter() {
            let mut enhancement = enhancement.borrow_mut();
            enhancement.update(&update_context)?;
        }

        let read_calls = self.cs2.ke_interface.total_read_calls();
        self.frame_read_calls = read_calls - self.last_total_read_calls;
        self.last_total_read_calls = read_calls;

        Ok(())
    }

    pub fn render(&self, ui: &imgui::Ui, unicode_text: &UnicodeTextRenderer) {
        {
            let settings = self.settings();
            let mut overlay = ui
                .window("overlay")
                .draw_background(false)
                .no_decoration()
                .size(ui.io().display_size, Condition::Always)
                .position([0.0, 0.0], Condition::Always);
            if !settings.bomb_timer_edit_mode {
                overlay = overlay.no_inputs();
            }
            overlay.build(|| self.render_overlay(ui, unicode_text));
        }

        {
            for enhancement in self.enhancements.iter() {
                let mut enhancement = enhancement.borrow_mut();
                if let Err(err) = enhancement.render_debug_window(&self.app_state, ui, unicode_text)
                {
                    log::error!("{:?}", err);
                }
            }
        }

        if self.settings_visible {
            let mut settings_ui = self.settings_ui.borrow_mut();
            settings_ui.render(self, ui, unicode_text);
        }

    }

    fn render_overlay(&self, ui: &imgui::Ui, unicode_text: &UnicodeTextRenderer) {
        let settings = self.settings();

        if settings.valthrun_watermark {
            let mut hud_items = vec![("SwingApp Overlay".to_owned(), None)];
            if settings.watermark_show_fps {
                hud_items.push((format!("{:.2} FPS", ui.io().framerate), None));
            }
            if settings.watermark_show_trigger {
                hud_items.push((
                    format!(
                        "TRG {} / {}",
                        if settings.trigger_bot_active { "ON" } else { "OFF" },
                        settings.trigger_bot_mode.short_name()
                    ),
                    Some(settings.trigger_bot_active),
                ));
            }
            if settings.watermark_show_rcs {
                hud_items.push((
                    format!(
                        "RCS {} / {}",
                        if settings.rcs_active { "ON" } else { "OFF" },
                        settings.rcs_mode.short_name()
                    ),
                    Some(settings.rcs_active),
                ));
            }
            if settings.watermark_show_map {
                let map_name = self
                    .app_state
                    .resolve::<StateCurrentMap>(())
                    .ok()
                    .and_then(|map| map.current_map.clone())
                    .unwrap_or_else(|| "Unknown".to_owned());
                hud_items.push((format!("{}", map_name), None));
            }
            if settings.watermark_show_esp {
                hud_items.push((
                    format!(
                        "ESP {} / {}",
                        if settings.esp_active { "ON" } else { "OFF" },
                        settings.esp_mode.short_name()
                    ),
                    Some(settings.esp_active),
                ));
            }

            let separator_width = ui.calc_text_size(" | ")[0];
            let total_width = hud_items
                .iter()
                .map(|(text, _)| ui.calc_text_size(text)[0])
                .sum::<f32>()
                + separator_width * hud_items.len().saturating_sub(1) as f32;
            let right_edge = ui.window_size()[0] - 10.0;
            ui.set_cursor_pos([(right_edge - total_width).max(10.0), 10.0]);

            for (index, (text, active)) in hud_items.iter().enumerate() {
                if index > 0 {
                    ui.text_with_shadow(" | ");
                    ui.same_line_with_spacing(0.0, 0.0);
                }
                match active {
                    Some(true) => ui.text_colored_with_shadow(
                        imgui::ImColor32::from_rgba(100, 230, 120, 255),
                        text,
                    ),
                    Some(false) => ui.text_colored_with_shadow(
                        imgui::ImColor32::from_rgba(255, 100, 100, 255),
                        text,
                    ),
                    None => ui.text_with_shadow(text),
                }
                if index + 1 < hud_items.len() {
                    ui.same_line_with_spacing(0.0, 0.0);
                }
            }
        }

        for enhancement in self.enhancements.iter() {
            let hack = enhancement.borrow();
            if let Err(err) = hack.render(&self.app_state, ui, unicode_text) {
                log::error!("{:?}", err);
            }
        }
    }
}

fn main() {
    let args = match AppArgs::try_parse() {
        Ok(args) => args,
        Err(error) => {
            println!("{:#}", error);
            std::process::exit(1);
        }
    };

    env_logger::builder()
        .filter_level(if args.verbose {
            log::LevelFilter::Trace
        } else {
            log::LevelFilter::Info
        })
        .parse_default_env()
        .init();

    let runtime = runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(1)
        .build()
        .expect("to be able to build a runtime");

    let _runtime_guard = runtime.enter();
    if let Err(error) = real_main(&args) {
        show_critical_error(&format!("{:#}", error));
    }
}

#[derive(Debug, Parser)]
#[clap(name = "SwingApp", version)]
struct AppArgs {
    /// Enable verbose logging ($env:RUST_LOG="trace")
    #[clap(short, long)]
    verbose: bool,

    /// Load the CS2 schema (offsets) from a file
    /// instead of resolving them at runtime by the CS2 schema system.
    #[arg(short, long)]
    schema_file: Option<PathBuf>,
}

fn real_main(args: &AppArgs) -> anyhow::Result<()> {
    let launch_token_path = std::env::var("SWING_LAUNCH_TOKEN_PATH")
        .context("This application must be started from the Swing Launcher")?;
    let launch_token = fs::read_to_string(&launch_token_path)
        .context("The launcher token could not be read")?;
    fs::remove_file(&launch_token_path)
        .context("The launcher token could not be consumed")?;
    let token: serde_json::Value = serde_json::from_str(&launch_token)
        .context("The launcher token is invalid")?;
    let token_value = token
        .get("token")
        .and_then(serde_json::Value::as_str)
        .context("The launcher token has no server credential")?;
    let endpoint = token
        .get("endpoint")
        .and_then(serde_json::Value::as_str)
        .context("The launcher token has no verification endpoint")?;
    let hwid = token
        .get("hwid")
        .and_then(serde_json::Value::as_str)
        .context("The launcher token has no HWID")?;
    let anon_key = token
        .get("anonKey")
        .and_then(serde_json::Value::as_str)
        .context("The launcher token has no API key")?;
    let expires_at = token
        .get("expiresAt")
        .and_then(serde_json::Value::as_i64)
        .context("The launcher token has no expiry")?;
    let nonce = token
        .get("nonce")
        .and_then(serde_json::Value::as_str)
        .context("The launcher token has no nonce")?;
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("The launcher token nonce is invalid");
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("The system clock is invalid")?
        .as_millis() as i64;
    if expires_at <= now {
        anyhow::bail!("The launcher token has expired");
    }
    let response = runtime::Handle::current()
        .block_on(async {
            reqwest::Client::new()
                .post(format!("{endpoint}/redeem-launch-token"))
                .header("apikey", anon_key)
                .header("Authorization", format!("Bearer {anon_key}"))
                .json(&serde_json::json!({ "token": token_value, "hwid": hwid }))
                .send()
                .await
        })
        .context("The launcher token could not be verified")?;
    let verification: serde_json::Value = runtime::Handle::current()
        .block_on(async { response.json().await })
        .context("The launcher token response is invalid")?;
    if !verification.get("verified").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        anyhow::bail!("The launcher token was rejected");
    }

    let build_info = version_info()?;
    log::info!(
        "{} v{} ({}). Windows build {}.",
        obfstr!("SwingApp"),
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH"),
        build_info.dwBuildNumber
    );
    log::info!(
        "{} {}",
        obfstr!("Current executable was built on"),
        env!("BUILD_TIME")
    );

    if unsafe { IsUserAnAdmin().as_bool() } {
        log::warn!("{}", obfstr!("Please do not run this as administrator!"));
        log::warn!("{}", obfstr!("Running the controller as administrator might cause failures with your graphic drivers."));
    }

    let username = std::env::var("KEYAUTH_USERNAME")
        .ok()
        .filter(|username| !username.trim().is_empty());
    let mut settings = load_app_settings()?;
    if let Some(username) = username.as_deref() {
        match runtime::Handle::current().block_on(cloud::load_config(username)) {
            Ok(cloud_settings) => {
                settings = cloud_settings;
                if let Err(error) = save_app_settings(&settings) {
                    log::warn!("Failed to cache cloud settings locally: {:#}", error);
                }
                log::info!("Loaded settings from cloud for {}", username);
            }
            Err(error) => {
                log::info!("No cloud settings loaded for {}: {:#}", username, error);
            }
        }
    }
    let cs2 = match CS2Handle::create(settings.metrics) {
        Ok(handle) => handle,
        Err(err) => {
            if let Some(err) = err.downcast_ref::<InterfaceError>() {
                if let Some(detailed_message) = err.detailed_message() {
                    show_critical_error(&detailed_message);
                    return Ok(());
                }
            }

            return Err(err);
        }
    };

    {
        let driver_name = cs2
            .ke_interface
            .driver_version()
            .get_application_name()
            .unwrap_or("<invalid>");

        if driver_name == obfstr!("zenith-driver") {
            let message = [
                obfstr!("You are using Zenith with the CS2 overlay."),
                obfstr!("Topmost overlays may be flagged regardless of using the Zenith driver."),
                obfstr!(""),
                obfstr!("Do you want to continue?"),
            ]
            .join("\n");

            let result = dialog::show_yes_no(obfstr!("SwingApp"), &message, false);
            if !result {
                log::info!("{}", obfstr!("Aborting launch due to user input."));
                return Ok(());
            }
        }
    }

    cs2.add_metrics_record(obfstr!("controller-status"), "initializing");

    let app_state = StateRegistry::new(1024 * 8);
    app_state.set(StateCS2Handle::new(cs2.clone()), ())?;
    app_state.set(StateCS2Memory::new(cs2.create_memory_view()), ())?;
    app_state.set(settings, ())?;

    {
        let cs2_build_info = app_state.resolve::<StateBuildInfo>(()).with_context(|| {
            obfstr!(
                "Failed to load CS2 build info. CS2 version might be newer / older then expected"
            )
            .to_string()
        })?;

        log::info!(
            "Found {}. Revision {} from {}.",
            obfstr!("Counter-Strike 2"),
            cs2_build_info.revision,
            cs2_build_info.build_datetime
        );
        cs2.add_metrics_record(
            obfstr!("cs2-version"),
            &format!("revision: {}", cs2_build_info.revision),
        );
    }

    schema_runtime::setup(
        &app_state,
        &SetupOptions {
            file: args.schema_file.clone(),
            fscache: Some(PathBuf::from_str("cached_schema")?),
        },
    )?;

    let cvars = ConVars::new(&app_state).context("cvars")?;
    let cvar_sensitivity = cvars
        .find_cvar("sensitivity")
        .context("cvar ensitivity")?
        .context("missing cvar sensitivity")?;

    log::debug!("Initialize overlay");
    let app_fonts: AppFonts = Default::default();
    let overlay_options = OverlayOptions {
        title: obfstr!("CS2 Overlay").to_string(),
        target: OverlayTarget::WindowOfProcess(cs2.process_id() as u32),
        register_fonts_callback: Some(Box::new({
            let app_fonts = app_fonts.clone();

            move |atlas| {
                let font_size = 18.0;
                let valthrun_font = atlas.add_font(&[FontSource::TtfData {
                    data: include_bytes!("../resources/Valthrun-Regular.ttf"),
                    size_pixels: font_size,
                    config: Some(FontConfig {
                        rasterizer_multiply: 1.5,
                        oversample_h: 4,
                        oversample_v: 4,
                        ..FontConfig::default()
                    }),
                }]);

                app_fonts.valthrun.set_id(valthrun_font);
            }
        })),
    };

    let mut overlay = match overlay::init(overlay_options) {
        Err(OverlayError::Vulkan(VulkanError::DllNotFound(LoadingError::LibraryLoadFailure(
            source,
        )))) => {
            match &source {
                libloading::Error::LoadLibraryExW { .. } => {
                    let error = source.source().context("LoadLibraryExW to have a source")?;
                    let message = format!("Failed to load vulkan-1.dll.\nError: {:#}", error);
                    show_critical_error(&message);
                }
                error => {
                    let message = format!(
                        "An error occurred while loading vulkan-1.dll.\nError: {:#}",
                        error
                    );
                    show_critical_error(&message);
                }
            }
            return Ok(());
        }
        value => value?,
    };

    {
        let settings = app_state.resolve::<AppSettings>(())?;
        if let Some(imgui_settings) = &settings.imgui {
            overlay.imgui.load_ini_settings(imgui_settings);
        }
    }

    let app = Application {
        fonts: app_fonts,
        app_state,

        cs2: cs2.clone(),

        enhancements: vec![
            Rc::new(RefCell::new(AntiAfk::new())),
            Rc::new(RefCell::new(AutoAccept::new())),
            Rc::new(RefCell::new(Radar::new())),
            Rc::new(RefCell::new(AntiAimPunsh::new(cvar_sensitivity))),
            Rc::new(RefCell::new(PlayerESP::new())),
            Rc::new(RefCell::new(SpectatorsListIndicator::new())),
            Rc::new(RefCell::new(BombInfoIndicator::new())),
            Rc::new(RefCell::new(BombLabelIndicator::new())),
            Rc::new(RefCell::new(TriggerBot::new())),
            Rc::new(RefCell::new(GrenadeHelper::new())),
            Rc::new(RefCell::new(SniperCrosshair::new())),
        ],

        last_total_read_calls: 0,
        frame_read_calls: 0,

        settings_visible: false,

        settings_dirty: false,
        cloud_username: username,
        settings_ui: RefCell::new(SettingsUI::new()),
        /* set the screen capture visibility at the beginning of the first update */
        settings_screen_capture_changed: AtomicBool::new(true),
        settings_render_debug_window_changed: AtomicBool::new(true),
        exit_requested: AtomicBool::new(false),
    };
    let app = Rc::new(RefCell::new(app));

    cs2.add_metrics_record(
        obfstr!("controller-status"),
        &format!(
            "initialized, version: {}, git-hash: {}, win-build: {}",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_HASH"),
            build_info.dwBuildNumber
        ),
    );

    log::info!("{}", obfstr!("App initialized. Spawning overlay."));
    let mut update_fail_count = 0;
    let mut update_timeout: Option<(Instant, Duration)> = None;
    overlay.main_loop(
        {
            let app = app.clone();
            move |controller| {
                let mut app = app.borrow_mut();
                if let Err(err) = app.pre_update(controller) {
                    show_critical_error(&format!("{:#}", err));
                    false
                } else {
                    true
                }
            }
        },
        move |ui, unicode_text| {
            let mut app = app.borrow_mut();

            if let Some((timeout, target)) = &update_timeout {
                if timeout.elapsed() > *target {
                    update_timeout = None;
                } else {
                    /* Not updating. On timeout... */
                    return true;
                }
            }

            if let Err(err) = app.update(ui) {
                if update_fail_count >= 10 {
                    log::error!("Over 10 errors occurred. Waiting 1s and try again.");
                    log::error!("Last error: {:#}", err);

                    update_timeout = Some((Instant::now(), Duration::from_millis(1000)));
                    update_fail_count = 0;
                    return true;
                } else {
                    update_fail_count += 1;
                }
            }

            app.render(ui, unicode_text);
            !app.exit_requested.load(Ordering::Relaxed)
        },
    );

    Ok(())
}
