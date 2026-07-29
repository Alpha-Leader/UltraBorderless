// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025-2026 Z1xus
// Copyright (C) 2026 Alpha-Leader

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
    Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM},
    Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits,
        GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO, ReleaseDC, SelectObject,
    },
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    UI::WindowsAndMessaging::{
        DrawIconEx, EnumWindows, FindWindowW, GCLP_HICON, GWL_EXSTYLE, GWL_STYLE,
        GetClassLongPtrW, GetWindowLongW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, ICON_SMALL, IsWindow,
        IsWindowVisible, MONITORINFOF_PRIMARY, SET_WINDOW_POS_FLAGS, SMTO_ABORTIFHUNG,
        SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageTimeoutW,
        SetWindowLongW, SetWindowPos, WM_GETICON, WS_BORDER, WS_CAPTION, WS_DLGFRAME,
        WS_EX_TOPMOST, WS_THICKFRAME,
    },
    },
    core::PCWSTR,
};

/// Bits cleared when making a window borderless.
const BORDER_STYLES: u32 = WS_BORDER.0 | WS_CAPTION.0 | WS_THICKFRAME.0 | WS_DLGFRAME.0;

/// How long to wait on a window's message loop when asking it for an icon.
/// Bounded so a hung application cannot stall the whole enumeration.
const ICON_QUERY_TIMEOUT_MS: u32 = 100;

const ICON_SIZE: i32 = 16;

