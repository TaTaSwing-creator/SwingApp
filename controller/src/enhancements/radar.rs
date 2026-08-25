use cs2::{
    CEntityIdentityEx,
    ClassNameCache,
    PlayerPawnState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
    StatePawnInfo,
};
use imgui::{ImColor32, MouseButton};

use super::Enhancement;
use crate::{
    settings::AppSettings,
    view::ViewController,
    UpdateContext,
};

struct RadarPlayer {
    position: nalgebra::Vector3<f32>,
    is_teammate: bool,
}

pub struct Radar {
    players: Vec<RadarPlayer>,
    center: nalgebra::Vector3<f32>,
    local_team_id: u8,
    dragging: bool,
    drag_offset: [f32; 2],
}

impl Radar {
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
            center: nalgebra::Vector3::zeros(),
            local_team_id: 0,
            dragging: false,
            drag_offset: [0.0, 0.0],
        }
    }
}

impl Enhancement for Radar {
    fn update_settings(
        &mut self,
        ui: &imgui::Ui,
        settings: &mut AppSettings,
    ) -> anyhow::Result<bool> {
        if !settings.radar || !settings.bomb_timer_edit_mode {
            self.dragging = false;
            return Ok(false);
        }

        let display_size = ui.io().display_size;
        let radar_position = [settings.radar_position_x, settings.radar_position_y];
        let size = settings.radar_size.clamp(100.0, 400.0);
        let mouse_position = ui.io().mouse_pos;
        let inside_radar = mouse_position[0] >= radar_position[0]
            && mouse_position[0] <= radar_position[0] + size
            && mouse_position[1] >= radar_position[1]
            && mouse_position[1] <= radar_position[1] + size;

        if !self.dragging && inside_radar && ui.is_mouse_clicked(MouseButton::Left) {
            self.dragging = true;
            self.drag_offset = [
                mouse_position[0] - radar_position[0],
                mouse_position[1] - radar_position[1],
            ];
        }

        if !self.dragging {
            return Ok(false);
        }

        if !ui.is_mouse_down(MouseButton::Left) {
            self.dragging = false;
            return Ok(false);
        }

        settings.radar_position_x = (mouse_position[0] - self.drag_offset[0])
            .clamp(0.0, (display_size[0] - size).max(0.0));
        settings.radar_position_y = (mouse_position[1] - self.drag_offset[1])
            .clamp(0.0, (display_size[1] - size).max(0.0));

        Ok(true)
    }

    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        self.players.clear();
        if !settings.radar {
            return Ok(());
        }

        let view = ctx.states.resolve::<ViewController>(())?;
        self.center = view.get_camera_world_position().unwrap_or_default();

        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let class_name_cache = ctx.states.resolve::<ClassNameCache>(())?;
        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        if let Ok(local_controller) = ctx.states.resolve::<StateLocalPlayerController>(()) {
            if let Some(local_controller) = local_controller
                .instance
                .value_reference(memory.view_arc())
            {
                self.local_team_id = local_controller.m_iPendingTeamNum()?;
            }
        }

        for entity_identity in entities.entities() {
            let entity_class = class_name_cache.lookup(&entity_identity.entity_class_info()?)?;
            if entity_class.map(|name| *name != "C_CSPlayerPawn").unwrap_or(true) {
                continue;
            }

            let handle = entity_identity.handle()?;
            let pawn_state = ctx.states.resolve::<PlayerPawnState>(handle)?;
            if *pawn_state != PlayerPawnState::Alive {
                continue;
            }

            let pawn_info = ctx.states.resolve::<StatePawnInfo>(handle)?;
            self.players.push(RadarPlayer {
                position: pawn_info.position,
                is_teammate: pawn_info.team_id == self.local_team_id,
            });
        }

        Ok(())
    }

    fn render(
        &self,
        states: &utils_state::StateRegistry,
        ui: &imgui::Ui,
        _unicode_text: &overlay::UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        if !states.resolve::<AppSettings>(())?.radar {
            return Ok(());
        }

        let settings = states.resolve::<AppSettings>(())?;
        let size = settings.radar_size.clamp(100.0, 400.0);
        let range = settings.radar_range.clamp(500.0, 4000.0);
        let origin = [settings.radar_position_x, settings.radar_position_y];
        let center = [origin[0] + size / 2.0, origin[1] + size / 2.0];
        let draw = ui.get_window_draw_list();

        draw.add_rect(
            origin,
            [origin[0] + size, origin[1] + size],
            ImColor32::from_rgba(12, 16, 20, (settings.radar_opacity * 255.0) as u8),
        )
        .filled(true)
        .build();
        draw.add_rect(
            origin,
            [origin[0] + size, origin[1] + size],
            ImColor32::from_rgba(150, 165, 170, 150),
        )
        .thickness(1.0)
        .build();
        draw.add_line(
            [center[0] - 8.0, center[1]],
            [center[0] + 8.0, center[1]],
            ImColor32::from_rgba(245, 245, 245, 255),
        )
        .thickness(2.0)
        .build();
        draw.add_line(
            [center[0], center[1] - 8.0],
            [center[0], center[1] + 8.0],
            ImColor32::from_rgba(245, 245, 245, 255),
        )
        .thickness(2.0)
        .build();

        for player in &self.players {
            let offset = player.position - self.center;
            let x = (offset.x / range * size / 2.0)
                .clamp(-size / 2.0 + 6.0, size / 2.0 - 6.0);
            let y = (-offset.y / range * size / 2.0)
                .clamp(-size / 2.0 + 6.0, size / 2.0 - 6.0);
            let color = if player.is_teammate {
                ImColor32::from_rgba(65, 165, 255, 255)
            } else {
                ImColor32::from_rgba(240, 75, 75, 255)
            };
            draw.add_circle([center[0] + x, center[1] + y], 4.0, color)
                .filled(true)
                .build();
        }

        Ok(())
    }
}