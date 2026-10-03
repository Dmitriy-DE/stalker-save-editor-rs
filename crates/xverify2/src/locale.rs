//! Locale-specific plural rules and compact UI formatting for the supported languages.

use sse_core::{Error, Result};
use std::fmt::Write as _;

const NBSP: char = '\u{00a0}';
const NNBSP: char = '\u{202f}';

/// Locales supported by the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    /// Russian.
    Ru,
    /// Ukrainian.
    Uk,
    /// English.
    En,
    /// German.
    De,
    /// French.
    Fr,
    /// Spanish.
    Es,
    /// Italian.
    It,
    /// Polish.
    Pl,
    /// Czech.
    Cs,
    /// Turkish.
    Tr,
    /// Brazilian Portuguese.
    PtBr,
    /// Japanese.
    Ja,
    /// Korean.
    Ko,
    /// Simplified Chinese.
    ZhCn,
    /// Traditional Chinese.
    ZhTw,
}

/// CLDR cardinal plural categories used by the supported locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    /// Singular category.
    One,
    /// Paucal category.
    Few,
    /// Large-count category.
    Many,
    /// Fallback category.
    Other,
}

/// Decimal operands needed by CLDR cardinal rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluralOperands {
    /// Absolute integer part.
    pub integer: u64,
    /// Number of visible fraction digits, including trailing zeroes.
    pub visible_fraction_digits: u8,
    /// Visible fraction digits interpreted as an integer.
    pub fraction: u64,
}

impl PluralOperands {
    /// Creates operands for an integer value.
    #[must_use]
    pub const fn integer(value: u64) -> Self {
        Self {
            integer: value,
            visible_fraction_digits: 0,
            fraction: 0,
        }
    }
}

/// A validated civil date used for UI formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    /// Gregorian year.
    pub year: u16,
    /// Month in `1..=12`.
    pub month: u8,
    /// Day in `1..=31`.
    pub day: u8,
}

/// Relative-time units supported by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeUnit {
    /// Seconds.
    Second,
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
}

#[derive(Debug, Clone, Copy)]
struct NumberStyle {
    group: char,
    decimal: char,
    minimum_group_digits: u8,
}

