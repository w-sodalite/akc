use std::env::{set_var, var};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{sleep, Builder};
use std::time::Duration;

use eframe::egui::{
    vec2, CentralPanel, Color32, Context, DragValue, FontData, FontDefinitions, FontFamily,
    RichText, Ui,
};
use eframe::epaint::ahash::HashMap;
use eframe::{run_native, App, Frame, NativeOptions};
use livesplit_hotkey::{Hook, Hotkey, KeyCode};
use log::{debug, info, warn};
use rdev::EventType;

const APP_NAME: &str = "AKC";

const WINDOW_TITLE: &str = "AKC (自动按键)";

const MIN_DELAY_MILLIS: u64 = 100;

const R1: [char; 10] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];

const R2: [char; 10] = ['Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P'];

const R3: [char; 9] = ['A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L'];

const R4: [char; 7] = ['Z', 'X', 'C', 'V', 'B', 'N', 'M'];

const PLUS: char = '+';

const START_KEY_LABEL_TEXT: &str = "启动按键";

const STOP_KEY_LABEL_TEXT: &str = "关闭按键";

const DELAY_MILLIS_LABEL_TEXT: &str = "延迟时间";

const DELAY_MILLIS_SUFFIX_TEXT: &str = "ms";

const RUN_STATUS_LABEL_TEXT: &str = "运行状态";

const RUN_STATUS_ACTIVE_TEXT: &str = "运行中";

const RUN_STATUS_NO_ACTIVE_TEXT: &str = "未运行";

const AUTO_PRESS_LABEL_TEXT: &str = "自动按键";

const HORIZONTAL_SPACE_AMOUNT: f32 = 10.0;

const VERTICAL_SPACE_AMOUNT: f32 = 10.0;

const LISTEN_THREAD_NAME: &str = "listen-thread";

macro_rules! add_selectable_values {
    ($ui:expr,$hk:expr,$($ident:ident),+) => {
        $(
            $ui.selectable_value(&mut $hk.key, KeyCode::$ident, stringify!($ident));
        )+
    };
}

macro_rules! add_key_ui {
    ($ui:expr, $label:expr,$hk:expr) => {
        $ui.horizontal(|ui| {
            ui.label($label);
            eframe::egui::ComboBox::new(eframe::egui::Id::new($label), "")
                .selected_text($hk.key.name())
                .show_ui(ui, |ui| {
                    add_selectable_values!(
                        ui, $hk, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12
                    );
                });
        });
    };
}

macro_rules! add_keyboard_ui {
    ($ui:expr,$se:expr,$($rs:expr),+) => {
        $(
            $ui.add_space(HORIZONTAL_SPACE_AMOUNT);
            $ui.horizontal(|ui| {
                for x in $rs {
                    let mut button = eframe::egui::Button::new(x.to_string()).min_size(vec2(25.0, 25.0));
                    if $se.keys.get(&x).copied().unwrap_or(false) {
                        button = button.fill(eframe::egui::Color32::GREEN);
                    };
                    if ui.add(button).clicked() {
                        let pressed = $se.keys.entry(x).or_insert(false);
                        *pressed = !*pressed;
                        $se.register_start(Some($se.start));
                    }
                }
            });
        )+
    };
}

macro_rules! rdev_key {
    ($ident:ident) => {
        Some(rdev::Key::$ident)
    };
}

