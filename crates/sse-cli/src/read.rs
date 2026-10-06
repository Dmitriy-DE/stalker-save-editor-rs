use std::path::Path;

use sse_core::{Error, SaveBuffer};
use sse_xray::Save;

use crate::S2_LEGACY_WARNING;

pub(super) fn read_info(path: Option<&String>) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    if let Ok(save) = sse_s2::S2Save::from_bytes(packed.as_slice()) {
        print_lines(s2_info_lines(&save, packed.as_slice()));
        return Ok(());
    }
    let save = read_xray_or_unsupported(packed.as_slice())?;
    println!("Integrity: X-Ray LZO/container OK");
    println!("Format: {}", save.format().id());
    println!("Packed: {}", packed.len());
    println!("Raw: {}", save.raw_size());
    println!("SHA256: {}", sse_codecs::sha256::sha256_hex(packed.as_slice()));
    println!("Money: {}", save.money()?);
    println!("Inventory objects: {}", save.inventory()?.len());
    Ok(())
}

pub(super) fn read_inventory(path: Option<&String>) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    if let Ok(save) = sse_s2::S2Save::from_bytes(packed.as_slice()) {
        print_lines(s2_inventory_lines(&save));
        return Ok(());
    }
    let save = read_xray_or_unsupported(packed.as_slice())?;
    println!("Format: {}", save.format().id());
    println!("POS        TYPE                 KEY                       COUNT   HANDLE");
    for item in save.inventory()? {
        let position = item.placement.as_deref().unwrap_or("inventory");
        let count = item
            .count
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string());
        println!(
            "{position:<10} {:<20} {:<25} {count:>7}  0x{:04X}",
            item.category, item.section, item.handle
        );
    }
    Ok(())
}

fn read_xray_or_unsupported(packed: &[u8]) -> sse_core::Result<Save> {
    Save::read(packed).map_err(|error| {
        if packed.starts_with(&u32::MAX.to_le_bytes()) {
            error
        } else {
            Error::damaged("Unknown or unsupported save format.")
        }
    })
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        println!("{line}");
    }
}

pub(super) fn s2_info_lines(save: &sse_s2::S2Save, packed: &[u8]) -> Vec<String> {
    let index = save.index();
    let items = save.items();
    let orphans = save.orphans();
    let mut lines = vec![
        "CRC: OK".to_owned(),
        "Format: stalker2".to_owned(),
        format!("Packed: {}", save.container().packed_size()),
        format!("Raw: {}", save.container().image().len()),
        format!("SHA256: {}", sse_codecs::sha256::sha256_hex(packed)),
        format!("Money: {}", save.money()),
        format!("Owned handles: {}", index.owned_handles().len()),
        format!("Grid handles parsed/total: {}", index.grid_handle_count()),
        format!("Grid cells: {}", index.grid_cells().len()),
        format!("Inventory objects: {}", items.len()),
        format!("Orphans: {}", orphans.len()),
    ];
    let unmatched_grid_cells = items.is_empty() && index.grid_handle_count() > 0;
    lines.extend(s2_cli_warnings(
        index.is_legacy(),
        save.warnings(),
        unmatched_grid_cells,
    ));
    lines
}

pub(super) fn s2_cli_warnings(is_legacy: bool, warnings: &[String], unmatched_grid_cells: bool) -> Vec<String> {
    // C# prints every reader warning and puts the 1.0.x layout warning, in Russian, last.
    let mut lines: Vec<String> = warnings
        .iter()
        .filter(|warning| !warning.contains("1.0.x"))
        .map(|warning| format!("Warning: {warning}"))
        .collect();
    if unmatched_grid_cells {
        let insert_at = lines
            .iter()
            .position(|line| line.starts_with("Warning: Owned handle "))
            .unwrap_or(lines.len());
        lines.insert(
            insert_at,
            "Warning: Не удалось сопоставить ни одной grid cell с object record".to_owned(),
        );
    }
    if is_legacy || warnings.iter().any(|warning| warning.contains("1.0.x")) {
        lines.push(format!("Warning: {S2_LEGACY_WARNING}"));
    }
    lines
}

pub(super) fn s2_inventory_lines(save: &sse_s2::S2Save) -> Vec<String> {
    let mut lines = vec![
        "Format: stalker2".to_owned(),
        "POS        TYPE                 KEY                       COUNT   HANDLE".to_owned(),
    ];
    lines.extend(save.items().into_iter().map(|item| {
        let position = if item.x.is_some() {
            "?"
        } else if matches!(item.kind_code, 6 | 8 | 10 | 11) {
            "у персонажа"
        } else {
            "экипировано"
        };
        let key = s2_type_key(item.type_key);
        let category = s2_category_name(item.kind_code, item.display_name.as_deref());
        format!(
            "{position:<10} {category:<20} {key:<25} {:>7}  0x{:08X}",
            item.count, item.handle
        )
    }));
    lines
}

pub(super) fn s2_type_key(key: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", key[0], key[1], key[2])
}

fn s2_category_name(kind: u8, display_name: Option<&str>) -> String {
    let normalized = display_name.unwrap_or_default().trim().to_lowercase();
    if normalized.starts_with("nvg_")
        || normalized.starts_with("binocular")
        || normalized.starts_with("пнв")
        || normalized.starts_with("бинокль")
        || normalized.starts_with("бинокл")
    {
        return "Устройство".to_owned();
    }
    if normalized.contains("_upgrade_") || normalized.contains("_attachment_") {
        return "Модуль/улучшение".to_owned();
    }
    if normalized.ends_with("_armor") || normalized.ends_with("_helmet") {
        return "Броня/экипировка".to_owned();
    }
    if normalized.contains("_armor_") || normalized.contains("_helmet_") || normalized.starts_with("gunbucket_") {
        return "Разное".to_owned();
    }

    match kind {
        0 => "Оружие".to_owned(),
        1 => "Броня/экипировка".to_owned(),
        2 => "Артефакт".to_owned(),
        4 => "Расходник".to_owned(),
        5 => "Патроны".to_owned(),
        6 => "Детектор".to_owned(),
        7 => "Гранаты".to_owned(),
        8 => "Разное".to_owned(),
        10 => "ПНВ".to_owned(),
        11 => "Бинокль".to_owned(),
        _ => format!("Тип {kind}"),
    }
}