/// Returns the CLDR cardinal category for already-separated decimal operands.
#[must_use]
pub fn plural_cardinal(locale: Locale, value: PluralOperands) -> PluralCategory {
    let i = value.integer;
    let v = value.visible_fraction_digits;
    let mod10 = i.rem_euclid(10);
    let mod100 = i.rem_euclid(100);
    match locale {
        Locale::Ru | Locale::Uk => {
            if v == 0 && mod10 == 1 && mod100 != 11 {
                PluralCategory::One
            } else if v == 0 && (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else if v == 0 && (mod10 == 0 || (5..=9).contains(&mod10) || (11..=14).contains(&mod100)) {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        Locale::Pl => {
            if v == 0 && i == 1 {
                PluralCategory::One
            } else if v == 0 && (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else if v == 0 && i != 1 && (mod10 <= 1 || (5..=9).contains(&mod10) || (12..=14).contains(&mod100)) {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        Locale::Cs => {
            if v == 0 && i == 1 {
                PluralCategory::One
            } else if v == 0 && (2..=4).contains(&i) {
                PluralCategory::Few
            } else if v != 0 {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        Locale::Fr | Locale::PtBr => {
            if i <= 1 {
                PluralCategory::One
            } else if v == 0 && i != 0 && i.rem_euclid(1_000_000) == 0 {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        Locale::Es | Locale::It => {
            if v == 0 && i == 1 {
                PluralCategory::One
            } else if v == 0 && i != 0 && i.rem_euclid(1_000_000) == 0 {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        Locale::En | Locale::De => {
            if v == 0 && i == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        Locale::Tr | Locale::Ja | Locale::Ko | Locale::ZhCn | Locale::ZhTw => PluralCategory::Other,
    }
}

/// Writes a locale-formatted signed decimal number into `out` without constructing an intermediate `String`.
///
/// `fraction` is `(digits, visible_digits)`: `(5, 2)` formats as `.05`/`,05`.
pub fn format_number(out: &mut String, locale: Locale, integer: i64, fraction: Option<(u64, u8)>) -> Result<()> {
    let style = number_style(locale);
    if integer < 0 {
        out.push('-');
    }
    write_grouped_unsigned(out, integer.unsigned_abs(), style)?;
    if let Some((digits, visible)) = fraction {
        if visible != 0 {
            out.push(style.decimal);
            write_fraction(out, digits, visible)?;
        }
    }
    Ok(())
}

/// Writes a human-readable binary size using locale-specific unit spelling.
pub fn format_size(out: &mut String, locale: Locale, bytes: u64) -> Result<()> {
    const KIB: u64 = 1024;
    const MIB: u64 = 1_048_576;
    const GIB: u64 = 1_073_741_824;
    let (divisor, unit) = if bytes >= GIB {
        (GIB, size_unit(locale, 3))
    } else if bytes >= MIB {
        (MIB, size_unit(locale, 2))
    } else if bytes >= KIB {
        (KIB, size_unit(locale, 1))
    } else {
        (1, size_unit(locale, 0))
    };
    if divisor == 1 {
        let value = i64::try_from(bytes).map_err(|_| Error::damaged("byte count does not fit i64"))?;
        format_number(out, locale, value, None)?;
    } else {
        let whole = bytes.div_euclid(divisor);
        let remainder = bytes.rem_euclid(divisor);
        let tenth = remainder
            .saturating_mul(10)
            .saturating_add(divisor.div_euclid(2))
            .div_euclid(divisor);
        let mut rounded_whole = whole;
        let mut rounded_tenth = tenth;
        if rounded_tenth >= 10 {
            rounded_whole = rounded_whole.saturating_add(1);
            rounded_tenth = 0;
        }
        let signed = i64::try_from(rounded_whole).map_err(|_| Error::damaged("scaled size does not fit i64"))?;
        let fraction = if rounded_tenth == 0 {
            None
        } else {
            Some((rounded_tenth, 1))
        };
        format_number(out, locale, signed, fraction)?;
    }
    out.push(' ');
    out.push_str(unit);
    Ok(())
}

/// Writes an in-game amount followed by the invariant `RU` suffix.
pub fn format_money_ru(out: &mut String, locale: Locale, amount: i64) -> Result<()> {
    format_number(out, locale, amount, None)?;
    out.push_str(" RU");
    Ok(())
}

/// Writes a locale-specific short date.
pub fn format_date(out: &mut String, locale: Locale, date: Date) -> Result<()> {
    validate_date(date)?;
    match locale {
        Locale::En => write!(out, "{:02}/{:02}/{:04}", date.month, date.day, date.year),
        Locale::Ja => write!(out, "{:04}/{:02}/{:02}", date.year, date.month, date.day),
        Locale::Ko => write!(out, "{:04}. {:02}. {:02}.", date.year, date.month, date.day),
        Locale::ZhCn | Locale::ZhTw => write!(out, "{}年{}月{}日", date.year, date.month, date.day),
        Locale::Ru | Locale::Uk | Locale::De | Locale::Pl | Locale::Cs | Locale::Tr => {
            write!(out, "{:02}.{:02}.{:04}", date.day, date.month, date.year)
        }
        Locale::Fr | Locale::Es | Locale::It | Locale::PtBr => {
            write!(out, "{:02}/{:02}/{:04}", date.day, date.month, date.year)
        }
    }
    .map_err(|error| Error::damaged(error.to_string()))
}

/// Writes a past relative time such as `3 minutes ago`. `value` must be non-negative.
pub fn format_relative(out: &mut String, locale: Locale, value: i64, unit: RelativeUnit) -> Result<()> {
    if value < 0 {
        return Err(Error::damaged("relative time must be non-negative"));
    }
    if unit == RelativeUnit::Day && value == 1 {
        out.push_str(yesterday(locale));
        return Ok(());
    }
    let unsigned = u64::try_from(value).map_err(|_| Error::damaged("relative value conversion failed"))?;
    let category = plural_cardinal(locale, PluralOperands::integer(unsigned));
    match locale {
        Locale::Ru => write_number_word_suffix(out, locale, value, ru_relative(unit, category), " назад"),
        Locale::Uk => write_number_word_suffix(out, locale, value, uk_relative(unit, category), " тому"),
        Locale::En => write_number_word_suffix(out, locale, value, en_relative(unit, category), " ago"),
        Locale::De => {
            out.push_str("vor ");
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(de_relative(unit, category));
            Ok(())
        }
        Locale::Fr => {
            out.push_str("il y a ");
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(fr_relative(unit, category));
            Ok(())
        }
        Locale::Es => {
            out.push_str("hace ");
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(es_relative(unit, category));
            Ok(())
        }
        Locale::It => write_number_word_suffix(out, locale, value, it_relative(unit, category), " fa"),
        Locale::Pl => write_number_word_suffix(out, locale, value, pl_relative(unit, category), " temu"),
        Locale::Cs => {
            out.push_str("před ");
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(cs_relative(unit, category));
            Ok(())
        }
        Locale::Tr => write_number_word_suffix(out, locale, value, tr_relative(unit), " önce"),
        Locale::PtBr => {
            out.push_str("há ");
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(pt_relative(unit, category));
            Ok(())
        }
        Locale::Ja => {
            format_number(out, locale, value, None)?;
            out.push(' ');
            out.push_str(ja_relative(unit));
            out.push_str("前");
            Ok(())
        }
        Locale::Ko => {
            format_number(out, locale, value, None)?;
            out.push_str(ko_relative(unit));
            out.push_str(" 전");
            Ok(())
        }
        Locale::ZhCn | Locale::ZhTw => {
            format_number(out, locale, value, None)?;
            out.push_str(zh_relative(locale, unit));
            out.push_str(if locale == Locale::ZhCn { "前" } else { "前" });
            Ok(())
        }
    }
}

/// Joins list items using locale punctuation and the locale's final conjunction.
pub fn join_list(out: &mut String, locale: Locale, items: &[&str]) {
    match items {
        [] => {}
        [only] => out.push_str(only),
        [first, second] => {
            out.push_str(first);
            out.push_str(two_separator(locale));
            out.push_str(second);
        }
        _ => {
            let last_index = items.len().saturating_sub(1);
            for (index, item) in items.iter().enumerate() {
                if index == 0 {
                    out.push_str(item);
                } else if index == last_index {
                    out.push_str(final_separator(locale));
                    out.push_str(item);
                } else {
                    out.push_str(middle_separator(locale));
                    out.push_str(item);
                }
            }
        }
    }
}

fn write_number_word_suffix(out: &mut String, locale: Locale, value: i64, word: &str, suffix: &str) -> Result<()> {
    format_number(out, locale, value, None)?;
    out.push(' ');
    out.push_str(word);
    out.push_str(suffix);
    Ok(())
}

fn number_style(locale: Locale) -> NumberStyle {
    match locale {
        Locale::Ru | Locale::Uk | Locale::Pl | Locale::Cs => NumberStyle {
            group: NBSP,
            decimal: ',',
            minimum_group_digits: 1,
        },
        Locale::Fr => NumberStyle {
            group: NNBSP,
            decimal: ',',
            minimum_group_digits: 1,
        },
        Locale::De | Locale::It | Locale::Tr | Locale::PtBr => NumberStyle {
            group: '.',
            decimal: ',',
            minimum_group_digits: 1,
        },
        Locale::Es => NumberStyle {
            group: '.',
            decimal: ',',
            minimum_group_digits: 2,
        },
        Locale::En | Locale::Ja | Locale::Ko | Locale::ZhCn | Locale::ZhTw => NumberStyle {
            group: ',',
            decimal: '.',
            minimum_group_digits: 1,
        },
    }
}

fn write_grouped_unsigned(out: &mut String, value: u64, style: NumberStyle) -> Result<()> {
    let mut digits = [0_u8; 20];
    let mut count = 0_usize;
    let mut remaining = value;
    loop {
        let digit =
            u8::try_from(remaining.rem_euclid(10)).map_err(|_| Error::damaged("decimal digit conversion failed"))?;
        let slot = digits
            .get_mut(count)
            .ok_or_else(|| Error::damaged("number has too many decimal digits"))?;
        *slot = digit;
        count = count
            .checked_add(1)
            .ok_or_else(|| Error::damaged("digit count overflow"))?;
        remaining = remaining.div_euclid(10);
        if remaining == 0 {
            break;
        }
    }
    let group = count >= usize::from(style.minimum_group_digits).saturating_add(3);
    for reverse_index in (0..count).rev() {
        if group && reverse_index != count.saturating_sub(1) && reverse_index.saturating_add(1).rem_euclid(3) == 0 {
            out.push(style.group);
        }
        let digit = digits
            .get(reverse_index)
            .copied()
            .ok_or_else(|| Error::damaged("digit index outside buffer"))?;
        out.push(char::from(b'0'.saturating_add(digit)));
    }
    Ok(())
}

fn write_fraction(out: &mut String, digits: u64, visible: u8) -> Result<()> {
    if visible > 18 {
        return Err(Error::damaged("too many visible fraction digits"));
    }
    let mut divisor = 1_u64;
    for _ in 1..visible {
        divisor = divisor.saturating_mul(10);
    }
    let modulus = divisor.saturating_mul(10);
    let normalized = if modulus == 0 {
        digits
    } else {
        digits.rem_euclid(modulus)
    };
    let mut current_divisor = divisor;
    for _ in 0..visible {
        let digit = normalized.div_euclid(current_divisor).rem_euclid(10);
        let digit = u8::try_from(digit).map_err(|_| Error::damaged("fraction digit conversion failed"))?;
        out.push(char::from(b'0'.saturating_add(digit)));
        current_divisor = current_divisor.div_euclid(10).max(1);
    }
    Ok(())
}

fn validate_date(date: Date) -> Result<()> {
    if !(1..=12).contains(&date.month) || !(1..=31).contains(&date.day) {
        return Err(Error::damaged("invalid civil date"));
    }
    Ok(())
}

fn size_unit(locale: Locale, power: u8) -> &'static str {
    match locale {
        Locale::Ru | Locale::Uk => match power {
            0 => "Б",
            1 => "КБ",
            2 => "МБ",
            _ => "ГБ",
        },
        Locale::ZhCn => match power {
            0 => "字节",
            1 => "千字节",
            2 => "兆字节",
            _ => "吉字节",
        },
        Locale::ZhTw => match power {
            0 => "位元組",
            1 => "KB",
            2 => "MB",
            _ => "GB",
        },
        _ => match power {
            0 => "B",
            1 => "KB",
            2 => "MB",
            _ => "GB",
        },
    }
}

fn yesterday(locale: Locale) -> &'static str {
    match locale {
        Locale::Ru => "вчера",
        Locale::Uk => "вчора",
        Locale::En => "yesterday",
        Locale::De => "gestern",
        Locale::Fr => "hier",
        Locale::Es => "ayer",
        Locale::It => "ieri",
        Locale::Pl => "wczoraj",
        Locale::Cs => "včera",
        Locale::Tr => "dün",
        Locale::PtBr => "ontem",
        Locale::Ja => "昨日",
        Locale::Ko => "어제",
        Locale::ZhCn => "昨天",
        Locale::ZhTw => "昨天",
    }
}

fn ru_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    match unit {
        RelativeUnit::Second => match cat {
            PluralCategory::One => "секунду",
            PluralCategory::Few => "секунды",
            _ => "секунд",
        },
        RelativeUnit::Minute => match cat {
            PluralCategory::One => "минуту",
            PluralCategory::Few => "минуты",
            _ => "минут",
        },
        RelativeUnit::Hour => match cat {
            PluralCategory::One => "час",
            PluralCategory::Few => "часа",
            _ => "часов",
        },
        RelativeUnit::Day => match cat {
            PluralCategory::One => "день",
            PluralCategory::Few => "дня",
            _ => "дней",
        },
    }
}
fn uk_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    match unit {
        RelativeUnit::Second => match cat {
            PluralCategory::One => "секунду",
            PluralCategory::Few => "секунди",
            _ => "секунд",
        },
        RelativeUnit::Minute => match cat {
            PluralCategory::One => "хвилину",
            PluralCategory::Few => "хвилини",
            _ => "хвилин",
        },
        RelativeUnit::Hour => match cat {
            PluralCategory::One => "годину",
            PluralCategory::Few => "години",
            _ => "годин",
        },
        RelativeUnit::Day => match cat {
            PluralCategory::One => "день",
            PluralCategory::Few => "дні",
            _ => "днів",
        },
    }
}
fn en_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "second"
            } else {
                "seconds"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "minute"
            } else {
                "minutes"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "hour"
            } else {
                "hours"
            }
        }
        RelativeUnit::Day => {
            if one {
                "day"
            } else {
                "days"
            }
        }
    }
}
fn de_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "Sekunde"
            } else {
                "Sekunden"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "Minute"
            } else {
                "Minuten"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "Stunde"
            } else {
                "Stunden"
            }
        }
        RelativeUnit::Day => {
            if one {
                "Tag"
            } else {
                "Tagen"
            }
        }
    }
}
fn fr_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "seconde"
            } else {
                "secondes"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "minute"
            } else {
                "minutes"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "heure"
            } else {
                "heures"
            }
        }
        RelativeUnit::Day => {
            if one {
                "jour"
            } else {
                "jours"
            }
        }
    }
}
fn es_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "segundo"
            } else {
                "segundos"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "minuto"
            } else {
                "minutos"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "hora"
            } else {
                "horas"
            }
        }
        RelativeUnit::Day => {
            if one {
                "día"
            } else {
                "días"
            }
        }
    }
}
fn it_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "secondo"
            } else {
                "secondi"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "minuto"
            } else {
                "minuti"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "ora"
            } else {
                "ore"
            }
        }
        RelativeUnit::Day => {
            if one {
                "giorno"
            } else {
                "giorni"
            }
        }
    }
}
fn pl_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    match unit {
        RelativeUnit::Second => match cat {
            PluralCategory::One => "sekundę",
            PluralCategory::Few => "sekundy",
            _ => "sekund",
        },
        RelativeUnit::Minute => match cat {
            PluralCategory::One => "minutę",
            PluralCategory::Few => "minuty",
            _ => "minut",
        },
        RelativeUnit::Hour => match cat {
            PluralCategory::One => "godzinę",
            PluralCategory::Few => "godziny",
            _ => "godzin",
        },
        RelativeUnit::Day => match cat {
            PluralCategory::One => "dzień",
            PluralCategory::Few => "dni",
            _ => "dni",
        },
    }
}
fn cs_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    match unit {
        RelativeUnit::Second => {
            if cat == PluralCategory::One {
                "sekundou"
            } else {
                "sekundami"
            }
        }
        RelativeUnit::Minute => {
            if cat == PluralCategory::One {
                "minutou"
            } else {
                "minutami"
            }
        }
        RelativeUnit::Hour => {
            if cat == PluralCategory::One {
                "hodinou"
            } else {
                "hodinami"
            }
        }
        RelativeUnit::Day => {
            if cat == PluralCategory::One {
                "dnem"
            } else {
                "dny"
            }
        }
    }
}
fn tr_relative(unit: RelativeUnit) -> &'static str {
    match unit {
        RelativeUnit::Second => "saniye",
        RelativeUnit::Minute => "dakika",
        RelativeUnit::Hour => "saat",
        RelativeUnit::Day => "gün",
    }
}
fn pt_relative(unit: RelativeUnit, cat: PluralCategory) -> &'static str {
    let one = cat == PluralCategory::One;
    match unit {
        RelativeUnit::Second => {
            if one {
                "segundo"
            } else {
                "segundos"
            }
        }
        RelativeUnit::Minute => {
            if one {
                "minuto"
            } else {
                "minutos"
            }
        }
        RelativeUnit::Hour => {
            if one {
                "hora"
            } else {
                "horas"
            }
        }
        RelativeUnit::Day => {
            if one {
                "dia"
            } else {
                "dias"
            }
        }
    }
}
fn ja_relative(unit: RelativeUnit) -> &'static str {
    match unit {
        RelativeUnit::Second => "秒",
        RelativeUnit::Minute => "分",
        RelativeUnit::Hour => "時間",
        RelativeUnit::Day => "日",
    }
}
fn ko_relative(unit: RelativeUnit) -> &'static str {
    match unit {
        RelativeUnit::Second => "초",
        RelativeUnit::Minute => "분",
        RelativeUnit::Hour => "시간",
        RelativeUnit::Day => "일",
    }
}
fn zh_relative(locale: Locale, unit: RelativeUnit) -> &'static str {
    match (locale, unit) {
        (Locale::ZhTw, RelativeUnit::Second) => "秒",
        (Locale::ZhTw, RelativeUnit::Minute) => " 分鐘",
        (Locale::ZhTw, RelativeUnit::Hour) => " 小時",
        (Locale::ZhTw, RelativeUnit::Day) => " 天",
        (_, RelativeUnit::Second) => "秒",
        (_, RelativeUnit::Minute) => "分钟",
        (_, RelativeUnit::Hour) => "小时",
        (_, RelativeUnit::Day) => "天",
    }
}

