//! System CJK font fallback (issue #321).
//!
//! egui's built-in fonts cover Latin, Greek and Cyrillic only, so a filename
//! like `01-日本語の曲.mp3` renders its kana and kanji as missing-glyph boxes.
//! Bundling Noto Sans CJK would add 15-20 MB to every binary, so instead the
//! first CJK font the OS already ships is appended to both font families as a
//! fallback. Only glyphs missing from egui's own fonts are taken from it, so
//! Latin text is unchanged.
//!
//! Exactly one font is loaded. egui keeps two copies of the bytes (the
//! `FontDefinitions` and the parsed `ab_glyph` font), so loading a Japanese,
//! a Chinese and a Korean face on macOS would cost well over 100 MB.

use std::path::PathBuf;
use std::sync::Arc;

/// Environment variable naming a font file to use instead of the built-in
/// candidate list. The escape hatch for an OS font set we did not predict.
pub const FONT_ENV: &str = "MP3RGUI_FONT";

/// Language groups a candidate font primarily serves. The user's locale
/// moves its group to the front; Japanese is the default order since every
/// CJK font covers kana and most kanji, while Hangul needs a Korean face.
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
pub fn install(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    if let Some((path, bytes)) = load_fallback() {
        let name = path.display().to_string();
        defs.font_data
            .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(defs);
}

fn load_fallback() -> Option<(PathBuf, Vec<u8>)> {
    fallback_candidates()
        .into_iter()
        .find_map(|path| std::fs::read(&path).ok().map(|bytes| (path, bytes)))
}

/// Candidate font files in preference order: the explicit override, the
/// fontconfig answer on Linux, then the static list with the locale's
/// language group first.
fn fallback_candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os(FONT_ENV).filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(path));
    }
    let preferred = preferred_lang();
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

/// Language group from the POSIX locale variables. macOS apps launched from
/// Finder and Windows have none of them set, so those fall through to `Ja`.
fn preferred_lang() -> Lang {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .find(|v| !v.is_empty())
        .map(|v| lang_from_locale(&v))
        .unwrap_or(Lang::Ja)
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
        let Some((path, bytes)) = load_fallback() else {
            eprintln!("no system CJK font on this machine, skipping");
            return;
        };
        let mut defs = egui::FontDefinitions::default();
        let name = path.display().to_string();
        defs.font_data
            .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
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
    }
}
