use std::time::Instant;

use anyhow::Context;
use cs2::{
    MouseState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
    StatePawnInfo,
};
use cs2_schema_cutl::EntityHandle;
use cs2_schema_generated::cs2::client::{
    C_BaseEntity,
    C_CSPlayerPawn,
};
use obfstr::obfstr;
use overlay::UnicodeTextRenderer;
use rand::{
    distributions::Uniform,
    prelude::Distribution,
};
use utils_state::StateRegistry;

use super::Enhancement;
use crate::{
    settings::{
        AppSettings,
        TriggerBotWeaponCategory,
    },
    view::{
        KeyToggle,
        StateLocalCrosshair,
    },
    UpdateContext,
};

enum TriggerState {
    Idle,
    Pending { delay: u32, timestamp: Instant },
    Sleep { delay: u32, timestamp: Instant },
    Active,
}

pub struct TriggerBot {
    toggle: KeyToggle,
    rcs_toggle: KeyToggle,
    state: TriggerState,
    trigger_active: bool,
    rcs_started_at: Option<Instant>,
    rcs_last_update: Instant,
    rcs_remainder: f32,
}

impl TriggerBot {
    pub fn new() -> Self {
        Self {
            toggle: KeyToggle::new(),
            rcs_toggle: KeyToggle::new(),
            state: TriggerState::Idle,
            trigger_active: false,
            rcs_started_at: None,
            rcs_last_update: Instant::now(),
            rcs_remainder: 0.0,
        }
    }

    fn update_rcs(&mut self, ctx: &UpdateContext, settings: &AppSettings) -> anyhow::Result<()> {
        self.rcs_toggle
            .update(&settings.rcs_mode, ctx.input, &settings.key_rcs);
        let now = Instant::now();
        if ctx.settings_visible
            || ctx.app_focus_lost
            || !self.rcs_toggle.enabled
            || !ctx.input.is_key_down(imgui::Key::MouseLeft)
        {
            self.rcs_started_at = None;
            self.rcs_last_update = now;
            self.rcs_remainder = 0.0;
            return Ok(());
        }

        let rcs_settings = self.current_rcs_settings(ctx, settings)?;
        if !rcs_settings.enabled {
            self.rcs_started_at = None;
            self.rcs_last_update = now;
            self.rcs_remainder = 0.0;
            return Ok(());
        }

        let started_at = *self.rcs_started_at.get_or_insert(now);
        let elapsed = started_at.elapsed().as_millis();
        let delta_time = self.rcs_last_update.elapsed().as_secs_f32();
        self.rcs_last_update = now;
        if elapsed < rcs_settings.delay as u128 {
            return Ok(());
        }

        let smoothing = rcs_settings.smoothing.max(1) as f32;
        let movement = rcs_settings.strength.max(0.0)
            * delta_time
            * 60.0
            * (150.0 / smoothing)
            + self.rcs_remainder;
        let vertical = movement.floor() as i32;
        self.rcs_remainder = movement.fract();
        if vertical == 0 {
            return Ok(());
        }

        let horizontal = if rcs_settings.jitter {
            let amount = rcs_settings.jitter_amount.max(0.0);
            let whole_amount = amount.floor() as i32;
            let fractional_amount = amount.fract();
            let effective_amount = whole_amount
                + i32::from(rand::random::<f32>() < fractional_amount);
            if effective_amount == 0 {
                0
            } else if rand::random::<bool>() {
                effective_amount
            } else {
                -effective_amount
            }
        } else {
            0
        };
        ctx.cs2.send_mouse_state(&[MouseState {
            last_x: horizontal,
            last_y: vertical,
            ..Default::default()
        }])?;
        Ok(())
    }

    fn should_be_active(&self, ctx: &UpdateContext) -> anyhow::Result<bool> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        let crosshair = ctx.states.resolve::<StateLocalCrosshair>(())?;
        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;

        let target = match crosshair.current_target() {
            Some(target) => target,
            None => return Ok(false),
        };

        if !target
            .entity_type
            .as_ref()
            .map(|t| t == "C_CSPlayerPawn")
            .unwrap_or(false)
        {
            return Ok(false);
        }

