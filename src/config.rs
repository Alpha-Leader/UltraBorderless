// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Alpha-Leader

use crate::window_manager::{Anchor, Placement, PlacementMode};
use std::path::PathBuf;

/// Settings that survive a restart, stored as `key=value` lines in
/// `%APPDATA%\<package name>\config.txt`.
///
/// Deliberately hand-rolled: the whole format is a handful of integers and
/// enums, which is not worth a serialization dependency. A missing or malformed
/// file is never an error — anything unparseable falls back to the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config
{
    pub placement: Placement,
    pub display_index: usize,
}

impl Config
{
    pub fn load() -> Self
    {
        let Some(path) = config_path() else {
            return Self::default();
        };

        let Ok(contents) = std::fs::read_to_string(&path) else {
            return Self::default();
        };

        let mut config = Self::default();

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            let key = key.trim();
            let value = value.trim();

            match key {
                "mode" => {
                    config.placement.mode = match value {
                        "leave_in_place" => PlacementMode::LeaveInPlace,
                        "region" => PlacementMode::Region,
                        _ => PlacementMode::FullDisplay,
                    }
                }
                "anchor" => {
                    config.placement.anchor = match value {
                        "left" => Anchor::Left,
                        "right" => Anchor::Right,
                        "custom" => Anchor::Custom,
                        _ => Anchor::Centered,
                    }
                }
                "width" => set_dimension(&mut config.placement.width, value),
                "height" => set_dimension(&mut config.placement.height, value),
                "custom_x" => set_offset(&mut config.placement.custom_x, value),
                "custom_y" => set_offset(&mut config.placement.custom_y, value),
                "always_on_top" => config.placement.always_on_top = value == "true",
                "display_index" => {
                    if let Ok(parsed) = value.parse::<usize>() {
                        config.display_index = parsed;
                    }
                }
                _ => {}
            }
        }

        config
    }

    /// Best effort: a failed write must never disrupt the app, so errors are
    /// dropped rather than surfaced.
    pub fn save(&self)
    {
        let Some(path) = config_path() else {
            return;
        };

        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }

        let mode = match self.placement.mode {
            PlacementMode::LeaveInPlace => "leave_in_place",
            PlacementMode::FullDisplay => "full_display",
            PlacementMode::Region => "region",
        };

        let anchor = match self.placement.anchor {
            Anchor::Centered => "centered",
            Anchor::Left => "left",
            Anchor::Right => "right",
            Anchor::Custom => "custom",
        };

        let contents = format!(
            "# {} settings\n\
             mode={}\n\
             width={}\n\
             height={}\n\
             anchor={}\n\
             custom_x={}\n\
             custom_y={}\n\
             always_on_top={}\n\
             display_index={}\n",
            env!("CARGO_PKG_NAME"),
            mode,
            self.placement.width,
            self.placement.height,
            anchor,
            self.placement.custom_x,
            self.placement.custom_y,
            self.placement.always_on_top,
            self.display_index,
        );

        let _ = std::fs::write(&path, contents);
    }
}

fn set_dimension(target: &mut i32, value: &str)
{
    if let Ok(parsed) = value.parse::<i32>()
        && parsed > 0
    {
        *target = parsed;
    }
}

fn set_offset(target: &mut i32, value: &str)
{
    if let Ok(parsed) = value.parse::<i32>() {
        *target = parsed;
    }
}

fn config_path() -> Option<PathBuf>
{
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join(env!("CARGO_PKG_NAME")).join("config.txt"))
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn unknown_and_malformed_entries_fall_back_to_defaults()
    {
        // Exercised indirectly: the parser must never panic on junk.
        let mut config = Config::default();
        set_dimension(&mut config.placement.width, "not-a-number");
        set_dimension(&mut config.placement.height, "-5");
        assert_eq!(config.placement.width, Placement::default().width);
        assert_eq!(config.placement.height, Placement::default().height);

        set_dimension(&mut config.placement.width, "2560");
        assert_eq!(config.placement.width, 2560);

        // Negative offsets are legitimate (a region left of the display origin).
        set_offset(&mut config.placement.custom_x, "-100");
        assert_eq!(config.placement.custom_x, -100);
    }
}
