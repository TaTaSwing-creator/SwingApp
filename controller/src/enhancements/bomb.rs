use anyhow::Context;
use cs2::{
    state::PlantedC4,
    BombCarrierInfo,
    CEntityIdentityEx,
    ClassNameCache,
    PlantedC4State,
    StateCS2Memory,
    StateEntityList,
};
use cs2_schema_generated::cs2::client::{
    C_BaseEntity,
    C_C4,
};
use imgui::{
    ImColor32,
    MouseButton,
};
use overlay::UnicodeTextRenderer;

use super::Enhancement;
use crate::{
    settings::AppSettings,
    utils::{
        TextWithShadowUi,
        UnicodeTextWithShadowUi,
    },
    view::ViewController,
};

pub struct BombInfoIndicator {
    dragging: bool,
    drag_offset: [f32; 2],
}
impl BombInfoIndicator {
    pub fn new() -> Self {
        Self {
            dragging: false,
            drag_offset: [0.0, 0.0],
        }
    }
}

const BOMB_TIMER_COLOR: ImColor32 = ImColor32::from_rgba(255, 0, 0, 255);
const BOMB_TIMER_PANEL_SIZE: [f32; 2] = [230.0, 92.0];

impl Enhancement for BombInfoIndicator {
    fn update(&mut self, _ctx: &crate::UpdateContext) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_settings(
        &mut self,
        ui: &imgui::Ui,
        settings: &mut AppSettings,
    ) -> anyhow::Result<bool> {
        if !settings.bomb_timer_edit_mode {
            self.dragging = false;
            return Ok(false);
        }

        let display_size = ui.io().display_size;
        let panel_position = [
            display_size[0] * settings.bomb_timer_position_x,
            display_size[1] * settings.bomb_timer_position_y,
        ];
        let mouse_position = ui.io().mouse_pos;
        let inside_panel = mouse_position[0] >= panel_position[0]
            && mouse_position[0] <= panel_position[0] + BOMB_TIMER_PANEL_SIZE[0]
            && mouse_position[1] >= panel_position[1]
            && mouse_position[1] <= panel_position[1] + BOMB_TIMER_PANEL_SIZE[1];

        if !self.dragging && inside_panel && ui.is_mouse_clicked(MouseButton::Left) {
            self.dragging = true;
            self.drag_offset = [
                mouse_position[0] - panel_position[0],
                mouse_position[1] - panel_position[1],
            ];
        }

        if !self.dragging {
            return Ok(false);
        }

        if !ui.is_mouse_down(MouseButton::Left) {
            self.dragging = false;
            return Ok(false);
        }

        let max_x = (display_size[0] - BOMB_TIMER_PANEL_SIZE[0]).max(0.0);
        let max_y = (display_size[1] - BOMB_TIMER_PANEL_SIZE[1]).max(0.0);
        settings.bomb_timer_position_x = ((mouse_position[0] - self.drag_offset[0]) / display_size[0])
            .clamp(0.0, max_x / display_size[0]);
        settings.bomb_timer_position_y = ((mouse_position[1] - self.drag_offset[1]) / display_size[1])
            .clamp(0.0, max_y / display_size[1]);

        Ok(true)
    }

    fn render(
        &self,
        states: &utils_state::StateRegistry,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;
        let bomb_state = states.resolve::<PlantedC4>(())?;

        if !settings.bomb_timer {
            return Ok(());
        }

        if matches!(bomb_state.state, PlantedC4State::NotPlanted) {
            return Ok(());
        }

        let offset_x = ui.io().display_size[0] * settings.bomb_timer_position_x;
        let offset_y = ui.io().display_size[1] * settings.bomb_timer_position_y;
        let draw_list = ui.get_window_draw_list();
        let panel_end = [
            offset_x + BOMB_TIMER_PANEL_SIZE[0],
            offset_y + BOMB_TIMER_PANEL_SIZE[1],
        ];

        draw_list
            .add_rect(
                [offset_x, offset_y],
                panel_end,
                ImColor32::from_rgba(10, 10, 13, 225),
            )
            .filled(true)
            .rounding(7.0)
            .build();
        draw_list
            .add_rect(
                [offset_x, offset_y],
                panel_end,
                ImColor32::from_rgba(255, 0, 0, 220),
            )
            .rounding(7.0)
            .thickness(1.5)
            .build();
        draw_list
            .add_rect(
                [offset_x, offset_y],
                [offset_x + 5.0, panel_end[1]],
                BOMB_TIMER_COLOR,
            )
            .filled(true)
            .build();

        let content_x = offset_x + 16.0;
        ui.set_cursor_pos([content_x, offset_y + 9.0]);
        ui.text_colored_with_shadow(
            ImColor32::from_rgba(255, 125, 125, 255),
            &format!(
                "C4  /  SITE {}",
                if bomb_state.bomb_site == 0 { "A" } else { "B" }
            ),
        );

        let mut text_y = offset_y + 30.0;

        match &bomb_state.state {
            PlantedC4State::Active { time_detonation } => {
                ui.set_cursor_pos([content_x, text_y]);
                ui.text_colored_with_shadow(BOMB_TIMER_COLOR, &format!("{:.3}s", time_detonation));

                text_y += 29.0;

                if let Some(defuser) = &bomb_state.defuser {
                    let defuse_text = format!(
                        "Defused in {:.3} by {}",
                        defuser.time_remaining, defuser.player_name
                    );

                    ui.set_cursor_pos([content_x, text_y]);
                    ui.unicode_text_colored_with_shadow(unicode_text, BOMB_TIMER_COLOR, &defuse_text);
                } else {
                    ui.set_cursor_pos([content_x, text_y]);
                    ui.text_colored_with_shadow(BOMB_TIMER_COLOR, "Not defusing");
                }
            }
            PlantedC4State::Defused => {
                ui.set_cursor_pos([content_x, text_y]);
                ui.text_colored_with_shadow(BOMB_TIMER_COLOR, "Bomb has been defused");
            }
            PlantedC4State::Detonated => {
                ui.set_cursor_pos([content_x, text_y]);
                ui.text_colored_with_shadow(BOMB_TIMER_COLOR, "Bomb has been detonated");
            }
            PlantedC4State::NotPlanted => unreachable!(),
        }
        Ok(())
    }
}