#[derive(Debug, Clone)]
pub struct WindowInfo
{
    pub hwnd: isize,
    pub title: String,
    pub process_name: String,
    pub is_borderless: bool,
    pub icon_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct DisplayInfo
{
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
}

/// A window's frame as it was before we stripped it, so it can be put back exactly.
#[derive(Debug, Clone, Copy)]
struct OriginalFrame
{
    style: u32,
    rect: RECT,
    was_topmost: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementMode
{
    /// Strip the borders but leave the window where it is.
    LeaveInPlace,
    /// Fill the whole monitor.
    FullDisplay,
    /// Fill a sub-rectangle of the monitor.
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor
{
    Centered,
    Left,
    Right,
    Custom,
}

/// Where a stripped window should be put. Deliberately free of Win32 calls so the
/// geometry can be unit-tested on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement
{
    pub mode: PlacementMode,
    pub width: i32,
    pub height: i32,
    pub anchor: Anchor,
    pub custom_x: i32,
    pub custom_y: i32,
    pub always_on_top: bool,
}

impl Default for Placement
{
    fn default() -> Self
    {
        Self {
            // Defaults to a centered 4K region: the common case is fitting a game
            // into part of an ultrawide, not stretching it across the whole panel.
            mode: PlacementMode::Region,
            width: 3840,
            height: 2160,
            anchor: Anchor::Centered,
            custom_x: 0,
            custom_y: 0,
            always_on_top: true,
        }
    }
}

impl Placement
{
    /// The target rectangle in desktop coordinates, or `None` to leave the window
    /// where it is. Offsets are relative to the display, so the result stays
    /// correct if the monitor is moved or its mode changes.
    pub fn resolve(&self, display: &DisplayInfo) -> Option<RECT>
    {
        let (x, y, width, height) = match self.mode {
            PlacementMode::LeaveInPlace => return None,
            PlacementMode::FullDisplay => {
                (display.x, display.y, display.width, display.height)
            }
            PlacementMode::Region => {
                let width = self.width.max(1);
                let height = self.height.max(1);

                let x = match self.anchor {
                    Anchor::Centered => display.x + (display.width - width) / 2,
                    Anchor::Left => display.x,
                    Anchor::Right => display.x + display.width - width,
                    Anchor::Custom => display.x + self.custom_x,
                };

                let y = match self.anchor {
                    Anchor::Custom => display.y + self.custom_y,
                    _ => display.y + (display.height - height) / 2,
                };

                (x, y, width, height)
            }
        };

        Some(RECT { left: x, top: y, right: x + width, bottom: y + height })
    }

    /// True when the resolved rectangle sticks out past the display, which is
    /// allowed but worth surfacing in the UI.
    pub fn overflows(&self, display: &DisplayInfo) -> bool
    {
        match self.resolve(display) {
            Some(rect) => {
                rect.left < display.x
                    || rect.top < display.y
                    || rect.right > display.x + display.width
                    || rect.bottom > display.y + display.height
            }
            None => false,
        }
    }
}

/// Payload threaded through `EnumWindows`. The process map is built once per
/// refresh rather than re-snapshotted for every window.
struct EnumContext
{
    windows: Vec<WindowInfo>,
    processes: HashMap<u32, String>,
}

/// Clears the in-progress flag however the refresh thread exits.
struct RefreshGuard(Arc<AtomicBool>);

impl Drop for RefreshGuard
{
    fn drop(&mut self)
    {
        self.0.store(false, Ordering::Release);
    }
}

impl WindowInfo
{
    pub fn display_text(&self) -> String
    {
        let max_title_len = 30;
        let max_process_len = 15;

        let truncated_title = if self.title.chars().count() > max_title_len {
            let truncated: String = self.title.chars().take(max_title_len - 3).collect();
            format!("{}...", truncated)
        } else {
            self.title.clone()
        };

        let truncated_process = if self.process_name.chars().count() > max_process_len {
            let truncated: String = self.process_name.chars().take(max_process_len - 3).collect();
            format!("{}...", truncated)
        } else {
            self.process_name.clone()
        };

        format!("{} ({})", truncated_title, truncated_process)
    }
}

impl DisplayInfo
{
    pub fn display_text(&self) -> String
    {
        let primary_indicator = if self.is_primary { " (Primary)" } else { "" };
        format!("{} - {}x{}{}", self.name, self.width, self.height, primary_indicator)
    }
}

pub struct WindowManager
{
    windows: Vec<WindowInfo>,
    refresh_in_progress: Arc<AtomicBool>,
    original_frames: HashMap<isize, OriginalFrame>,
}

impl WindowManager
{
    pub fn new() -> Self
    {
        Self {
            windows: Vec::new(),
            refresh_in_progress: Arc::new(AtomicBool::new(false)),
            original_frames: HashMap::new(),
        }
    }

    /// Starts a background enumeration. Returns `None` if one is already running,
    /// so the caller never holds a receiver that will not produce a value.
    pub fn refresh_windows_async(&self) -> Option<std::sync::mpsc::Receiver<Vec<WindowInfo>>>
    {
        let refresh_flag = Arc::clone(&self.refresh_in_progress);

        if refresh_flag
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }

        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let _guard = RefreshGuard(refresh_flag);

            let mut context =
                EnumContext { windows: Vec::new(), processes: build_process_map() };

            unsafe {
                if EnumWindows(
                    Some(enum_windows_proc),
                    LPARAM(&mut context as *mut EnumContext as isize),
                )
                .is_ok()
                {
                    context.windows.sort_by(|a: &WindowInfo, b: &WindowInfo| a.title.cmp(&b.title));
                }
            }

            let _ = sender.send(context.windows);
        });

        Some(receiver)
    }

    pub fn get_windows(&self) -> &[WindowInfo]
    {
        &self.windows
    }

    pub fn set_windows(&mut self, windows: Vec<WindowInfo>)
    {
        self.windows = windows;

        // Drop saved frames for windows that no longer exist, so the map cannot
        // grow without bound over a long session.
        self.original_frames
            .retain(|hwnd, _| unsafe { IsWindow(Some(as_hwnd(*hwnd))).as_bool() });
    }

    /// Whether this app stripped the given window and can put it back exactly.
    pub fn has_saved_frame(&self, hwnd: isize) -> bool
    {
        self.original_frames.contains_key(&hwnd)
    }

