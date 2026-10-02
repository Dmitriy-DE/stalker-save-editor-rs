//! Text and string table character encodings.
//!
//! Handles UTF-8 (with optional BOM) and fallback to Windows-1251 / Windows-1250
//! as used by X-Ray engine configs and string tables.

/// Mapping for Windows-1251 bytes 0x80..=0xFF to Unicode codepoints.
pub const CP1251_TABLE: [u16; 128] = [
    1026, 1027, 8218, 1107, 8222, 8230, 8224, 8225, 8364, 8240, 1033, 8249, 1034, 1036, 1035, 1039, 1106, 8216, 8217,
    8220, 8221, 8226, 8211, 8212, 65533, 8482, 1113, 8250, 1114, 1116, 1115, 1119, 160, 1038, 1118, 1032, 164, 1168,
    166, 167, 1025, 169, 1028, 171, 172, 173, 174, 1031, 176, 177, 1030, 1110, 1169, 181, 182, 183, 1105, 8470, 1108,
    187, 1112, 1029, 1109, 1111, 1040, 1041, 1042, 1043, 1044, 1045, 1046, 1047, 1048, 1049, 1050, 1051, 1052, 1053,
    1054, 1055, 1056, 1057, 1058, 1059, 1060, 1061, 1062, 1063, 1064, 1065, 1066, 1067, 1068, 1069, 1070, 1071, 1072,
    1073, 1074, 1075, 1076, 1077, 1078, 1079, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1088, 1089, 1090, 1091,
    1092, 1093, 1094, 1095, 1096, 1097, 1098, 1099, 1100, 1101, 1102, 1103,
];

/// Mapping for Windows-1250 bytes 0x80..=0xFF to Unicode codepoints.
pub const CP1250_TABLE: [u16; 128] = [
    8364, 65533, 8218, 65533, 8222, 8230, 8224, 8225, 65533, 8240, 352, 8249, 346, 356, 381, 377, 65533, 8216, 8217,
    8220, 8221, 8226, 8211, 8212, 65533, 8482, 353, 8250, 347, 357, 382, 378, 160, 711, 728, 321, 164, 260, 166, 167,
    168, 169, 350, 171, 172, 173, 174, 379, 176, 177, 731, 322, 180, 181, 182, 183, 184, 261, 351, 187, 317, 733, 318,
    380, 340, 193, 194, 258, 196, 313, 262, 199, 268, 201, 280, 203, 282, 205, 206, 270, 272, 323, 327, 211, 212, 336,
    214, 215, 344, 366, 218, 368, 220, 221, 354, 223, 341, 225, 226, 259, 228, 314, 263, 231, 269, 233, 281, 235, 283,
    237, 238, 271, 273, 324, 328, 243, 244, 337, 246, 247, 345, 367, 250, 369, 252, 253, 355, 729,
];

/// UTF-8 byte order mark.
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Decodes bytes from Windows-1251.
#[must_use]
pub fn decode_windows_1251(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte < 0x80 {
            out.push(char::from(byte));
        } else {
            let offset = (byte.saturating_sub(0x80)) as usize;
            let code = CP1251_TABLE.get(offset).copied().unwrap_or(65533);
            let ch = char::from_u32(u32::from(code)).unwrap_or('\u{FFFD}');
            out.push(ch);
        }
    }
    out
}

/// Decodes bytes from Windows-1250.
#[must_use]
pub fn decode_windows_1250(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte < 0x80 {
            out.push(char::from(byte));
        } else {
            let offset = (byte.saturating_sub(0x80)) as usize;
            let code = CP1250_TABLE.get(offset).copied().unwrap_or(65533);
            let ch = char::from_u32(u32::from(code)).unwrap_or('\u{FFFD}');
            out.push(ch);
        }
    }
    out
}

/// Decodes text by trying UTF-8 first (stripping BOM if present), falling back to Windows-1251.
#[must_use]
pub fn decode_text(data: &[u8]) -> String {
    let bytes = if data.starts_with(&UTF8_BOM) {
        data.get(3..).unwrap_or(&[])
    } else {
        data
    };

    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_windows_1251(bytes),
    }
}

/// Decodes an archive entry file name, trying UTF-8, then Windows-1251, then lossy UTF-8.
#[must_use]
pub fn decode_archive_name(value: &[u8]) -> String {
    let bytes = if value.starts_with(&UTF8_BOM) {
        value.get(3..).unwrap_or(&[])
    } else {
        value
    };

    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.replace('\\', "/");
    }

    // Check if Windows-1251 decodes without replacement characters
    let cp1251 = decode_windows_1251(bytes);
    if !cp1251.contains('\u{FFFD}') {
        return cp1251.replace('\\', "/");
    }

    String::from_utf8_lossy(bytes).replace('\\', "/")
}
