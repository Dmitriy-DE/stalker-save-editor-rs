//! The browser editor end to end, driven the way the page drives it: a save is opened from memory, the item
//! pictures arrive later as a separate file, and a money edit produces a copy for download.
//!
//! One test in its own process: the item icon store is shared by every runtime in a process.

use std::ops::Range;
use std::path::{Path, PathBuf};

use sse_ui::event_loop::WindowEvent;
use sse_ui::item_icons;
use sse_web::WebRuntime;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// The «Инвентарь» tab of an opened save in a 1920×1080 frame (wide enough for the inventory row to show its picture).
const INVENTORY_TAB: (i32, i32) = (438, 168);
/// The «+10 000» money button of the inventory.
const ADD_10_000: (i32, i32) = (859, 264);
/// The «Сохранить» button of the header.
const SAVE: (i32, i32) = (1852, 28);
/// The icon frame of the first inventory row.
const FIRST_ROW_ICON: (Range<usize>, Range<usize>) = (588..628, 414..454);

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn click(runtime: &mut WebRuntime, (x, y): (i32, i32)) {
    runtime.dispatch(WindowEvent::PointerMoved { x, y });
    runtime.dispatch(WindowEvent::Button {
        button: 1,
        pressed: true,
        x,
        y,
    });
    runtime.dispatch(WindowEvent::Button {
        button: 1,
        pressed: false,
        x,
        y,
    });
}

/// Pixels of a frame region, row by row.
fn region(frame: &[u32], (columns, rows): (Range<usize>, Range<usize>)) -> Vec<u32> {
    let stride = usize::try_from(WIDTH).unwrap_or(0);
    frame
        .chunks(stride.max(1))
        .enumerate()
        .filter(|(row, _)| rows.contains(row))
        .flat_map(|(_, pixels)| pixels.get(columns.clone()).unwrap_or_default().to_vec())
        .collect()
}

/// The pack the build script writes, built here from the same pictures.
fn pack_from_assets() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let assets = repo_root().join("crates/sse-ui/assets/item-icons");
    let mut pictures = Vec::new();
    for folder in ["s2", "xray"] {
        for entry in std::fs::read_dir(assets.join(folder))? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "png") {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("icon name is not text")?;
                pictures.push((format!("{folder}/{name}"), std::fs::read(&path)?));
            }
        }
    }
    let entries: Vec<(&str, &[u8])> = pictures
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    Ok(item_icons::encode_pack(&entries)?)
}

#[test]
fn opened_save_gets_item_pictures_later_and_a_money_edit_downloads_a_copy() -> TestResult {
    let source = std::fs::read(repo_root().join("fixtures/synthetic/xray-clear-sky.sav"))?;
    let money_before = sse_xray::save::Save::read(&source)?.money()?;
    let mut runtime = WebRuntime::new()?;
    runtime.frame(WIDTH, HEIGHT)?;

    runtime.open_browser_file("xray-clear-sky.sav", source.clone(), 1_700_000_000_000)?;
    click(&mut runtime, INVENTORY_TAB);
    let without_pictures = runtime.frame(WIDTH, HEIGHT)?.to_vec();

    // A damaged pack is refused and changes nothing on screen: the rows keep their empty frames.
    assert!(runtime.install_item_icons(b"not an icon pack".to_vec()).is_err());
    assert_eq!(runtime.frame(WIDTH, HEIGHT)?, without_pictures);

    let pack = pack_from_assets()?;
    // The file made by `tools/build-web.sh` must be this very pack. `SSE_REQUIRE_WEB_ICON_PACK` (set where the
    // web build ran first) turns its absence into a failure.
    let built = repo_root().join("web/item-icons.bin");
    if built.exists() {
        assert!(
            std::fs::read(&built)? == pack,
            "web/item-icons.bin differs from the pictures in the repository"
        );
    } else {
        assert!(
            std::env::var_os("SSE_REQUIRE_WEB_ICON_PACK").is_none(),
            "web/item-icons.bin was not built"
        );
    }
    assert!(runtime.install_item_icons(pack)? > 600);
    let with_pictures = runtime.frame(WIDTH, HEIGHT)?.to_vec();
    if !item_icons::pack_embedded() {
        // Before the pack arrived the first row had an empty frame; now it shows the picture of the item.
        assert_ne!(
            region(&with_pictures, FIRST_ROW_ICON),
            region(&without_pictures, FIRST_ROW_ICON)
        );
    }

    assert!(runtime.take_download().is_none());
    click(&mut runtime, ADD_10_000);
    click(&mut runtime, SAVE);
    let download = runtime
        .take_download()
        .ok_or("a money edit did not produce a download")?;
    assert!(download.file_name.ends_with(".sav"));
    assert_ne!(download.bytes.as_slice(), source.as_slice());
    assert_eq!(
        sse_xray::save::Save::read(download.bytes.as_slice())?.money()?,
        money_before.saturating_add(10_000)
    );
    assert!(runtime.take_download().is_none());
    Ok(())
}
