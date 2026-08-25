use cs2::{
    schema::ConVar,
    MouseState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
};
use overlay::UnicodeTextRenderer;
use raw_struct::Reference;

use super::Enhancement;
use crate::settings::AppSettings;

pub struct AntiAimPunsh {
    mouse_sensitivity: Reference<dyn ConVar>,

    mouse_adjustment_x: i32,
    mouse_adjustment_y: i32,
}

impl AntiAimPunsh {
    pub fn new(mouse_sensitivity: Reference<dyn ConVar>) -> Self {
        Self {
            mouse_sensitivity,

            mouse_adjustment_x: 0,
            mouse_adjustment_y: 0,
        }
    }
}

impl Enhancement for AntiAimPunsh {
    fn update(&mut self, ctx: &crate::UpdateContext) -> anyhow::Result<()> {
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let settings = ctx.states.resolve::<AppSettings>(())?;
        if !settings.aim_assist_recoil || ctx.settings_visible || ctx.app_focus_lost {
            return Ok(());
        }

        let local_controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let local_pawn_handle = match local_controller.instance.value_reference(memory.view_arc()) {
            Some(local_controller) => local_controller.m_hPlayerPawn()?,
            None => return Ok(()),
        };

        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let Some(local_pawn) = entities
            .entity_from_handle(&local_pawn_handle)
            .and_then(|entity| entity.value_reference(memory.view_arc()))
        else {
            self.mouse_adjustment_x = 0;
            self.mouse_adjustment_y = 0;
            return Ok(());
        };

        let min_bullets = settings
            .aim_assist_recoil_min_bullets
            .max(1)
            .min(i32::MAX as u32) as i32;
        if local_pawn.m_iShotsFired()? < min_bullets {
            self.mouse_adjustment_x = 0;
            self.mouse_adjustment_y = 0;
            return Ok(());
        }

        let mouse_sensitivity = self.mouse_sensitivity.fl_value()?;
        if mouse_sensitivity <= f32::EPSILON {
            self.mouse_adjustment_x = 0;
            self.mouse_adjustment_y = 0;
            return Ok(());
        }
        let punch_angle = nalgebra::Vector4::from_row_slice(&local_pawn.m_aimPunchAngle()?) * 2.0;

        let mouse_x = (punch_angle.y / (mouse_sensitivity * 0.022)).round() as i32;
        let mouse_y = (punch_angle.x / (mouse_sensitivity * 0.022)).round() as i32;

        let delta_x = mouse_x - self.mouse_adjustment_x;
        let delta_y = mouse_y - self.mouse_adjustment_y;

        if delta_x != 0 || delta_y != 0 {
            ctx.cs2.send_mouse_state(&[MouseState {
                last_y: -delta_y,
                last_x: delta_x,
                ..Default::default()
            }])?;

            self.mouse_adjustment_x = mouse_x;
            self.mouse_adjustment_y = mouse_y;
        }

        Ok(())
    }

    fn render(
        &self,
        _states: &utils_state::StateRegistry,
        _ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
