//! Our own interface toolkit (U1): canvas, text, layout, widgets, windows. Owner: Claude.

/// Text field editing model (X27).
pub mod edit;
/// Event loop: window events and worker messages on one channel, idle sleep, damage-only repaint (U1).
pub mod event_loop;
/// Bundled fonts, glyph cache and text drawing (U1).
pub mod glyphs;
/// Pixel-tolerant screenshot golden comparison (X37).
pub mod golden;
/// Measure-and-arrange layout over an arena (X26).
pub mod layout;
/// Virtual list and table model (X28).
pub mod list;
/// Numbers, sizes, dates and plural forms in the 15 interface languages (X23).
pub mod locale;
/// macOS window adapter over the safe `sse-sys` window API.
#[cfg(any(target_os = "macos", test))]
pub mod macos_window;
/// SVG path data, flattening, strokes and the 28 interface icons (X25).
pub mod path;
/// Software raster primitives over a premultiplied BGRA surface (X14).
pub mod raster;
/// Editor screens, the shell and the screen registry (S1–S5).
pub mod screens;
/// C# screen acceptance inventory (X36).
pub mod screens_spec;
/// Line breaking, carets, ellipsis and search folding (X13).
pub mod text;
/// Visual theme tokens and interface scaling (X36).
pub mod theme;
/// Generated Unicode 17 grapheme and line-break property tables.
pub mod unicode_tables;
pub mod wayland;
/// Retained widget tree with damage tracking (U1).
pub mod widget;
/// Win32 window adapter over the safe `sse-sys` window API.
#[cfg(any(windows, test))]
pub mod win32_window;
/// X11 core protocol, MIT-SHM, XKB and clipboard encoding over a transport trait (X11).
pub mod x11;
/// X11 window backend over a Unix socket (U1).
#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11_window;

/// Reusable retained widgets.
pub mod widgets;