    pub fn get_displays(&self) -> Vec<DisplayInfo>
    {
        let mut displays = Vec::new();

        unsafe {
            let _ = EnumDisplayMonitors(
                Some(HDC::default()),
                None,
                Some(enum_monitors_proc),
                LPARAM(&mut displays as *mut Vec<DisplayInfo> as isize),
            );
        }

        displays.sort_by(|a: &DisplayInfo, b: &DisplayInfo| {
            if a.is_primary && !b.is_primary {
                std::cmp::Ordering::Less
            } else if !a.is_primary && b.is_primary {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        displays
    }

    /// Strips a window's borders and places it per `placement`, or restores the
    /// exact frame we saved when stripping it. Never invents a frame for a window
    /// it did not modify.
    pub fn toggle_borderless(
        &mut self,
        hwnd: isize,
        placement: &Placement,
        selected_display: Option<&DisplayInfo>,
    ) -> anyhow::Result<()>
    {
        let handle = as_hwnd(hwnd);

        unsafe {
            // Handles are recycled by Windows, so a stale entry from the last
            // snapshot could otherwise land on an unrelated window.
            if !IsWindow(Some(handle)).as_bool() {
                anyhow::bail!("window no longer exists");
            }

            let current_style = GetWindowLongW(handle, GWL_STYLE) as u32;
            if current_style == 0 {
                anyhow::bail!("could not read window style");
            }

            if let Some(original) = self.original_frames.remove(&hwnd) {
                SetWindowLongW(handle, GWL_STYLE, original.style as i32);

                // Put the window back in its original band. SWP_NOZORDER must be
                // absent for the insert-after handle to take effect.
                let restore_z =
                    if original.was_topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };

                SetWindowPos(
                    handle,
                    Some(restore_z),
                    original.rect.left,
                    original.rect.top,
                    original.rect.right - original.rect.left,
                    original.rect.bottom - original.rect.top,
                    SWP_FRAMECHANGED,
                )?;

                return Ok(());
            }

            if (current_style & BORDER_STYLES) == 0 {
                anyhow::bail!("window has no borders to remove");
            }

            let mut rect = RECT::default();
            GetWindowRect(handle, &mut rect)?;

            let was_topmost =
                (GetWindowLongW(handle, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;

            self.original_frames
                .insert(hwnd, OriginalFrame { style: current_style, rect, was_topmost });

            SetWindowLongW(handle, GWL_STYLE, (current_style & !BORDER_STYLES) as i32);

            // A window narrower than the monitor never triggers the shell's
            // fullscreen detection, so covering the taskbar requires topmost.
            let (insert_after, z_flag) = if placement.always_on_top {
                (HWND_TOPMOST, SET_WINDOW_POS_FLAGS(0))
            } else {
                (HWND_TOP, SWP_NOZORDER)
            };

            let target = selected_display.and_then(|display| placement.resolve(display));

            match target {
                Some(rect) => SetWindowPos(
                    handle,
                    Some(insert_after),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_FRAMECHANGED | z_flag,
                )?,
                // No display to resolve against, or LeaveInPlace: restyle only.
                None => SetWindowPos(
                    handle,
                    Some(insert_after),
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | z_flag,
                )?,
            }
        }

        Ok(())
    }
}

fn as_hwnd(hwnd: isize) -> HWND
{
    HWND(hwnd as *mut std::ffi::c_void)
}

/// Switches this app's own title bar to the dark variant.
///
/// The title bar is drawn by Windows, not by egui, so it follows the *system*
/// theme regardless of the app's own styling. Without this a machine set to
/// light mode gets a white title bar above a dark window.
pub fn use_dark_titlebar_for_own_window() -> bool
{
    let mut title: Vec<u16> = env!("CARGO_PKG_NAME").encode_utf16().collect();
    title.push(0);

    unsafe {
        let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) else {
            return false;
        };

        if hwnd.is_invalid() {
            return false;
        }

        // The attribute takes a Win32 BOOL, which is a 32-bit int.
        let enabled: i32 = 1;
        let size = std::mem::size_of::<i32>() as u32;

        // Attribute 20 on Windows 10 2004+ and Windows 11; 19 on the older
        // Windows 10 builds that supported it first.
        let applied = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const i32 as *const std::ffi::c_void,
            size,
        )
        .is_ok();

        if applied {
            return true;
        }

        DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(19),
            &enabled as *const i32 as *const std::ffi::c_void,
            size,
        )
        .is_ok()
    }
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL
{
    unsafe {
        let context = &mut *(lparam.0 as *mut EnumContext);

        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }

        let mut title_buffer = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, &mut title_buffer);
        if title_len <= 0 {
            return true.into();
        }

        let title = String::from_utf16_lossy(&title_buffer[..title_len as usize]);

        // Derived from Cargo.toml so a rename cannot leave the app listing itself.
        if title.trim().is_empty()
            || title.starts_with("Program Manager")
            || title == env!("CARGO_PKG_NAME")
        {
            return true.into();
        }

        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));

        let process_name = context
            .processes
            .get(&process_id)
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string());

        if process_name.eq_ignore_ascii_case(env!("CARGO_PKG_NAME")) {
            return true.into();
        }

        let current_style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let is_borderless = (current_style & BORDER_STYLES) == 0;

        let icon_data = extract_window_icon(hwnd);

        context.windows.push(WindowInfo {
            hwnd: hwnd.0 as isize,
            title,
            process_name,
            is_borderless,
            icon_data,
        });

        true.into()
    }
}

