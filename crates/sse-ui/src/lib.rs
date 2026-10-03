//! Our own interface toolkit (U1): canvas, text, layout, widgets, windows. Owner: Claude.

/// Text field editing model (X27).
pub mod edit;
/// Virtual list and table model (X28).
pub mod list;
/// Software raster primitives over a premultiplied BGRA surface (X14).
pub mod raster;
/// Line breaking, carets, ellipsis and search folding (X13).
pub mod text;
/// X11 core protocol, MIT-SHM, XKB and clipboard encoding over a transport trait (X11).
pub mod x11;
