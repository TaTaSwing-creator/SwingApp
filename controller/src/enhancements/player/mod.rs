use cs2::{
    BoneFlags,
    CEntityIdentityEx,
    CS2Model,
    ClassNameCache,
    LocalCameraControllerTarget,
    PlayerPawnState,
    StateCS2Memory,
    StateEntityList,
    StateLocalPlayerController,
    StatePawnInfo,
    StatePawnModelInfo,
};
use info_layout::PlayerInfoLayout;
use obfstr::obfstr;
use overlay::UnicodeTextRenderer;

use super::Enhancement;
use crate::{
    settings::{
        AppSettings,
        EspBoxType,
        EspConfig,
        EspHeadDot,
        EspHealthBar,
        EspPlayerSettings,
        EspSelector,
    },
    view::{
        KeyToggle,
        ViewController,
    },
};

mod info_layout;

struct PlayerESPInfo {
    pawn_info: StatePawnInfo,
    pawn_model: StatePawnModelInfo,
}

pub struct PlayerESP {
    toggle: KeyToggle,
    players: Vec<PlayerESPInfo>,
    local_team_id: u8,
}

impl PlayerESP {
    pub fn new() -> Self {
        PlayerESP {
            toggle: KeyToggle::new(),
            players: Default::default(),
            local_team_id: 0,
        }
    }

    fn resolve_esp_player_config<'a>(
        &self,
        settings: &'a AppSettings,
        target: &StatePawnInfo,
    ) -> Option<&'a EspPlayerSettings> {
        let mut esp_target = Some(EspSelector::PlayerTeamVisibility {
            enemy: target.team_id != self.local_team_id,
            visible: true, // TODO: Implement visibility, maybe rename it to spottet!
        });

        while let Some(target) = esp_target.take() {
            let config_key = target.config_key();

            if settings
                .esp_settings_enabled
                .get(&config_key)
                .cloned()
                .unwrap_or_default()
            {
                if let Some(settings) = settings.esp_settings.get(&config_key) {
                    if let EspConfig::Player(settings) = settings {
                        return Some(settings);
                    }
                }
            }

            esp_target = target.parent();
        }

        None
    }

    fn draw_offscreen_arrow(
        &self,
        draw: &imgui::DrawListMut,
        position: mint::Vector2<f32>,
        angle: f32,
        size: f32,
        color: [f32; 4],
    ) {
        // Create arrow pointing to the right (0 radians)
        // Then rotate it based on the angle
        let arrow_points = [
            [size, 0.0],                // Tip
            [-size * 0.5, size * 0.6],  // Bottom
            [-size * 0.5, -size * 0.6], // Top
        ];

        let cos_angle = angle.cos();
        let sin_angle = angle.sin();

        let rotated_points: Vec<[f32; 2]> = arrow_points
            .iter()
            .map(|[x, y]| {
                [
                    position.x + x * cos_angle - y * sin_angle,
                    position.y + x * sin_angle + y * cos_angle,
                ]
            })
            .collect();

        draw.add_triangle(
            rotated_points[0],
            rotated_points[1],
            rotated_points[2],
            color,
        )
        .filled(true)
        .build();
    }

    fn draw_box_corners(
        draw: &imgui::DrawListMut,
        min: [f32; 2],
        max: [f32; 2],
        color: [f32; 4],
        thickness: f32,
    ) {
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
            draw.add_line(start, end, color)
                .thickness(thickness)
                .build();
        }
    }

}

