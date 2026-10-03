//! Our own interface toolkit (U1): canvas, text, layout, widgets, windows. Owner: Claude.

/// Text field editing model (X27).
pub mod edit;
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
pub mod wayland;
/// X11 core protocol, MIT-SHM, XKB and clipboard encoding over a transport trait (X11).
pub mod x11;
