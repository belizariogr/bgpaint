//! Scripted input for automated UI testing (enabled with the `automation` cargo feature).
//!
//! `BGPAINT_SCRIPT=<file>` runs one command per line (coordinates in points, window relative):
//! `move X Y`, `down X Y [left|right|middle]`, `up X Y [button]`, `click X Y [button]`,
//! `drag X1 Y1 X2 Y2 [steps] [button]`, `key [ctrl+][shift+][alt+]NAME`, `text STRING`,
//! `wait FRAMES`, `shot NAME` (PNG in `BGPAINT_SHOT_DIR`), `quit`. Lines starting with `#` are ignored.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use std::collections::VecDeque;

pub struct Automation {
    steps: VecDeque<String>,
    queued: VecDeque<Vec<Event>>,
    wait: u32,
    shot_dir: std::path::PathBuf,
    pub quit: bool,
}

impl Automation {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("BGPAINT_SCRIPT")?;
        let text = std::fs::read_to_string(path).ok()?;
        let steps = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect();
        let shot_dir = std::env::var_os("BGPAINT_SHOT_DIR").map(Into::into).unwrap_or_else(std::env::temp_dir);
        Some(Self { steps, queued: VecDeque::new(), wait: 10, shot_dir, quit: false })
    }

    fn button(s: Option<&str>) -> PointerButton {
        match s {
            Some("right") => PointerButton::Secondary,
            Some("middle") => PointerButton::Middle,
            _ => PointerButton::Primary,
        }
    }

    fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    pub fn hook(&mut self, ctx: &egui::Context, raw: &mut RawInput) {
        ctx.request_repaint();
        if let Some(events) = self.queued.pop_front() {
            raw.events.extend(events);
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(line) = self.steps.pop_front() else { return };
        eprintln!("automation: {line}");
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| parts.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match parts.first().copied() {
            Some("move") => raw.events.push(Event::PointerMoved(Pos2::new(num(1), num(2)))),
            Some("down") => {
                let p = Pos2::new(num(1), num(2));
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Self::press(p, Self::button(parts.get(3).copied()), true));
            }
            Some("up") => {
                let p = Pos2::new(num(1), num(2));
                raw.events.push(Self::press(p, Self::button(parts.get(3).copied()), false));
            }
            Some("click") => {
                let p = Pos2::new(num(1), num(2));
                let b = Self::button(parts.get(3).copied());
                raw.events.push(Event::PointerMoved(p));
                self.queued.push_back(vec![Self::press(p, b, true)]);
                self.queued.push_back(vec![Self::press(p, b, false)]);
                self.wait = 2;
            }
            Some("drag") => {
                let (a, b) = (Pos2::new(num(1), num(2)), Pos2::new(num(3), num(4)));
                let steps = parts.get(5).and_then(|v| v.parse::<usize>().ok()).unwrap_or(20).max(1);
                let button = Self::button(parts.get(6).copied());
                raw.events.push(Event::PointerMoved(a));
                self.queued.push_back(vec![Self::press(a, button, true)]);
                for i in 1..=steps {
                    self.queued.push_back(vec![Event::PointerMoved(a.lerp(b, i as f32 / steps as f32))]);
                }
                self.queued.push_back(vec![Self::press(b, button, false)]);
                self.wait = 2;
            }
            Some("key") => {
                let spec = parts.get(1).copied().unwrap_or("");
                let mut modifiers = Modifiers::NONE;
                let mut name = spec;
                for part in spec.split('+') {
                    match part {
                        "ctrl" => modifiers = modifiers | Modifiers::COMMAND,
                        "shift" => modifiers = modifiers | Modifiers::SHIFT,
                        "alt" => modifiers = modifiers | Modifiers::ALT,
                        other => name = other,
                    }
                }
                if let Some(key) = Key::from_name(name) {
                    raw.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
                    self.queued.push_back(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
                }
            }
            Some("text") => raw.events.push(Event::Text(line.trim_start_matches("text").trim_start().to_owned())),
            Some("wait") => self.wait = num(1) as u32,
            Some("shot") => {
                let path = self.shot_dir.join(format!("{}.png", parts.get(1).copied().unwrap_or("shot")));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    save_png(&img, &path);
                })));
                self.wait = 6;
            }
            Some("quit") => self.quit = true,
            _ => eprintln!("automation: unknown step `{line}`"),
        }
    }
}

fn save_png(img: &egui::ColorImage, path: &std::path::Path) {
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    let Some(mut p) = bgpaint_core::tiny_skia::Pixmap::new(w, h) else { return };
    for (d, c) in p.data_mut().chunks_exact_mut(4).zip(&img.pixels) {
        d.copy_from_slice(&c.to_array());
    }
    if let Err(e) = p.save_png(path) {
        eprintln!("automation: could not save {}: {e}", path.display());
    } else {
        eprintln!("automation: saved {}", path.display());
    }
}