        if settings.trigger_bot_team_check {
            let crosshair_entity = entities
                .entity_from_handle(&EntityHandle::<dyn C_CSPlayerPawn>::from_index(
                    target.entity_id,
                ))
                .context("missing crosshair player pawn")?
                .value_reference(memory.view_arc())
                .context("entity nullptr")?;

            let local_player_controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
            let Some(local_player_controller) = local_player_controller
                .instance
                .value_reference(memory.view_arc())
            else {
                return Ok(false);
            };

            if crosshair_entity.m_iTeamNum()? == local_player_controller.m_iTeamNum()? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn current_weapon_settings<'a>(
        &self,
        ctx: &UpdateContext,
        settings: &'a AppSettings,
    ) -> anyhow::Result<&'a crate::settings::TriggerBotWeaponSettings> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(controller) = controller.instance.value_reference(memory.view_arc()) else {
            anyhow::bail!("local player controller is unavailable")
        };
        let pawn_handle = controller.m_hPlayerPawn()?;
        let pawn_info = ctx.states.resolve::<StatePawnInfo>(pawn_handle)?;
        let category = TriggerBotWeaponCategory::from_weapon(pawn_info.weapon);
        Ok(settings
            .trigger_bot_weapon_settings
            .get(&category)
            .or_else(|| settings.trigger_bot_weapon_settings.get(&TriggerBotWeaponCategory::Other))
            .expect("trigger bot weapon settings defaults are initialized"))
    }

    fn current_weapon_has_ammo(&self, ctx: &UpdateContext) -> anyhow::Result<bool> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(controller) = controller.instance.value_reference(memory.view_arc()) else {
            return Ok(false);
        };
        let pawn_info = ctx
            .states
            .resolve::<StatePawnInfo>(controller.m_hPlayerPawn()?)?;
        Ok(pawn_info.weapon_current_ammo > 0)
    }

    fn current_rcs_settings<'a>(
        &self,
        ctx: &UpdateContext,
        settings: &'a AppSettings,
    ) -> anyhow::Result<&'a crate::settings::RcsWeaponSettings> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(controller) = controller.instance.value_reference(memory.view_arc()) else {
            anyhow::bail!("local player controller is unavailable")
        };
        let pawn_info = ctx.states.resolve::<StatePawnInfo>(controller.m_hPlayerPawn()?)?;
        let category = TriggerBotWeaponCategory::from_weapon(pawn_info.weapon);
        Ok(settings
            .rcs_weapon_settings
            .get(&category)
            .or_else(|| settings.rcs_weapon_settings.get(&TriggerBotWeaponCategory::Other))
            .expect("rcs weapon settings defaults are initialized"))
    }
}

impl Enhancement for TriggerBot {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        self.update_rcs(ctx, &settings)?;
        if self.toggle.update(
            &settings.trigger_bot_mode,
            ctx.input,
            &settings.key_trigger_bot,
        ) {
            ctx.cs2.add_metrics_record(
                obfstr!("feature-trigger-bot-toggle"),
                &format!(
                    "enabled: {}, mode: {:?}",
                    self.toggle.enabled, settings.trigger_bot_mode
                ),
            );
        }

        let should_shoot: bool = if self.toggle.enabled
            && !ctx.settings_visible
            && !ctx.app_focus_lost
        {
            let weapon_settings = self.current_weapon_settings(ctx, &settings)?;
            weapon_settings.enabled
                && self.current_weapon_has_ammo(ctx)?
                && self.should_be_active(ctx)?
        } else {
            false
        };
        let trigger_bot_active = self.toggle.enabled;
        let rcs_active = self.rcs_toggle.enabled
            && !ctx.settings_visible
            && !ctx.app_focus_lost;
        drop(settings);
        let mut runtime_settings = ctx.states.resolve_mut::<AppSettings>(())?;
        runtime_settings.trigger_bot_active = trigger_bot_active;
        runtime_settings.rcs_active = rcs_active;
        drop(runtime_settings);
        let settings = ctx.states.resolve::<AppSettings>(())?;

        loop {
            match &self.state {
                TriggerState::Idle => {
                    if !should_shoot {
                        /* nothing changed */
                        break;
                    }

                    let weapon_settings = self.current_weapon_settings(ctx, &settings)?;
                    let delay_min = weapon_settings.delay_min.min(weapon_settings.delay_max);
                    let delay_max = weapon_settings.delay_min.max(weapon_settings.delay_max);
                    let selected_delay = if delay_max == delay_min {
                        delay_min
                    } else {
                        let dist = Uniform::new_inclusive(delay_min, delay_max);
                        dist.sample(&mut rand::thread_rng())
                    };

                    log::trace!(
                        "Setting trigger bot into pending mode with a delay of {}ms",
                        selected_delay
                    );
                    self.state = TriggerState::Pending {
                        delay: selected_delay,
                        timestamp: Instant::now(),
                    };
                }
                TriggerState::Pending { delay, timestamp } => {
                    let time_elapsed = timestamp.elapsed().as_millis();
                    if time_elapsed < *delay as u128 {
                        /* still waiting to be activated */
                        break;
                    }

                    if settings.trigger_bot_check_target_after_delay && !should_shoot {
                        self.state = TriggerState::Idle;
                    } else {
                        self.state = TriggerState::Active;
                    }
                    /* regardless of the next state, we always need to execute the current action */
                    break;
                }
                TriggerState::Sleep { delay, timestamp } => {
                    let time_elapsed = timestamp.elapsed().as_millis();
                    if time_elapsed < *delay as u128 {
                        /* still waiting to be activated */
                        break;
                    }
                    self.state = TriggerState::Idle;
                    break;
                }
                TriggerState::Active => {
                    if should_shoot {
                        /* nothing changed */
                        break;
                    }

                    self.state = TriggerState::Idle;
                }
            }
        }

        let should_be_active = matches!(self.state, TriggerState::Active);
        if should_be_active != self.trigger_active {
            self.trigger_active = should_be_active;

            let mut state = MouseState {
                ..Default::default()
            };
            state.buttons[0] = Some(self.trigger_active);
            ctx.cs2.send_mouse_state(&[state])?;
            log::trace!("Setting shoot state to {}", self.trigger_active);

            let weapon_settings = self.current_weapon_settings(ctx, &settings)?;
            self.state = TriggerState::Sleep {
                delay: weapon_settings.shot_duration,
                timestamp: Instant::now(),
            };
        }

        Ok(())
    }

    fn render(
        &self,
        _states: &StateRegistry,
        _ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