fn main() {
    if var("RUST_LOG").is_err() {
        set_var("RUST_LOG", "debug");
    }
    env_logger::init();

    let options = NativeOptions {
        resizable: false,
        initial_window_size: Some(vec2(450.0, 300.0)),
        ..NativeOptions::default()
    };
    run_native(
        APP_NAME,
        options,
        Box::new(|ctx| {
            let mut fonts = FontDefinitions::default();
            fonts.font_data.insert(
                "FZY4JW".to_string(),
                FontData::from_static(include_bytes!("../fonts/FZY4JW.TTF")),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .push("FZY4JW".to_string());
            ctx.egui_ctx.set_fonts(fonts);
            Box::<Akc>::default()
        }),
    );
}

struct Akc {
    start: ListenKey,
    stop: ListenKey,
    delay: u64,
    keys: HashMap<char, bool>,
    active: Arc<AtomicBool>,
    hook: Hook,
}

impl Default for Akc {
    fn default() -> Self {
        let akc = Akc {
            start: ListenKey::new(KeyCode::F1),
            stop: ListenKey::new(KeyCode::F2),
            delay: MIN_DELAY_MILLIS,
            hook: Hook::new().unwrap(),
            active: Arc::new(AtomicBool::new(false)),
            keys: HashMap::default(),
        };
        akc.register_start(None);
        akc.register_stop(None);
        akc
    }
}

impl Akc {
    fn register<F: FnMut() + Send + 'static>(&self, listen_key: ListenKey, callback: F) {
        let hot_key = Hotkey::try_from(listen_key).unwrap();
        self.hook.register(hot_key, callback).unwrap_or(())
    }

    pub fn unregister(&self, listen_key: ListenKey) {
        let hot_key = Hotkey::try_from(listen_key).unwrap();
        self.hook.unregister(hot_key).unwrap_or(())
    }

    pub fn register_start(&self, back: Option<ListenKey>) {
        if let Some(back) = back {
            self.unregister(back);
        }
        let keys = Arc::new(
            self.keys
                .iter()
                .filter(|(_, p)| **p)
                .map(|(k, _)| k)
                .copied()
                .collect::<Vec<_>>(),
        );
        if !keys.is_empty() {
            let active = self.active.clone();
            let delay = self.delay;
            debug!(
                "Register start callback: hot_key={:?} delay={delay}ms keys={:?}",
                self.start.key, keys
            );
            self.register(self.start, move || {
                match active.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => {
                        let active = active.clone();
                        let keys = keys.clone();
                        Builder::new()
                            .name(LISTEN_THREAD_NAME.to_string())
                            .spawn(move || {
                                while active.load(Ordering::SeqCst) {
                                    keys.iter().copied().for_each(|key| {
                                        if let Some(key) = get_rdev_key(key) {
                                            rdev::simulate(&EventType::KeyPress(key)).unwrap();
                                            rdev::simulate(&EventType::KeyRelease(key)).unwrap();
                                        }
                                    });
                                    sleep(Duration::from_millis(delay));
                                }
                            })
                            .unwrap();
                        info!("Start keyboard simulate thread success!")
                    }
                    Err(_) => warn!("Keyboard simulate thread already start!"),
                }
            })
        }
    }

    pub fn register_stop(&self, back: Option<ListenKey>) {
        if let Some(back) = back {
            self.unregister(back);
        }
        let active = self.active.clone();
        debug!("Register stop callback: hot_key={:?}", self.stop.key);
        self.register(self.stop, move || {
            match active.compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => info!("Stop keyboard simulate thread success!"),
                Err(_) => warn!("Keyboard simulate thread already stop!"),
            }
        });
    }

    fn add_start_key(&mut self, ui: &mut Ui) {
        let current = self.start;
        ui.add_space(VERTICAL_SPACE_AMOUNT);
        add_key_ui!(ui, START_KEY_LABEL_TEXT, &mut self.start);
        if current != self.start {
            self.register_start(Some(current));
        }
    }

    fn add_stop_key(&mut self, ui: &mut Ui) {
        let current = self.stop;
        ui.add_space(VERTICAL_SPACE_AMOUNT);
        add_key_ui!(ui, STOP_KEY_LABEL_TEXT, &mut self.stop);
        if current != self.stop {
            self.register_stop(Some(current));
        }
    }

    fn add_delay_millis(&mut self, ui: &mut Ui) {
        ui.add_space(VERTICAL_SPACE_AMOUNT);
        ui.horizontal(|ui| {
            ui.label(DELAY_MILLIS_LABEL_TEXT);
            if ui.add(DragValue::new(&mut self.delay)).changed() && self.delay < MIN_DELAY_MILLIS {
                self.delay = MIN_DELAY_MILLIS
            }
            ui.label(DELAY_MILLIS_SUFFIX_TEXT);
        });
    }

    fn add_run_status(&mut self, ui: &mut Ui) {
        ui.add_space(VERTICAL_SPACE_AMOUNT);
        ui.horizontal(|ui| {
            ui.label(RUN_STATUS_LABEL_TEXT);
            if self.active.load(Ordering::SeqCst) {
                ui.label(RichText::new(RUN_STATUS_ACTIVE_TEXT).color(Color32::GREEN));
            } else {
                ui.label(RUN_STATUS_NO_ACTIVE_TEXT);
            }
        });
    }

    pub fn add_virtually_keyboard(&mut self, ui: &mut Ui) {
        ui.add_space(HORIZONTAL_SPACE_AMOUNT);
        ui.horizontal(|ui| {
            ui.label(AUTO_PRESS_LABEL_TEXT);
            ui.vertical(|ui| {
                add_keyboard_ui!(ui, self, R1, R2, R3, R4);
            });
        });
    }
}