/// Snapshots every running process once, keyed by pid.
fn build_process_map() -> HashMap<u32, String>
{
    let mut processes = HashMap::new();

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return processes;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let raw = String::from_utf16_lossy(&entry.szExeFile);
                let name = raw.trim_end_matches('\0');
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                processes.insert(entry.th32ProcessID, stem.to_string());

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    processes
}

unsafe extern "system" fn enum_monitors_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> windows::core::BOOL
{
    unsafe {
        let displays = &mut *(lparam.0 as *mut Vec<DisplayInfo>);

        let mut monitor_info =
            MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };

        if GetMonitorInfoW(hmonitor, &mut monitor_info).as_bool() {
            let width = monitor_info.rcMonitor.right - monitor_info.rcMonitor.left;
            let height = monitor_info.rcMonitor.bottom - monitor_info.rcMonitor.top;
            let is_primary = (monitor_info.dwFlags & MONITORINFOF_PRIMARY) != 0;

            let name = format!("Display {}", displays.len() + 1);

            displays.push(DisplayInfo {
                name,
                x: monitor_info.rcMonitor.left,
                y: monitor_info.rcMonitor.top,
                width,
                height,
                is_primary,
            });
        }

        true.into()
    }
}

struct GdiResources
{
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
}

impl GdiResources
{
    fn new(size: i32) -> Option<Self>
    {
        unsafe {
            let hdc_screen = GetDC(Some(HWND::default()));
            if hdc_screen.is_invalid() {
                return None;
            }

            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            if hdc_mem.is_invalid() {
                ReleaseDC(Some(HWND::default()), hdc_screen);
                return None;
            }

            let hbitmap = CreateCompatibleBitmap(hdc_screen, size, size);
            if hbitmap.is_invalid() {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(Some(HWND::default()), hdc_screen);
                return None;
            }

            let old_bitmap = SelectObject(hdc_mem, hbitmap.into());

            Some(Self { hdc_screen, hdc_mem, hbitmap, old_bitmap })
        }
    }