pub struct BombLabelIndicator {}
impl BombLabelIndicator {
    pub fn new() -> Self {
        Self {}
    }

    /// Render bomb label text above the bomb
    fn render_bomb_text(
        &self,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
        view: &ViewController,
        position: &nalgebra::Vector3<f32>,
        color: ImColor32,
    ) -> anyhow::Result<()> {
        if let Some(screen_pos) = view.world_to_screen(position, false) {
            let text = "Bomb";
            let text_size = ui.calc_text_size(text);

            // Position text above the bomb
            let text_x = screen_pos.x - text_size[0] / 2.0;
            let text_y = screen_pos.y - 30.0;

            ui.set_cursor_pos([text_x, text_y]);
            ui.unicode_text_colored_with_shadow(unicode_text, color, text);
        }
        Ok(())
    }
}

impl Enhancement for BombLabelIndicator {
    fn update(&mut self, _ctx: &crate::UpdateContext) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(
        &self,
        states: &utils_state::StateRegistry,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;
        let bomb_state = states.resolve::<PlantedC4>(())?;
        let bomb_carrier = states.resolve::<BombCarrierInfo>(())?;
        let view = states.resolve::<ViewController>(())?;

        if !settings.bomb_label {
            return Ok(());
        }

        // Show bomb label for planted bombs
        if !matches!(bomb_state.state, PlantedC4State::NotPlanted) {
            self.render_bomb_text(
                ui,
                unicode_text,
                &view,
                &bomb_state.position,
                ImColor32::from_rgba(255, 0, 0, 255), // Red color for planted bomb
            )?;
        }

        // Show bomb label for dropped C4 entities (when not being carried)
        if bomb_carrier.carrier_entity_id.is_none() {
            let memory = states.resolve::<StateCS2Memory>(())?;
            let entities = states.resolve::<StateEntityList>(())?;
            let class_name_cache = states.resolve::<ClassNameCache>(())?;

            for entity_identity in entities.entities().iter() {
                let class_name = class_name_cache
                    .lookup(&entity_identity.entity_class_info()?)
                    .context("class name")?;

                if !class_name.map(|name| name == "C_C4").unwrap_or(false) {
                    continue;
                }

                let c4_entity = entity_identity
                    .entity_ptr::<dyn C_C4>()?
                    .value_copy(memory.view())?
                    .context("C4 entity nullptr")?;

                // Skip if bomb is planted
                if c4_entity.m_bBombPlanted()? {
                    continue;
                }

                // Get the position of the dropped C4
                let game_scene_node = entity_identity
                    .entity_ptr::<dyn C_BaseEntity>()?
                    .value_reference(memory.view_arc())
                    .context("C_BaseEntity pointer was null")?
                    .m_pGameSceneNode()?
                    .value_reference(memory.view_arc())
                    .context("m_pGameSceneNode pointer was null")?
                    .copy()?;

                let position = game_scene_node.m_vecAbsOrigin()?;

                self.render_bomb_text(
                    ui,
                    unicode_text,
                    &view,
                    &position.into(),
                    ImColor32::from_rgba(255, 165, 0, 255), // Orange color for dropped bomb
                )?;
            }
        }

        Ok(())
    }
}
