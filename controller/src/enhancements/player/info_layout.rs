use imgui::{
    DrawListMut,
    ImColor32,
};

use crate::utils::TextWithShadowDrawList;
use crate::settings::EspInfoPosition;

pub struct PlayerInfoLayout<'a> {
    ui: &'a imgui::Ui,
    draw: &'a DrawListMut<'a>,

    vmin: nalgebra::Vector2<f32>,
    vmax: nalgebra::Vector2<f32>,

    line_counts: [usize; 4],
    font_scale: f32,

}

impl<'a> PlayerInfoLayout<'a> {
    pub fn new(
        ui: &'a imgui::Ui,
        draw: &'a DrawListMut<'a>,
        screen_bounds: mint::Vector2<f32>,
        vmin: nalgebra::Vector2<f32>,
        vmax: nalgebra::Vector2<f32>,
    ) -> Self {
        let target_scale_raw = (vmax.y - vmin.y) / screen_bounds.y * 8.0;
        let target_scale = target_scale_raw.clamp(0.5, 1.25);
        ui.set_window_font_scale(target_scale);

        Self {
            ui,
            draw,

            vmin,
            vmax,

            line_counts: [0; 4],
            font_scale: target_scale,

        }
    }

    pub fn add_line(
        &mut self,
        position: EspInfoPosition,
        color: impl Into<ImColor32>,
        text: &str,
    ) {
        let [text_width, _] = self.ui.calc_text_size(text);

        let mut pos = match position {
            EspInfoPosition::Left => [self.vmin.x - text_width - 5.0, self.vmin.y],
            EspInfoPosition::Right => [self.vmax.x + 5.0, self.vmin.y],
            EspInfoPosition::Top => [
                (self.vmin.x + self.vmax.x - text_width) / 2.0,
                self.vmin.y - self.font_scale * self.ui.text_line_height() - 4.0,
            ],
            EspInfoPosition::Bottom => [
                (self.vmin.x + self.vmax.x - text_width) / 2.0,
                self.vmax.y + 4.0,
            ],
        };

        let position_index = match position {
            EspInfoPosition::Left => 0,
            EspInfoPosition::Right => 1,
            EspInfoPosition::Top => 2,
            EspInfoPosition::Bottom => 3,
        };
        pos[1] += self.line_counts[position_index] as f32
            * self.font_scale
            * self.ui.text_line_height()
            + 4.0 * self.line_counts[position_index] as f32;

        self.draw.add_text_with_shadow(pos, color, text);
        self.line_counts[position_index] += 1;
    }
}

impl Drop for PlayerInfoLayout<'_> {
    fn drop(&mut self) {
        self.ui.set_window_font_scale(1.0);
    }
}