    fn draw_icon(
        &self,
        icon_handle: windows::Win32::UI::WindowsAndMessaging::HICON,
        size: i32,
    ) -> windows::core::Result<()>
    {
        unsafe {
            DrawIconEx(
                self.hdc_mem,
                0,
                0,
                icon_handle,
                size,
                size,
                0,
                Some(windows::Win32::Graphics::Gdi::HBRUSH::default()),
                windows::Win32::UI::WindowsAndMessaging::DI_NORMAL,
            )
        }
    }

    fn get_bitmap_data(&self, size: i32) -> Option<Vec<u8>>
    {
        unsafe {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default(); 1],
            };

            let mut rgba_data = vec![0u8; (size * size * 4) as usize];
            let result = GetDIBits(
                self.hdc_mem,
                self.hbitmap,
                0,
                size as u32,
                Some(rgba_data.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            if result == 0 {
                return None;
            }

            for chunk in rgba_data.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }

            Some(rgba_data)
        }
    }
}

impl Drop for GdiResources
{
    fn drop(&mut self)
    {
        unsafe {
            SelectObject(self.hdc_mem, self.old_bitmap);
            let _ = DeleteObject(self.hbitmap.into());
            let _ = DeleteDC(self.hdc_mem);
            ReleaseDC(Some(HWND::default()), self.hdc_screen);
        }
    }
}