impl Enhancement for PlayerESP {
    fn update(&mut self, ctx: &crate::UpdateContext) -> anyhow::Result<()> {
        let entities = ctx.states.resolve::<StateEntityList>(())?;
        let class_name_cache = ctx.states.resolve::<ClassNameCache>(())?;
        let settings = ctx.states.resolve::<AppSettings>(())?;
        if self
            .toggle
            .update(&settings.esp_mode, ctx.input, &settings.esp_toggle)
        {
            ctx.cs2.add_metrics_record(
                obfstr!("feature-esp-toggle"),
                &format!(
                    "enabled: {}, mode: {:?}",
                    self.toggle.enabled, settings.esp_mode
                ),
            );
        }

        let esp_active = self.toggle.enabled;
        drop(settings);
        ctx.states.resolve_mut::<AppSettings>(())?.esp_active = esp_active;

        self.players.clear();
        if !self.toggle.enabled {
            return Ok(());
        }

        self.players.reserve(16);

        let memory = ctx.states.resolve::<StateCS2Memory>(())?;
        let local_player_controller = ctx.states.resolve::<StateLocalPlayerController>(())?;
        let Some(local_player_controller) = local_player_controller
            .instance
            .value_reference(memory.view_arc())
        else {
            return Ok(());
        };

        self.local_team_id = local_player_controller.m_iPendingTeamNum()?;

        let view_target = ctx.states.resolve::<LocalCameraControllerTarget>(())?;
        let view_target_entity_id = match &view_target.target_entity_id {
            Some(value) => *value,
            None => return Ok(()),
        };

        for entity_identity in entities.entities() {
            if entity_identity.handle::<()>()?.get_entity_index() == view_target_entity_id {
                continue;
            }

            let entity_class = class_name_cache.lookup(&entity_identity.entity_class_info()?)?;
            if !entity_class
                .map(|name| *name == "C_CSPlayerPawn")
                .unwrap_or(false)
            {
                /* entity is not a player pawn */
                continue;
            }

            let pawn_state = ctx
                .states
                .resolve::<PlayerPawnState>(entity_identity.handle()?)?;
            if *pawn_state != PlayerPawnState::Alive {
                continue;
            }

            let pawn_info = ctx
                .states
                .resolve::<StatePawnInfo>(entity_identity.handle()?)?;

            if pawn_info.player_health <= 0 || pawn_info.player_name.is_none() {
                continue;
            }

            let pawn_model = ctx
                .states
                .resolve::<StatePawnModelInfo>(entity_identity.handle()?)?;

            self.players.push(PlayerESPInfo {
                pawn_info: pawn_info.clone(),
                pawn_model: pawn_model.clone(),
            });
        }

        Ok(())
    }

