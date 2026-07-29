// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025-2026 Z1xus
// Copyright (C) 2026 Alpha-Leader
//
// Modified from ihateborders <https://github.com/Z1xus/ihateborders>.
// See NOTICE for the list of changes.

#![windows_subsystem = "windows"]

mod app;
mod config;
mod ui;
mod window_manager;

use app::{BorderlessApp, create_app_options};

fn main() -> Result<(), eframe::Error>
{
    eframe::run_native(
        env!("CARGO_PKG_NAME"),
        create_app_options(),
        Box::new(|cc| Ok(Box::new(BorderlessApp::new(cc)))),
    )
}
