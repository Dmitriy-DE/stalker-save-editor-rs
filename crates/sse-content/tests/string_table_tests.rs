//! String table reader tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_content::string_tables::XRayStringTables;

struct TestFile {
    path: String,
    data: Vec<u8>,
}

#[test]
fn string_tables_use_only_the_preferred_language_folder() {
    let files = vec![
        TestFile {
            path: "configs/text/eng/items.xml".to_string(),
            data: b"<string_table><string id=\"st_medkit\"><text>Medkit</text></string></string_table>".to_vec(),
        },
        TestFile {
            path: "configs/text/rus/items.xml".to_string(),
            // Windows-1251 encoded "Аптечка": 0xC0, 0xEF, 0xF2, 0xE5, 0xF7, 0xEA, 0xE0
            data: [
                b"<?xml version=\"1.0\" encoding=\"windows-1251\"?><string_table><string id=\"st_medkit\"><text>"
                    .as_slice(),
                &[0xC0, 0xEF, 0xF2, 0xE5, 0xF7, 0xEA, 0xE0],
                b"</text></string></string_table>",
            ]
            .concat(),
        },
    ];

    let ru = XRayStringTables::read(&files, |f| &f.path, |f| Some(f.data.clone()), "ru");
    assert_eq!(ru.get("st_medkit").map(String::as_str), Some("Аптечка"));

    let en = XRayStringTables::read(&files, |f| &f.path, |f| Some(f.data.clone()), "en");
    assert_eq!(en.get("st_medkit").map(String::as_str), Some("Medkit"));
}

#[test]
fn string_tables_decode_entities_and_cdata() {
    let files = vec![TestFile {
        path: "gamedata/configs/text/rus/test.xml".to_string(),
        data: b"<string_table><string id=\"quote\"><text>&quot;Stalker&quot; &amp; &apos;Zone&apos;</text></string></string_table>".to_vec(),
    }];

    let res = XRayStringTables::read(&files, |f| &f.path, |f| Some(f.data.clone()), "ru");
    assert_eq!(res.get("quote").map(String::as_str), Some("\"Stalker\" & 'Zone'"));
}