fn get_rdev_key(k: char) -> Option<rdev::Key> {
    match k {
        '0' => rdev_key!(Num0),
        '1' => rdev_key!(Num1),
        '2' => rdev_key!(Num2),
        '3' => rdev_key!(Num3),
        '4' => rdev_key!(Num4),
        '5' => rdev_key!(Num5),
        '6' => rdev_key!(Num6),
        '7' => rdev_key!(Num7),
        '8' => rdev_key!(Num8),
        '9' => rdev_key!(Num9),
        'Q' => rdev_key!(KeyQ),
        'W' => rdev_key!(KeyW),
        'E' => rdev_key!(KeyE),
        'R' => rdev_key!(KeyR),
        'T' => rdev_key!(KeyT),
        'Y' => rdev_key!(KeyY),
        'U' => rdev_key!(KeyU),
        'I' => rdev_key!(KeyI),
        'O' => rdev_key!(KeyO),
        'P' => rdev_key!(KeyP),
        'A' => rdev_key!(KeyA),
        'S' => rdev_key!(KeyS),
        'D' => rdev_key!(KeyD),
        'F' => rdev_key!(KeyF),
        'G' => rdev_key!(KeyG),
        'H' => rdev_key!(KeyH),
        'J' => rdev_key!(KeyJ),
        'K' => rdev_key!(KeyK),
        'L' => rdev_key!(KeyL),
        'Z' => rdev_key!(KeyZ),
        'X' => rdev_key!(KeyX),
        'C' => rdev_key!(KeyC),
        'V' => rdev_key!(KeyV),
        'B' => rdev_key!(KeyB),
        'N' => rdev_key!(KeyN),
        'M' => rdev_key!(KeyM),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
struct ListenKey {
    ctrl: bool,
    alt: bool,
    key: KeyCode,
}

impl ListenKey {
    pub fn new(key: KeyCode) -> Self {
        Self {
            ctrl: false,
            alt: false,
            key,
        }
    }
}

impl TryFrom<ListenKey> for Hotkey {
    type Error = ();

    fn try_from(listen_key: ListenKey) -> Result<Self, Self::Error> {
        let mut value = String::new();
        if listen_key.ctrl {
            value.push_str(KeyCode::ControlLeft.name());
            value.push(PLUS);
        }
        if listen_key.alt {
            value.push_str(KeyCode::AltLeft.name());
            value.push(PLUS);
        }
        value.push_str(listen_key.key.name());
        Hotkey::from_str(&value)
    }
}

impl App for Akc {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        //设置窗口名称
        frame.set_window_title(WINDOW_TITLE);
        //创建主Panel
        let panel = CentralPanel::default();
        panel.show(ctx, |ui| {
            //运行中时,禁用界面操作
            ui.set_enabled(!self.active.load(Ordering::SeqCst));
            //启动按键
            self.add_start_key(ui);
            //关闭按键
            self.add_stop_key(ui);
            //延迟时间
            self.add_delay_millis(ui);
            //运行状态
            self.add_run_status(ui);
            //虚拟键盘
            self.add_virtually_keyboard(ui);
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.active.fetch_and(false, Ordering::SeqCst);
    }
}