    fn render(
        &self,
        states: &utils_state::StateRegistry,
        ui: &imgui::Ui,
        unicode_text: &UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        let settings = states.resolve::<AppSettings>(())?;
        let view = states.resolve::<ViewController>(())?;

        let draw = ui.get_window_draw_list();
        if settings.aim_assist {
            let center = [view.screen_bounds.x / 2.0, view.screen_bounds.y / 2.0];
            let radius = (settings.aim_assist_fov.clamp(1.0, 30.0) / 30.0)
                * view.screen_bounds.y.min(view.screen_bounds.x)
                * 0.5;
            draw.add_circle(center, radius, [1.0, 0.2, 0.2, 0.65])
                .thickness(1.0)
                .build();
        }
        const UNITS_TO_METERS: f32 = 0.01905;
        const MAX_HEAD_SIZE: f32 = 250.0;

        let view_world_position = match view.get_camera_world_position() {
            Some(view_world_position) => view_world_position,
            _ => return Ok(()),
        };

        for entry in self.players.iter() {
            let PlayerESPInfo {
                pawn_info,
                pawn_model,
            } = entry;

            let distance = (pawn_info.position - view_world_position).norm() * UNITS_TO_METERS;
            let esp_settings = match self.resolve_esp_player_config(&settings, pawn_info) {
                Some(settings) => settings,
                None => continue,
            };
            if esp_settings.near_players {
                if distance > esp_settings.near_players_distance {
                    continue;
                }
            }

            let player_rel_health = (pawn_info.player_health as f32 / 100.0).clamp(0.0, 1.0);

            let entry_model = states.resolve::<CS2Model>(pawn_model.model_address)?;
            let player_2d_box = view.calculate_box_2d(
                &(entry_model.vhull_min + pawn_info.position),
                &(entry_model.vhull_max + pawn_info.position),
            );

            if esp_settings.box_fill {
                if let Some((vmin, vmax)) = &player_2d_box {
                    draw.add_rect(
                        [vmin.x, vmin.y],
                        [vmax.x, vmax.y],
                        esp_settings
                            .box_fill_color
                            .calculate_color(player_rel_health, distance),
                    )
                    .filled(true)
                    .build();
                }
            }

            if esp_settings.skeleton {
                let bones = entry_model.bones.iter().zip(pawn_model.bone_states.iter());

                for (bone, state) in bones {
                    if (bone.flags & BoneFlags::FlagHitbox as u32) == 0 {
                        continue;
                    }

                    let parent_index = if let Some(parent) = bone.parent {
                        parent
                    } else {
                        continue;
                    };

                    let parent_position = match view
                        .world_to_screen(&pawn_model.bone_states[parent_index].position, true)
                    {
                        Some(position) => position,
                        None => continue,
                    };
                    let bone_position = match view.world_to_screen(&state.position, true) {
                        Some(position) => position,
                        None => continue,
                    };

                    if bone.name == "pelvis" {
                        continue;
                    }

                    draw.add_line(
                        parent_position,
                        bone_position,
                        esp_settings
                            .skeleton_color
                            .calculate_color(player_rel_health, distance),
                    )
                    .thickness(esp_settings.skeleton_width)
                    .build();
                }
            }

            if esp_settings.head_dot != EspHeadDot::None {
                if let Some(head_bone_index) = entry_model
                    .bones
                    .iter()
                    .position(|bone| bone.name == "head_0")
                {
                    if let Some(head_state) = pawn_model.bone_states.get(head_bone_index) {
                        if let (Some(head_position), Some(head_far)) = (
                            view.world_to_screen(
                                &(head_state.position
                                    + nalgebra::Vector3::new(0.0, 0.0, esp_settings.head_dot_z)),
                                true,
                            ),
                            view.world_to_screen(
                                &(head_state.position
                                    + nalgebra::Vector3::new(
                                        0.0,
                                        0.0,
                                        esp_settings.head_dot_z + 2.0,
                                    )),
                                true,
                            ),
                        ) {
                            let color = esp_settings
                                .head_dot_color
                                .calculate_color(player_rel_health, distance);

                            let radius =
                                f32::min(f32::abs(head_position.y - head_far.y), MAX_HEAD_SIZE)
                                    * esp_settings.head_dot_base_radius;

                            let circle = draw.add_circle(head_position, radius, color);

                            match esp_settings.head_dot {
                                EspHeadDot::Filled => {
                                    circle.filled(true).build();
                                }
                                EspHeadDot::NotFilled => {
                                    circle
                                        .filled(false)
                                        .thickness(esp_settings.head_dot_thickness)
                                        .build();
                                }
                                EspHeadDot::None => unreachable!(),
                            }
                        }
                    }
                }
            }

            match esp_settings.box_type {
                EspBoxType::Box2D => {
                    if let Some((vmin, vmax)) = &player_2d_box {
                        if esp_settings.box_shadow {
                            let shadow_min = [vmin.x + 2.0, vmin.y + 2.0];
                            let shadow_max = [vmax.x + 2.0, vmax.y + 2.0];
                            if esp_settings.box_corners {
                                Self::draw_box_corners(
                                    &draw,
                                    shadow_min,
                                    shadow_max,
                                    [0.0, 0.0, 0.0, 0.85],
                                    esp_settings.box_width + 2.0,
                                );
                            } else {
                                draw.add_rect(shadow_min, shadow_max, [0.0, 0.0, 0.0, 0.85])
                                    .thickness(esp_settings.box_width + 2.0)
                                    .build();
                            }
                        }
                        let box_color = esp_settings
                            .box_color
                            .calculate_color(player_rel_health, distance);
                        if esp_settings.box_corners {
                            Self::draw_box_corners(
                                &draw,
                                [vmin.x, vmin.y],
                                [vmax.x, vmax.y],
                                box_color,
                                esp_settings.box_width,
                            );
                        } else {
                            draw.add_rect([vmin.x, vmin.y], [vmax.x, vmax.y], box_color)
                                .thickness(esp_settings.box_width)
                                .build();
                        }
                    }
                }
                EspBoxType::Box3D => {
                    if esp_settings.box_shadow {
                        view.draw_box_3d(
                            &draw,
                            &(entry_model.vhull_min + pawn_info.position),
                            &(entry_model.vhull_max + pawn_info.position),
                            [0.0, 0.0, 0.0, 0.85].into(),
                            esp_settings.box_width + 2.0,
                        );
                    }
                    view.draw_box_3d(
                        &draw,
                        &(entry_model.vhull_min + pawn_info.position),
                        &(entry_model.vhull_max + pawn_info.position),
                        esp_settings
                            .box_color
                            .calculate_color(player_rel_health, distance)
                            .into(),
                        esp_settings.box_width,
                    );
                }
                EspBoxType::None => {}
            }

            if let Some((vmin, vmax)) = &player_2d_box {
                let box_bounds = match esp_settings.health_bar {
                    EspHealthBar::None => None,
                    EspHealthBar::Left => {
                        let xoffset =
                            vmin.x - esp_settings.box_width / 2.0 - esp_settings.health_bar_width;

                        Some([
                            xoffset,
                            vmin.y - esp_settings.box_width / 2.0,
                            esp_settings.health_bar_width,
                            vmax.y - vmin.y + esp_settings.box_width,
                        ])
                    }
                    EspHealthBar::Right => {
                        let xoffset = vmax.x + esp_settings.box_width / 2.0;

                        Some([
                            xoffset,
                            vmin.y - esp_settings.box_width / 2.0,
                            esp_settings.health_bar_width,
                            vmax.y - vmin.y + esp_settings.box_width,
                        ])
                    }
                    EspHealthBar::Top => {
                        let yoffset =
                            vmin.y - esp_settings.box_width / 2.0 - esp_settings.health_bar_width;

                        Some([
                            vmin.x,
                            yoffset,
                            vmax.x - vmin.x,
                            esp_settings.health_bar_width,
                        ])
                    }
                    EspHealthBar::Bottom => {
                        let yoffset = vmax.y + esp_settings.box_width / 2.0;

                        Some([
                            vmin.x,
                            yoffset,
                            vmax.x - vmin.x,
                            esp_settings.health_bar_width,
                        ])
                    }
                };

                if let Some([mut box_x, mut box_y, mut box_width, mut box_height]) = box_bounds {
                    const BORDER_WIDTH: f32 = 1.0;
                    draw.add_rect(
                        [box_x + BORDER_WIDTH / 2.0, box_y + BORDER_WIDTH / 2.0],
                        [
                            box_x + box_width - BORDER_WIDTH / 2.0,
                            box_y + box_height - BORDER_WIDTH / 2.0,
                        ],
                        [0.0, 0.0, 0.0, 1.0],
                    )
                    .filled(false)
                    .thickness(BORDER_WIDTH)
                    .build();

                    box_x += BORDER_WIDTH / 2.0 + 1.0;
                    box_y += BORDER_WIDTH / 2.0 + 1.0;

                    box_width -= BORDER_WIDTH + 2.0;
                    box_height -= BORDER_WIDTH + 2.0;

                    let health_color = esp_settings
                        .health_bar_color
                        .calculate_color(player_rel_health, distance);
                    draw.add_rect(
                        [box_x, box_y],
                        [box_x + box_width, box_y + box_height],
                        [0.08, 0.08, 0.08, 0.9],
                    )
                    .filled(true)
                    .build();
                    if box_width < box_height {
                        /* vertical */
                        let yoffset = box_y + (1.0 - player_rel_health) * box_height;
                        draw.add_rect(
                            [box_x, yoffset],
                            [box_x + box_width, box_y + box_height],
                            health_color,
                        )
                        .filled(true)
                        .build();
                    } else {
                        /* horizontal */
                        let xoffset = box_x + player_rel_health * box_width;
                        draw.add_rect(
                            [box_x, box_y],
                            [xoffset, box_y + box_height],
                            health_color,
                        )
                        .filled(true)
                        .build();
                    }
                }
            }

            if let Some((vmin, vmax)) = player_2d_box {
                let mut player_info = PlayerInfoLayout::new(
                    ui,
                    &draw,
                    view.screen_bounds,
                    vmin,
                    vmax,
                );

                if esp_settings.info_name {
                    player_info.add_line(
                        esp_settings.info_name_position,
                        esp_settings
                            .info_name_color
                            .calculate_color(player_rel_health, distance),
                        pawn_info
                            .player_name
                            .as_ref()
                            .map_or("unknown", String::as_str),
                    );

                    if let Some(player_name) = &pawn_info.player_name {
                        unicode_text.register_unicode_text(player_name);
                    }
                }

                if esp_settings.info_weapon {
                    let text = pawn_info.weapon.display_name();
                    player_info.add_line(
                        esp_settings.info_weapon_position,
                        esp_settings
                            .info_weapon_color
                            .calculate_color(player_rel_health, distance),
                        &text,
                    );
                }

                if esp_settings.info_ammo && pawn_info.weapon_current_ammo != -1 {
                    let text = format!(
                        "{}/{}",
                        pawn_info.weapon_current_ammo, pawn_info.weapon_reserve_ammo
                    );
                    player_info.add_line(
                        esp_settings.info_ammo_position,
                        esp_settings
                            .info_ammo_color
                            .calculate_color(player_rel_health, distance),
                        &text,
                    );
                }

                if esp_settings.info_hp_text {
                    let text = format!("{} HP", pawn_info.player_health);
                    player_info.add_line(
                        esp_settings.info_hp_text_position,
                        esp_settings
                            .info_hp_text_color
                            .calculate_color(player_rel_health, distance),
                        &text,
                    );
                }

                let mut player_utilities = Vec::new();
                if esp_settings.info_grenades {
                    if pawn_info.player_has_flash > 0 {
                        player_utilities.push(format!("Flashbang x{}", pawn_info.player_has_flash));
                    }
                    if pawn_info.player_has_smoke {
                        player_utilities.push("Smoke".to_string());
                    }
                    if pawn_info.player_has_hegrenade {
                        player_utilities.push("HE Grenade".to_string());
                    }
                    if pawn_info.player_has_molotov {
                        player_utilities.push("Molotov".to_string());
                    }
                    if pawn_info.player_has_incendiary {
                        player_utilities.push("Incendiary".to_string());
                    }
                    if pawn_info.player_has_decoy {
                        player_utilities.push("Decoy".to_string());
                    }

                    if !player_utilities.is_empty() {
                        player_info.add_line(
                            esp_settings.info_grenades_position,
                            esp_settings
                                .info_grenades_color
                                .calculate_color(player_rel_health, distance),
                            &player_utilities.join(", "),
                        );
                    }
                }

                let mut player_flags = Vec::new();
                if esp_settings.info_flag_kit && pawn_info.player_has_defuser {
                    player_flags.push("Kit");
                }

                if esp_settings.info_flag_bomb && pawn_info.player_has_bomb {
                    player_flags.push("Bomb Carrier");
                }

                if esp_settings.info_flag_scoped && pawn_info.player_is_scoped {
                    player_flags.push("scoped");
                }

                if esp_settings.info_flag_flashed && pawn_info.player_flashtime > 0.0 {
                    player_flags.push("flashed");
                }

                if !player_flags.is_empty() {
                    player_info.add_line(
                        esp_settings.info_flags_position,
                        esp_settings
                            .info_flags_color
                            .calculate_color(player_rel_health, distance),
                        &player_flags.join(", "),
                    );
                }
                if esp_settings.info_distance {
                    let text = format!("{:.0}m", distance);
                    player_info.add_line(
                        esp_settings.info_distance_position,
                        esp_settings
                            .info_distance_color
                            .calculate_color(player_rel_health, distance),
                        &text,
                    );
                }
            }

            // Draw offscreen indicators for players not visible on screen
            if esp_settings.offscreen_arrows {
                // Use head position for more accurate direction, fallback to body position
                let target_position = if let Some(head_bone_index) = entry_model
                    .bones
                    .iter()
                    .position(|bone| bone.name == "head_0")
                {
                    pawn_model
                        .bone_states
                        .get(head_bone_index)
                        .map(|head_state| head_state.position)
                        .unwrap_or(pawn_info.position)
                } else {
                    // If no head bone, use top of the player hull
                    entry_model.vhull_max + pawn_info.position
                };

                if let Some((indicator_pos, angle)) = view.calculate_offscreen_indicator(
                    &target_position,
                    esp_settings.offscreen_arrows_radius_from_center,
                ) {
                    self.draw_offscreen_arrow(
                        &draw,
                        indicator_pos,
                        angle,
                        esp_settings.offscreen_arrows_size,
                        esp_settings
                            .offscreen_arrows_color
                            .calculate_color(player_rel_health, distance),
                    );
                }
            }
        }

        Ok(())
    }
}
