use std::{
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

use enigo::{Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use super::Enhancement;
use crate::{
    settings::AppSettings,
    UpdateContext,
};

const KEY_HOLD_TIME: Duration = Duration::from_millis(350);

pub struct AntiAfk {
    stop_signal: Option<Arc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
    interval: u32,
    move_pixels: u32,
    use_keyboard: bool,
}

impl AntiAfk {
    pub fn new() -> Self {
        Self {
            stop_signal: None,
            worker: None,
            interval: 25,
            move_pixels: 8,
            use_keyboard: true,
        }
    }

    fn start(&mut self, interval: u32, move_pixels: u32, use_keyboard: bool) {
        if self.worker.is_some() {
            return;
        }

        let stop_signal = Arc::new(AtomicBool::new(false));
        let worker_stop_signal = stop_signal.clone();
        let worker = thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(enigo) => enigo,
                Err(error) => {
                    log::warn!("Anti AFK could not initialize mouse input: {}", error);
                    return;
                }
            };

            while !worker_stop_signal.load(Ordering::Relaxed) {
                for _ in 0..interval.max(5) * 2 {
                    if worker_stop_signal.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }

                if use_keyboard {
                    let _ = enigo.key(Key::Unicode('a'), Direction::Press);
                    thread::sleep(KEY_HOLD_TIME);
                    let _ = enigo.key(Key::Unicode('a'), Direction::Release);
                    thread::sleep(Duration::from_millis(100));
                    let _ = enigo.key(Key::Unicode('d'), Direction::Press);
                    thread::sleep(KEY_HOLD_TIME);
                    let _ = enigo.key(Key::Unicode('d'), Direction::Release);
                } else {
                    let pixels = move_pixels.clamp(1, 50) as i32;
                    let _ = enigo.move_mouse(pixels, 0, Coordinate::Rel);
                    let _ = enigo.move_mouse(-pixels, 0, Coordinate::Rel);
                }
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

impl Drop for AntiAfk {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Enhancement for AntiAfk {
    fn update(&mut self, ctx: &UpdateContext) -> anyhow::Result<()> {
        let settings = ctx.states.resolve::<AppSettings>(())?;
        let enabled = settings.anti_afk;
        let interval = settings.anti_afk_interval.clamp(5, 120);
        let move_pixels = settings.anti_afk_move_pixels.clamp(1, 50);
        let use_keyboard = settings.anti_afk_use_keyboard;
        if enabled {
            if self.worker.is_none()
                || self.interval != interval
                || self.move_pixels != move_pixels
                || self.use_keyboard != use_keyboard
            {
                self.stop();
                self.interval = interval;
                self.move_pixels = move_pixels;
                self.use_keyboard = use_keyboard;
                self.start(interval, move_pixels, use_keyboard);
            }
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