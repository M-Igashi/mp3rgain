//! System CJK font fallback (issue #321).
//!
//! egui's built-in fonts cover Latin, Greek and Cyrillic only, so a filename
//! like `01-日本語の曲.mp3` renders its kana and kanji as missing-glyph boxes.
//! Bundling Noto Sans CJK would add 15-20 MB to every binary, so instead the
//! first CJK font the OS already ships is appended to both font families as a
//! fallback. Only glyphs missing from egui's own fonts are taken from it, so
//! Latin text is unchanged.
//!
//! Exactly one font is loaded, at startup, for every user: a file name can
//! need it whatever the UI language, and reading it takes a few
//! milliseconds. The bytes are kept for the life of the process and handed to
//! egui as static data, which it parses in place; owned bytes it copies, so
//! the Japanese face cost 15 MB resident instead of 7, and the Korean one
//! 105 MB instead of 52 (issue #470).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// Environment variable naming a font file to use instead of the built-in
/// candidate list. The escape hatch for an OS font set we did not predict.
pub const FONT_ENV: &str = "MP3RGUI_FONT";

/// Language groups a candidate font primarily serves. The locale from
/// `i18n::font_locale` moves its group to the front; Japanese is the default
/// order since every CJK font covers kana and most kanji, while Hangul needs
/// a Korean face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Ja,
    Zh,
    Ko,
}