fn two_separator(locale: Locale) -> &'static str {
    final_separator(locale)
}
fn middle_separator(locale: Locale) -> &'static str {
    match locale {
        Locale::Ja | Locale::ZhCn | Locale::ZhTw => "、",
        _ => ", ",
    }
}
fn final_separator(locale: Locale) -> &'static str {
    match locale {
        Locale::Ru => " и ",
        Locale::Uk => " і ",
        Locale::En => " and ",
        Locale::De => " und ",
        Locale::Fr => " et ",
        Locale::Es => " y ",
        Locale::It => " e ",
        Locale::Pl => " i ",
        Locale::Cs => " a ",
        Locale::Tr => " ve ",
        Locale::PtBr => " e ",
        Locale::Ko => " 및 ",
        Locale::Ja => "、",
        Locale::ZhCn | Locale::ZhTw => " 和 ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        locale: Locale,
        number: &'static str,
        size: &'static str,
        money: &'static str,
        date: &'static str,
        minutes: &'static str,
        yesterday: &'static str,
        list: &'static str,
        one: PluralCategory,
        two: PluralCategory,
        five: PluralCategory,
    }

    const CASES: &[Case] = &[
        Case {
            locale: Locale::Ru,
            number: "1\u{00a0}234\u{00a0}567,05",
            size: "1,5 МБ",
            money: "12\u{00a0}345 RU",
            date: "03.10.2026",
            minutes: "3 минуты назад",
            yesterday: "вчера",
            list: "a, b и c",
            one: PluralCategory::One,
            two: PluralCategory::Few,
            five: PluralCategory::Many,
        },
        Case {
            locale: Locale::Uk,
            number: "1\u{00a0}234\u{00a0}567,05",
            size: "1,5 МБ",
            money: "12\u{00a0}345 RU",
            date: "03.10.2026",
            minutes: "3 хвилини тому",
            yesterday: "вчора",
            list: "a, b і c",
            one: PluralCategory::One,
            two: PluralCategory::Few,
            five: PluralCategory::Many,
        },
        Case {
            locale: Locale::En,
            number: "1,234,567.05",
            size: "1.5 MB",
            money: "12,345 RU",
            date: "10/03/2026",
            minutes: "3 minutes ago",
            yesterday: "yesterday",
            list: "a, b and c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::De,
            number: "1.234.567,05",
            size: "1,5 MB",
            money: "12.345 RU",
            date: "03.10.2026",
            minutes: "vor 3 Minuten",
            yesterday: "gestern",
            list: "a, b und c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Fr,
            number: "1\u{202f}234\u{202f}567,05",
            size: "1,5 MB",
            money: "12\u{202f}345 RU",
            date: "03/10/2026",
            minutes: "il y a 3 minutes",
            yesterday: "hier",
            list: "a, b et c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Es,
            number: "1.234.567,05",
            size: "1,5 MB",
            money: "12.345 RU",
            date: "03/10/2026",
            minutes: "hace 3 minutos",
            yesterday: "ayer",
            list: "a, b y c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::It,
            number: "1.234.567,05",
            size: "1,5 MB",
            money: "12.345 RU",
            date: "03/10/2026",
            minutes: "3 minuti fa",
            yesterday: "ieri",
            list: "a, b e c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Pl,
            number: "1\u{00a0}234\u{00a0}567,05",
            size: "1,5 MB",
            money: "12\u{00a0}345 RU",
            date: "03.10.2026",
            minutes: "3 minuty temu",
            yesterday: "wczoraj",
            list: "a, b i c",
            one: PluralCategory::One,
            two: PluralCategory::Few,
            five: PluralCategory::Many,
        },
        Case {
            locale: Locale::Cs,
            number: "1\u{00a0}234\u{00a0}567,05",
            size: "1,5 MB",
            money: "12\u{00a0}345 RU",
            date: "03.10.2026",
            minutes: "před 3 minutami",
            yesterday: "včera",
            list: "a, b a c",
            one: PluralCategory::One,
            two: PluralCategory::Few,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Tr,
            number: "1.234.567,05",
            size: "1,5 MB",
            money: "12.345 RU",
            date: "03.10.2026",
            minutes: "3 dakika önce",
            yesterday: "dün",
            list: "a, b ve c",
            one: PluralCategory::Other,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::PtBr,
            number: "1.234.567,05",
            size: "1,5 MB",
            money: "12.345 RU",
            date: "03/10/2026",
            minutes: "há 3 minutos",
            yesterday: "ontem",
            list: "a, b e c",
            one: PluralCategory::One,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Ja,
            number: "1,234,567.05",
            size: "1.5 MB",
            money: "12,345 RU",
            date: "2026/10/03",
            minutes: "3 分前",
            yesterday: "昨日",
            list: "a、b、c",
            one: PluralCategory::Other,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::Ko,
            number: "1,234,567.05",
            size: "1.5 MB",
            money: "12,345 RU",
            date: "2026. 10. 03.",
            minutes: "3분 전",
            yesterday: "어제",
            list: "a, b 및 c",
            one: PluralCategory::Other,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::ZhCn,
            number: "1,234,567.05",
            size: "1.5 兆字节",
            money: "12,345 RU",
            date: "2026年10月3日",
            minutes: "3分钟前",
            yesterday: "昨天",
            list: "a、b 和 c",
            one: PluralCategory::Other,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
        Case {
            locale: Locale::ZhTw,
            number: "1,234,567.05",
            size: "1.5 MB",
            money: "12,345 RU",
            date: "2026年10月3日",
            minutes: "3 分鐘前",
            yesterday: "昨天",
            list: "a、b 和 c",
            one: PluralCategory::Other,
            two: PluralCategory::Other,
            five: PluralCategory::Other,
        },
    ];

    fn rendered_number(locale: Locale) -> String {
        let mut out = String::new();
        assert!(format_number(&mut out, locale, 1_234_567, Some((5, 2))).is_ok());
        out
    }
    fn rendered_size(locale: Locale) -> String {
        let mut out = String::new();
        assert!(format_size(&mut out, locale, 1_572_864).is_ok());
        out
    }
    fn rendered_money(locale: Locale) -> String {
        let mut out = String::new();
        assert!(format_money_ru(&mut out, locale, 12_345).is_ok());
        out
    }
    fn rendered_date(locale: Locale) -> String {
        let mut out = String::new();
        assert!(format_date(
            &mut out,
            locale,
            Date {
                year: 2026,
                month: 10,
                day: 3
            }
        )
        .is_ok());
        out
    }
    fn rendered_relative(locale: Locale, value: i64, unit: RelativeUnit) -> String {
        let mut out = String::new();
        assert!(format_relative(&mut out, locale, value, unit).is_ok());
        out
    }
    fn rendered_list(locale: Locale) -> String {
        let mut out = String::new();
        join_list(&mut out, locale, &["a", "b", "c"]);
        out
    }

    #[test]
    fn ten_cases_per_language() {
        assert_eq!(CASES.len(), 15);
        for case in CASES {
            assert_eq!(rendered_number(case.locale), case.number);
            assert_eq!(rendered_size(case.locale), case.size);
            assert_eq!(rendered_money(case.locale), case.money);
            assert_eq!(rendered_date(case.locale), case.date);
            assert_eq!(rendered_relative(case.locale, 3, RelativeUnit::Minute), case.minutes);
            assert_eq!(rendered_relative(case.locale, 1, RelativeUnit::Day), case.yesterday);
            assert_eq!(rendered_list(case.locale), case.list);
            assert_eq!(plural_cardinal(case.locale, PluralOperands::integer(1)), case.one);
            assert_eq!(plural_cardinal(case.locale, PluralOperands::integer(2)), case.two);
            assert_eq!(plural_cardinal(case.locale, PluralOperands::integer(5)), case.five);
        }
    }

    #[test]
    fn fractions_use_visible_digit_count() {
        assert_eq!(
            plural_cardinal(
                Locale::Ru,
                PluralOperands {
                    integer: 1,
                    visible_fraction_digits: 1,
                    fraction: 0
                }
            ),
            PluralCategory::Other
        );
        assert_eq!(
            plural_cardinal(
                Locale::Cs,
                PluralOperands {
                    integer: 2,
                    visible_fraction_digits: 1,
                    fraction: 5
                }
            ),
            PluralCategory::Many
        );
    }
}
