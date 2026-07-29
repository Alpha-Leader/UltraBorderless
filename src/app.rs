// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025-2026 Z1xus
// Copyright (C) 2026 Alpha-Leader

use crate::{
    config::Config,
    ui::{self, IconCacheInterface},
    window_manager::{DisplayInfo, Placement, WindowInfo, WindowManager},
};
use eframe::egui;
use std::{
    collections::HashMap,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// How long an action's success/failure message stays on screen.
const STATUS_TTL: Duration = Duration::from_secs(4);

struct IconCache
{
    cache: HashMap<String, (egui::TextureHandle, Instant)>,
    max_size: usize,
    ttl: Duration,
}

impl IconCache
{
    fn new() -> Self
    {
        Self { cache: HashMap::new(), max_size: 100, ttl: Duration::from_secs(300) }
    }

    fn cleanup_expired(&mut self)
    {
        let now = Instant::now();
        self.cache.retain(|_, (_, last_used)| now.duration_since(*last_used) < self.ttl);
    }

    fn remove_oldest(&mut self)
    {
        if let Some(oldest_key) = self
            .cache
            .iter()
            .min_by_key(|(_, (_, last_used))| *last_used)
            .map(|(key, _)| key.clone())
        {
            self.cache.remove(&oldest_key);
        }
    }
}

impl IconCacheInterface for IconCache
{
    fn get(&mut self, key: &str) -> Option<&egui::TextureHandle>
    {
        let now = Instant::now();

        let fresh = match self.cache.get(key) {
            Some((_, last_used)) => now.duration_since(*last_used) < self.ttl,
            None => return None,
        };

        if !fresh {
            self.cache.remove(key);
            return None;
        }

        let (texture, last_used) = self.cache.get_mut(key)?;
        *last_used = now;
        Some(texture)
    }

    fn insert(&mut self, key: String, texture: egui::TextureHandle)
    {
        self.cleanup_expired();

        if self.cache.len() >= self.max_size {
            self.remove_oldest();
        }

        self.cache.insert(key, (texture, Instant::now()));
    }

    fn contains_key(&self, key: &str) -> bool
    {
        if let Some((_, last_used)) = self.cache.get(key) {
            last_used.elapsed() < self.ttl
        } else {
            false
        }
    }
}

pub struct BorderlessApp
{
    window_manager: WindowManager,
    /// The selected window is tracked by handle, never by list index: the list is
    /// rebuilt and re-sorted every few seconds, so an index can silently come to
    /// mean a different application.
    selected_hwnd: Option<isize>,
    last_refresh: Instant,
    icon_cache: IconCache,
    placement: Placement,
    selected_display: Option<usize>,
    displays: Vec<DisplayInfo>,
    needs_repaint: bool,
    refresh_receiver: Option<Receiver<Vec<WindowInfo>>>,
    status: Option<(String, bool, Instant)>,
    dark_titlebar_applied: bool,
}

impl BorderlessApp
{
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self
    {
        ui::setup_dark_theme(&cc.egui_ctx);

        let window_manager = WindowManager::new();
        let displays = window_manager.get_displays();
        let config = Config::load();

        let selected_display = if displays.is_empty() {
            None
        } else {
            // A saved index can outlive the monitor it referred to.
            Some(config.display_index.min(displays.len() - 1))
        };

        let mut app = Self {
            window_manager,
            selected_hwnd: None,
            last_refresh: Instant::now(),
            icon_cache: IconCache::new(),
            placement: config.placement,
            selected_display,
            displays,
            needs_repaint: false,
            refresh_receiver: None,
            status: None,
            dark_titlebar_applied: false,
        };

        app.start_async_refresh();

        app
    }

    fn start_async_refresh(&mut self)
    {
        if self.refresh_receiver.is_none() {
            self.refresh_receiver = self.window_manager.refresh_windows_async();
        }
    }

    fn handle_refresh(&mut self)
    {
        if let Some(receiver) = &self.refresh_receiver {
            match receiver.try_recv() {
                Ok(windows) => {
                    self.window_manager.set_windows(windows);
                    self.last_refresh = Instant::now();
                    self.needs_repaint = true;
                    self.refresh_receiver = None;

                    if let Some(hwnd) = self.selected_hwnd {
                        let still_present =
                            self.window_manager.get_windows().iter().any(|w| w.hwnd == hwnd);
                        if !still_present {
                            self.selected_hwnd = None;
                        }
                    }
                }
                // The scan thread ended without a result. Clear the receiver so the
                // timer can start a new scan instead of waiting on it forever.
                Err(TryRecvError::Disconnected) => {
                    self.last_refresh = Instant::now();
                    self.refresh_receiver = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        if self.refresh_receiver.is_none() && self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.start_async_refresh();
        }
    }

    fn handle_keyboard_input(&mut self, ctx: &egui::Context)
    {
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.refresh_receiver = None;
            self.start_async_refresh();
            self.needs_repaint = true;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected_hwnd = None;
            self.needs_repaint = true;
        }
    }

    fn handle_window_action(&mut self, hwnd: isize)
    {
        let title = self
            .window_manager
            .get_windows()
            .iter()
            .find(|w| w.hwnd == hwnd)
            .map(|w| w.title.clone())
            .unwrap_or_else(|| "selected window".to_string());

        let display = self.selected_display.and_then(|index| self.displays.get(index)).cloned();

        let restoring = self.window_manager.has_saved_frame(hwnd);

        match self.window_manager.toggle_borderless(hwnd, &self.placement, display.as_ref()) {
            Ok(()) => {
                let action = if restoring { "Restored borders on" } else { "Made borderless:" };
                self.set_status(format!("{} {}", action, title), false);

                self.save_config();

                self.refresh_receiver = None;
                self.start_async_refresh();
                self.needs_repaint = true;
            }
            Err(error) => {
                self.set_status(format!("{}: {}", title, error), true);
                self.needs_repaint = true;
            }
        }
    }

    fn set_status(&mut self, message: String, is_error: bool)
    {
        self.status = Some((message, is_error, Instant::now()));
    }

    fn save_config(&self)
    {
        Config { placement: self.placement, display_index: self.selected_display.unwrap_or(0) }
            .save();
    }

    fn current_status(&mut self) -> Option<(&str, bool)>
    {
        let expired = match &self.status {
            Some((_, _, shown_at)) => shown_at.elapsed() >= STATUS_TTL,
            None => return None,
        };

        if expired {
            self.status = None;
            return None;
        }

        self.status.as_ref().map(|(message, is_error, _)| (message.as_str(), *is_error))
    }
}

impl eframe::App for BorderlessApp
{
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)
    {
        // Deferred to the first frame: the native window is guaranteed to exist
        // by now, which it is not during BorderlessApp::new.
        if !self.dark_titlebar_applied {
            self.dark_titlebar_applied = crate::window_manager::use_dark_titlebar_for_own_window();
        }

        self.handle_refresh();
        self.handle_keyboard_input(ctx);

        self.icon_cache.cleanup_expired();

        let selected_hwnd = self.selected_hwnd;
        let can_restore = selected_hwnd.is_some_and(|hwnd| self.window_manager.has_saved_frame(hwnd));

        egui::CentralPanel::default().show(ctx, |ui| {
            let windows = self.window_manager.get_windows();

            ui::render_header(ui, windows.len());

            ui::render_window_selector(ui, windows, &mut self.selected_hwnd, &mut self.icon_cache);

            if self.displays.len() > 1 {
                ui::render_display_selector(ui, &self.displays, &mut self.selected_display);
            }

            let display = self.selected_display.and_then(|index| self.displays.get(index));
            ui::render_placement_controls(ui, &mut self.placement, display);

            let action =
                ui::render_action_button(ui, windows, self.selected_hwnd, can_restore);

            if let Some((message, is_error)) = self.current_status() {
                ui::render_status(ui, message, is_error);
            }

            ui::render_footer(ui);

            if let Some(hwnd) = action {
                self.handle_window_action(hwnd);
            }
        });

        if self.needs_repaint {
            self.needs_repaint = false;
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(REFRESH_INTERVAL);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>)
    {
        self.save_config();
    }
}

pub fn create_app_options() -> eframe::NativeOptions
{
    let icon_data = load_icon();

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(env!("CARGO_PKG_NAME"))
            .with_inner_size([380.0, 450.0])
            .with_min_inner_size([380.0, 450.0])
            .with_max_inner_size([380.0, 450.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icon_data),
        ..Default::default()
    }
}

fn load_icon() -> egui::IconData
{
    let icon_bytes = include_bytes!("../assets/icon.ico");

    let image = image::load_from_memory(icon_bytes).expect("Failed to load icon").into_rgba8();

    let (width, height) = image.dimensions();
    let rgba = image.into_raw();

    egui::IconData { rgba, width, height }
}
