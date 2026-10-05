//! Browser runtime rendering and input contracts.

use sse_ui::event_loop::WindowEvent;
use sse_web::{decode_event, frame_len, WebEvent, WebRuntime};

#[test]
fn first_frame_paints_the_shell_into_a_bounded_argb_buffer() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = WebRuntime::new()?;
    let frame = runtime.frame(320, 240)?.to_vec();

    assert_eq!(frame.len(), 320 * 240);
    assert!(frame.iter().any(|pixel| *pixel != 0));
    assert_eq!(runtime.frame(320, 240)?, frame);
    Ok(())
}

#[test]
fn keyboard_navigation_marks_the_next_shell_frame_dirty() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = WebRuntime::new()?;
    let first = runtime.frame(960, 640)?.to_vec();

    assert!(!runtime.dispatch(WindowEvent::Key {
        pressed: true,
        keysym: 0xff54,
        text: None,
        ctrl: false,
        shift: false,
    }));
    let next = runtime.frame(960, 640)?;

    assert_ne!(first, next);
    Ok(())
}

#[test]
fn frame_length_accepts_4k_and_rejects_unbounded_sizes() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(frame_len(3840, 2160)?, 8_294_400);
    assert_eq!(frame_len(1, 8192)?, 8192);
    assert!(frame_len(3840, 2161).is_err());
    assert!(frame_len(1, 8193).is_err());
    assert!(frame_len(0, 1).is_err());
    Ok(())
}

#[test]
fn browser_event_decoder_rejects_bad_codes_and_preserves_keyboard_modifiers() {
    assert_eq!(decode_event(99, 0, 0, 0, 0, 0), None);
    assert_eq!(decode_event(0, -1, 640, 0, 0, 0), None);
    assert_eq!(decode_event(3, 4, 1, 0, 0, 0), None);
    assert_eq!(decode_event(5, 1, 65, 0xd800, 0, 0), None);
    assert_eq!(
        decode_event(5, 1, 0xff54, 0, 1, 1),
        Some(WebEvent::Key {
            pressed: true,
            keysym: 0xff54,
            text: None,
            ctrl: true,
            shift: true,
        })
    );
}
