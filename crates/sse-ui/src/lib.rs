//! Our own interface toolkit (U1): canvas, text, layout, widgets, windows. Owner: Claude.

/// Text field editing model (X27).
pub mod edit;
/// Event loop: window events and worker messages on one channel, idle sleep, damage-only repaint (U1).
pub mod event_loop;
/// Bundled fonts, glyph cache and text drawing (U1).
pub mod glyphs;
/// Measure-and-arrange layout over an arena (X26).
pub mod layout;
/// Virtual list and table model (X28).
pub mod list;
/// Numbers, sizes, dates and plural forms in the 15 interface languages (X23).
pub mod locale;
/// SVG path data, flattening, strokes and the 28 interface icons (X25).
pub mod path;
/// Software raster primitives over a premultiplied BGRA surface (X14).
pub mod raster;
/// Line breaking, carets, ellipsis and search folding (X13).
pub mod text;
/// Generated Unicode 17 grapheme and line-break property tables.
pub mod unicode_tables;
pub mod wayland;
/// Retained widget tree with damage tracking (U1).
pub mod widget;
/// X11 core protocol, MIT-SHM, XKB and clipboard encoding over a transport trait (X11).
pub mod x11;
/// X11 window backend over a Unix socket (U1).
#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11_window;
