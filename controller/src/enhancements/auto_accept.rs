use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use screenshots::Screen;

use super::Enhancement;
use crate::{
    settings::AppSettings,
    UpdateContext,
};

const TARGET_COLOR: [i32; 3] = [60, 170, 80];
const COLOR_TOLERANCE: i32 = 55;

pub struct AutoAccept {
    stop_signal: Option<Arc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
}

impl AutoAccept {
    pub fn new() -> Self {
        Self {
            stop_signal: None,
            worker: None,
        }
    }

    fn start(&mut self) {
        if self.worker.is_some() {
            return;
        }

        let stop_signal = Arc::new(AtomicBool::new(false));
        let worker_stop_signal = stop_signal.clone();
        let worker = thread::spawn(move || {
            let screens = match Screen::all() {
                Ok(screens) if !screens.is_empty() => screens,
                Ok(_) => return,
                Err(error) => {
                    log::warn!("Auto Accept could not list screens: {}", error);
                    return;
                }
            };
            let screen = screens[0];
            let display = screen.display_info;
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(enigo) => enigo,
                Err(error) => {
                    log::warn!("Auto Accept could not initialize mouse input: {}", error);
                    return;
                }
            };

            while !worker_stop_signal.load(Ordering::Relaxed) {
                if let Ok(image) = screen.capture() {
                    let target_x = image.width() / 2;
                    let target_y = (image.height() as f32 * 0.418) as u32;
                    let mut green_pixels = 0;

                    for offset_y in 0..9 {
                        for offset_x in 0..9 {
                            let x = target_x.saturating_sub(4) + offset_x;
                            let y = target_y.saturating_sub(4) + offset_y;
                            let pixel = image.get_pixel(x, y);
                            let matches_target = (0..3).all(|index| {
                                (pixel[index] as i32 - TARGET_COLOR[index]).abs() <= COLOR_TOLERANCE
                            });
                            if matches_target {
                                green_pixels += 1;
                            }
                        }
                    }

                    if green_pixels >= 12 {
                        let click_x = display.x + target_x as i32;
                        let click_y = display.y + target_y as i32;
                        let _ = enigo.move_mouse(click_x, click_y, Coordinate::Abs);
                        let _ = enigo.button(Button::Left, Direction::Click);
                        log::info!("Auto Accept clicked the detected button");
                        thread::sleep(Duration::from_secs(10));
                        continue;
                    }
                }

                thread::sleep(Duration::from_millis(500));
            }
        });

        self.stop_signal = Some(stop_signal);
        self.worker = Some(worker);
    }

    fn stop(&mut self) {
        if let Some(stop_signal) = self.stop_signal.take() {
            stop_signal.store(true, Ordering::Relaxed);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AutoAccept {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Enhancement for AutoAccept {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let enabled = ctx.states.resolve::<AppSettings>(())?.auto_accept;
        if enabled {
            self.start();
        } else {
            self.stop();
        }
        Ok(())
    }

    fn render(
        &self,
        _states: &utils_state::StateRegistry,
        _ui: &imgui::Ui,
        _unicode_text: &overlay::UnicodeTextRenderer,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}