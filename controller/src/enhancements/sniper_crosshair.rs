use anyhow::Context;
use cs2::{
    CEntityIdentityEx,
    LocalCameraControllerTarget,
    StateCS2Memory,
    StateEntityList,
    WeaponId,
    WEAPON_FLAG_TYPE_SNIPER_RIFLE,
};
use cs2_schema_generated::cs2::client::{
    CPlayer_WeaponServices,
    C_BasePlayerPawn,
    C_CSPlayerPawn,
    C_EconEntity,
};
use overlay::UnicodeTextRenderer;
use utils_state::StateRegistry;

use super::Enhancement;
use crate::settings::AppSettings;

pub struct SniperCrosshair;

impl SniperCrosshair {
    pub fn new() -> Self {
        Self
    }

    fn is_sniper_weapon(&self, weapon_id: u16) -> bool {
        WeaponId::from_id(weapon_id)
            .map(|weapon| weapon.flags() & WEAPON_FLAG_TYPE_SNIPER_RIFLE != 0)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::SniperCrosshair;
    use cs2::WeaponId;

    #[test]
    fn recognizes_sniper_rifles() {
        let crosshair = SniperCrosshair::new();

        for weapon in [WeaponId::AWP, WeaponId::Ssg08, WeaponId::Scar20, WeaponId::G3SG1] {
            assert!(crosshair.is_sniper_weapon(weapon.id()));
        }
    }

    #[test]
    fn rejects_non_sniper_weapons() {
        let crosshair = SniperCrosshair::new();

        assert!(!crosshair.is_sniper_weapon(WeaponId::Ak47.id()));
    }
}

impl Enhancement for SniperCrosshair {
    fn update(&mut self, _ctx: &crate::UpdateContext) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(
        &self,
        states: &StateRegistry,
        ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;

        if !settings.crosshair && !settings.sniper_crosshair {
            return Ok(());
        }

        let should_draw = if settings.crosshair {
            true
        } else {
            let memory = states.resolve::<StateCS2Memory>(())?;
            let entities = states.resolve::<StateEntityList>(())?;
            let view_target = states.resolve::<LocalCameraControllerTarget>(())?;
            let Some(target_entity_id) = view_target.target_entity_id else {
                return Ok(());
            };

            let player_pawn = entities
                .identity_from_index(target_entity_id)
                .context("missing entity identity")?
                .entity_ptr::<dyn C_CSPlayerPawn>()?
                .value_reference(memory.view_arc())
                .context("player pawn nullptr")?;
            let weapon_services = player_pawn
                .m_pWeaponServices()?
                .value_reference(memory.view_arc())
                .context("m_pWeaponServices nullptr")?;
            let active_weapon_handle = weapon_services
                .cast::<dyn CPlayer_WeaponServices>()
                .m_hActiveWeapon()?;
            let Some(weapon) = entities
                .entity_from_handle(&active_weapon_handle)
                .and_then(|weapon| weapon.value_reference(memory.view_arc()))
            else {
                return Ok(());
            };

            let weapon_id = weapon
                .cast::<dyn C_EconEntity>()
                .m_AttributeManager()?
                .m_Item()?
                .m_iItemDefinitionIndex()?;

            self.is_sniper_weapon(weapon_id)
        };

        if !should_draw {
            return Ok(());
        }

        let draw = ui.get_window_draw_list();
        let display_size = ui.io().display_size;
        let screen_center = [display_size[0] / 2.0, display_size[1] / 2.0];
        let mut color = settings.crosshair_color.as_f32();
        color[3] *= settings.crosshair_opacity.clamp(0.1, 1.0);
        let outline_color = [0.0, 0.0, 0.0, color[3]];
        let gap = settings.crosshair_gap.max(0.0);
        let length = settings.crosshair_length.max(1.0);
        let thickness = settings.crosshair_thickness.max(1.0);
        let outline_width = if settings.crosshair_outline { thickness + 2.0 } else { 0.0 };

        if settings.crosshair && settings.crosshair_lines {
            for (index, (start, end)) in [
                ([screen_center[0] - gap - length, screen_center[1]], [screen_center[0] - gap, screen_center[1]]),
                ([screen_center[0] + gap, screen_center[1]], [screen_center[0] + gap + length, screen_center[1]]),
                ([screen_center[0], screen_center[1] - gap - length], [screen_center[0], screen_center[1] - gap]),
                ([screen_center[0], screen_center[1] + gap], [screen_center[0], screen_center[1] + gap + length]),
            ].into_iter().enumerate() {
                if settings.crosshair_t_style && index == 2 {
                    continue;
                }
                if settings.crosshair_outline {
                    draw.add_line(start, end, outline_color)
                        .thickness(outline_width)
                        .build();
                }
                draw.add_line(start, end, color)
                    .thickness(thickness)
                    .build();
            }
        }

        if settings.crosshair && settings.crosshair_center_dot {
            let dot_size = settings.crosshair_dot_size.max(1.5);
            if settings.crosshair_outline {
                draw.add_circle(screen_center, dot_size + 1.0, outline_color)
                    .filled(true)
                    .build();
            }
            draw.add_circle(screen_center, dot_size, color)
                .filled(true)
                .build();
        }

        if !settings.crosshair {
            draw.add_circle(screen_center, 3.5, [0.0, 0.0, 0.0, 0.8])
                .filled(true)
                .build();
            draw.add_circle(screen_center, 2.0, [1.0, 1.0, 1.0, 0.8])
                .filled(true)
                .build();
        }

        Ok(())
    }

    fn render_debug_window(
        &mut self,
        _states: &StateRegistry,
        _ui: &imgui::Ui,
        _unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