#[cfg(target_os = "macos")]
const CANDIDATES: &[(Lang, &str)] = &[
    (Lang::Ja, "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
    (Lang::Zh, "/System/Library/Fonts/PingFang.ttc"),
    (Lang::Zh, "/System/Library/Fonts/Hiragino Sans GB.ttc"),
    (Lang::Ko, "/System/Library/Fonts/AppleSDGothicNeo.ttc"),
    (
        Lang::Ko,
        "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
    ),
    (
        Lang::Ja,
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    ),
];

#[cfg(windows)]
const CANDIDATES: &[(Lang, &str)] = &[
    (Lang::Ja, "YuGothM.ttc"),
    (Lang::Ja, "meiryo.ttc"),
    (Lang::Ja, "msgothic.ttc"),
    (Lang::Zh, "msyh.ttc"),
    (Lang::Zh, "simsun.ttc"),
    (Lang::Ko, "malgun.ttf"),
];

#[cfg(not(any(target_os = "macos", windows)))]
const CANDIDATES: &[(Lang, &str)] = &[
    (
        Lang::Ja,
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ),
    (
        Lang::Ja,
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ),
    (
        Lang::Ja,
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ),
    (
        Lang::Ja,
        "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf",
    ),
    (
        Lang::Ja,
        "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    ),
    (
        Lang::Ja,
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    ),
    (Lang::Zh, "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
    (Lang::Ko, "/usr/share/fonts/truetype/nanum/NanumGothic.ttf"),
];

/// Install egui's default fonts plus the system CJK fallback, if one exists.
/// `locale` (`zh_CN`, `ko_KR`, `en_US`) comes from `i18n::font_locale`.
pub fn install(ctx: &egui::Context, locale: &str) {
    let mut defs = egui::FontDefinitions::default();
    if let Some((path, bytes)) = load_fallback(locale) {
        let name = path.display().to_string();
        defs.font_data
            .insert(name.clone(), Arc::new(egui::FontData::from_static(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(defs);
}

/// Font files read so far. Each is read once and never freed, so a language
/// switch that changes the CJK group reads its face once and a switch back
/// reuses the first.
static LOADED: Mutex<Vec<(PathBuf, &'static [u8])>> = Mutex::new(Vec::new());

fn load_fallback(locale: &str) -> Option<(PathBuf, &'static [u8])> {
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    fallback_candidates(locale).into_iter().find_map(|path| {
        if let Some(&(_, bytes)) = loaded.iter().find(|(p, _)| *p == path) {
            return Some((path, bytes));
        }
        let bytes: &'static [u8] = Box::leak(std::fs::read(&path).ok()?.into_boxed_slice());
        loaded.push((path.clone(), bytes));
        Some((path, bytes))
    })
}

/// Candidate font files in preference order: the explicit override, the
/// fontconfig answer on Linux, then the static list with the locale's group
/// first.
fn fallback_candidates(locale: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os(FONT_ENV).filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    let preferred = lang_from_locale(locale);
    #[cfg(not(any(target_os = "macos", windows)))]
    paths.extend(fontconfig_match(preferred));
    paths.extend(ordered_static_candidates(preferred));
    paths
}

fn ordered_static_candidates(preferred: Lang) -> Vec<PathBuf> {
    let (first, rest): (Vec<_>, Vec<_>) = CANDIDATES.iter().partition(|(l, _)| *l == preferred);
    first
        .into_iter()
        .chain(rest)
        .map(|(_, p)| font_path(p))
        .collect()
}

#[cfg(windows)]
fn font_path(name: &str) -> PathBuf {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(windir).join("Fonts").join(name)
}

#[cfg(not(windows))]
fn font_path(name: &str) -> PathBuf {
    PathBuf::from(name)
}

/// Whether two locales want the same CJK fallback face.
pub fn same_cjk_group(a: &str, b: &str) -> bool {
    lang_from_locale(a) == lang_from_locale(b)
}

/// Whether `locale` is Chinese, Japanese or Korean, the languages a font
/// group exists for.
pub fn is_cjk(locale: &str) -> bool {
    matches!(
        locale.get(..2).map(str::to_ascii_lowercase).as_deref(),
        Some("zh" | "ja" | "ko")
    )
}

fn lang_from_locale(locale: &str) -> Lang {
    match locale.get(..2).map(str::to_ascii_lowercase).as_deref() {
        Some("zh") => Lang::Zh,
        Some("ko") => Lang::Ko,
        _ => Lang::Ja,
    }
}

/// Ask fontconfig which file it would use for the language. Returns the
/// path only when fontconfig actually found a font declaring that language,
/// so a Latin-only system does not get DejaVu loaded twice for nothing.
#[cfg(not(any(target_os = "macos", windows)))]
fn fontconfig_match(preferred: Lang) -> Option<PathBuf> {
    let lang = match preferred {
        Lang::Ja => "ja",
        Lang::Zh => "zh",
        Lang::Ko => "ko",
    };
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{lang}", &format!("sans-serif:lang={lang}")])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let (file, langs) = text.trim().split_once('\n')?;
    langs
        .split('|')
        .any(|l| l == lang)
        .then(|| PathBuf::from(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_locale_selects_cjk_group() {
        assert!(same_cjk_group("zh_CN", "zh-Hans"));
        assert!(same_cjk_group("ko_KR", "ko"));
        assert!(!same_cjk_group("zh_CN", "ko_KR"));
        assert!(same_cjk_group("pt_BR", "en_US"));
        assert!(same_cjk_group("", "ja_JP.UTF-8"));
    }

    #[test]
    fn locale_picks_language_group() {
        assert_eq!(lang_from_locale("ja_JP.UTF-8"), Lang::Ja);
        assert_eq!(lang_from_locale("zh_CN.UTF-8"), Lang::Zh);
        assert_eq!(lang_from_locale("ko_KR.UTF-8"), Lang::Ko);
        assert_eq!(lang_from_locale("en_US.UTF-8"), Lang::Ja);
        assert_eq!(lang_from_locale("C"), Lang::Ja);
    }

    #[test]
    fn preferred_group_comes_first() {
        let ordered = ordered_static_candidates(Lang::Ko);
        let ko: Vec<PathBuf> = CANDIDATES
            .iter()
            .filter(|(l, _)| *l == Lang::Ko)
            .map(|(_, p)| font_path(p))
            .collect();
        assert_eq!(&ordered[..ko.len()], &ko[..]);
        assert_eq!(ordered.len(), CANDIDATES.len());
    }

    /// End-to-end on a machine that has a system CJK font: the fallback must
    /// actually supply the glyphs from the issue #321 filename.
    #[test]
    fn japanese_glyphs_resolve_when_a_system_font_exists() {
        let Some((path, bytes)) = load_fallback("ja") else {
            eprintln!("no system CJK font on this machine, skipping");
            return;
        };
        let mut defs = egui::FontDefinitions::default();
        let name = path.display().to_string();
        defs.font_data
            .insert(name.clone(), Arc::new(egui::FontData::from_static(bytes)));
        defs.families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(name);
        let fonts = egui::epaint::text::Fonts::new(1.0, 2048, defs);
        let id = egui::FontId::proportional(14.0);
        assert!(
            fonts.has_glyphs(&id, "01-日本語の曲.mp3"),
            "font: {}",
            path.display()
        );
        assert!(fonts.has_glyphs(&id, "abc-123"));

        // A second install, after a language switch back, reuses the bytes
        // rather than keeping another copy (issue #470).
        let (_, again) = load_fallback("ja").unwrap();
        assert!(std::ptr::eq(bytes, again));
    }
}