fn extract_window_icon(hwnd: HWND) -> Option<Vec<u8>>
{
    unsafe {
        // WM_GETICON is handled by the target window's message loop, so this must
        // be time-bounded: a plain SendMessageW to a hung app never returns and
        // would strand the refresh thread permanently.
        let mut icon_result = 0usize;
        let sent = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            ICON_QUERY_TIMEOUT_MS,
            Some(&mut icon_result),
        );

        let icon_handle = if sent.0 != 0 && icon_result != 0 {
            windows::Win32::UI::WindowsAndMessaging::HICON(icon_result as *mut std::ffi::c_void)
        } else {
            let class_icon = GetClassLongPtrW(hwnd, GCLP_HICON);
            if class_icon != 0 {
                windows::Win32::UI::WindowsAndMessaging::HICON(class_icon as *mut std::ffi::c_void)
            } else {
                return None;
            }
        };

        let gdi_resources = GdiResources::new(ICON_SIZE)?;

        if gdi_resources.draw_icon(icon_handle, ICON_SIZE).is_err() {
            return None;
        }

        gdi_resources.get_bitmap_data(ICON_SIZE)
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
        core::w,
    };

    /// The 7680x2160 ultrawide this feature was built for.
    fn ultrawide() -> DisplayInfo
    {
        DisplayInfo {
            name: "Display 1".to_string(),
            x: 0,
            y: 0,
            width: 7680,
            height: 2160,
            is_primary: true,
        }
    }

    fn region(width: i32, height: i32, anchor: Anchor) -> Placement
    {
        Placement { mode: PlacementMode::Region, width, height, anchor, ..Placement::default() }
    }

    /// The headline case: a 4K game centered on the ultrawide, leaving 1920px
    /// of desktop free on each side.
    #[test]
    fn centers_a_4k_region_on_the_ultrawide()
    {
        let rect = region(3840, 2160, Anchor::Centered).resolve(&ultrawide()).unwrap();

        assert_eq!((rect.left, rect.top), (1920, 0));
        assert_eq!((rect.right - rect.left, rect.bottom - rect.top), (3840, 2160));
        assert_eq!(rect.right, 5760);
    }

    #[test]
    fn anchors_pin_to_the_correct_edge()
    {
        let display = ultrawide();

        assert_eq!(region(3840, 2160, Anchor::Left).resolve(&display).unwrap().left, 0);
        assert_eq!(region(3840, 2160, Anchor::Right).resolve(&display).unwrap().left, 3840);

        let custom = Placement {
            custom_x: 100,
            custom_y: 50,
            ..region(1920, 1080, Anchor::Custom)
        };
        let rect = custom.resolve(&display).unwrap();
        assert_eq!((rect.left, rect.top), (100, 50));
    }

    /// A shorter region centers vertically rather than pinning to the top.
    #[test]
    fn a_shorter_region_is_centered_vertically()
    {
        let rect = region(3840, 1080, Anchor::Centered).resolve(&ultrawide()).unwrap();
        assert_eq!(rect.top, 540);
        assert_eq!(rect.bottom, 1620);
    }

    #[test]
    fn full_display_and_leave_in_place_are_unchanged()
    {
        let display = ultrawide();

        let full = Placement { mode: PlacementMode::FullDisplay, ..Placement::default() };
        let rect = full.resolve(&display).unwrap();
        assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (0, 0, 7680, 2160));
        assert!(!full.overflows(&display));

        let leave = Placement { mode: PlacementMode::LeaveInPlace, ..Placement::default() };
        assert!(leave.resolve(&display).is_none());
    }

    #[test]
    fn oversized_regions_are_reported_as_overflowing()
    {
        let display = ultrawide();

        assert!(!region(3840, 2160, Anchor::Centered).overflows(&display));
        assert!(region(3840, 4320, Anchor::Centered).overflows(&display));
        assert!(region(8000, 2160, Anchor::Centered).overflows(&display));
    }

    /// Offsets are display-relative, so a monitor that is not at the desktop
    /// origin still gets the region placed inside it.
    #[test]
    fn regions_follow_a_display_that_is_not_at_the_origin()
    {
        let secondary = DisplayInfo { x: -1920, y: 200, width: 1920, height: 1080, ..ultrawide() };

        let rect = region(1280, 720, Anchor::Centered).resolve(&secondary).unwrap();
        assert_eq!((rect.left, rect.top), (-1600, 380));
    }

    /// The bug this guards: restoring used to OR in `WS_CAPTION | WS_THICKFRAME`
    /// rather than replaying the window's real style, so frames came back wrong.
    #[test]
    fn restores_the_original_style_exactly()
    {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("ultraborderless-test"),
                // Visible, like every window the app actually enumerates: a
                // never-shown window does not pick up WS_EX_TOPMOST.
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                400,
                300,
                None,
                None,
                None,
                None,
            )
            .expect("create test window");

            let handle = hwnd.0 as isize;
            let original = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let original_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            assert_ne!(original & BORDER_STYLES, 0, "fixture should start with borders");
            assert_eq!(original_ex & WS_EX_TOPMOST.0, 0, "fixture should not start topmost");

            let mut manager = WindowManager::new();
            let placement = Placement {
                mode: PlacementMode::LeaveInPlace,
                always_on_top: true,
                ..Placement::default()
            };

            manager.toggle_borderless(handle, &placement, None).expect("strip borders");
            assert_eq!(
                GetWindowLongW(hwnd, GWL_STYLE) as u32 & BORDER_STYLES,
                0,
                "border bits should be cleared"
            );
            assert_ne!(
                GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                0,
                "always_on_top should have made the window topmost"
            );
            assert!(manager.has_saved_frame(handle));

            manager.toggle_borderless(handle, &placement, None).expect("restore borders");
            assert_eq!(
                GetWindowLongW(hwnd, GWL_STYLE) as u32,
                original,
                "style must round-trip exactly"
            );
            assert_eq!(
                GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                0,
                "topmost must be dropped again on restore"
            );
            assert!(!manager.has_saved_frame(handle));

            let _ = DestroyWindow(hwnd);
        }
    }

    /// Handles are recycled, so acting on a dead one could hit an unrelated window.
    #[test]
    fn rejects_a_handle_that_is_not_a_window()
    {
        let mut manager = WindowManager::new();
        let placement = Placement::default();
        assert!(manager.toggle_borderless(0, &placement, None).is_err());
        assert!(manager.toggle_borderless(0xDEAD_BEEF, &placement, None).is_err());
    }
}
