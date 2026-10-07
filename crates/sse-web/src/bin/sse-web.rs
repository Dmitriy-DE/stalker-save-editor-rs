//! Registers browser callbacks before JavaScript calls the stable system ABI exports.

fn main() {
    sse_web::register_browser_callbacks();
}
