//! GUI localisation. Catalogues under `locales/` are compiled into the binary
//! by `build.rs`. English text in the code is the msgid; a missing entry
//! shows that English.

include!(concat!(env!("OUT_DIR"), "/locale/translators.rs"));
include!(concat!(env!("OUT_DIR"), "/locale/catalogues.rs"));

/// Apply `choice` as the UI language.
///
/// `None` follows the system. `Some("en")` forces English. Any other value
/// is a catalogue name such as `pt_BR`. `sys-locale` reports `pt-BR`; gettext
/// names the file `pt_BR.po`, and `include-po` falls back from `pt_BR` to `pt`.
pub fn apply(choice: Option<&str>) {
    let resolved = resolved_locale(choice);
    let code = if choice == Some("en") {
        "en"
    } else {
        resolved.as_str()
    };
    let _ = translators::set_locale(code);
}

/// Locale whose CJK font group comes first: the UI language when it is
/// Chinese, Japanese or Korean, the system language otherwise. File names
/// come from the system and the library, not from the UI, so a Korean user
/// who picks the English UI still needs the Korean face (issue #452).
pub fn font_locale(choice: Option<&str>) -> String {
    let ui = resolved_locale(choice);
    if crate::fonts::is_cjk(&ui) {
        ui
    } else {
        resolved_locale(None)
    }
}

/// Locale code the font loader should see for this choice, in gettext form.
pub fn resolved_locale(choice: Option<&str>) -> String {
    match choice {
        Some("en") => "en".to_string(),
        Some(code) => gettext_code(code),
        None => system_locale().unwrap_or_default(),
    }
}

fn system_locale() -> Option<String> {
    sys_locale::get_locale().map(|raw| gettext_code(&raw))
}

/// `pt-BR` and `pt_BR.UTF-8` both become `pt_BR`.
pub fn gettext_code(raw: &str) -> String {
    let code = raw.split(['.', '@']).next().unwrap_or(raw);
    code.replace('-', "_")
}

/// msgid of the Language menu's own-name entry. Each catalogue translates
/// it (`Português (Brasil)`); the menu reads those through [`native_name`]
/// so the row stays in that language. This call is what `xtr` extracts.
#[allow(dead_code)]
fn language_name_msgid() -> String {
    tr!("language name" => "English")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Mutex;

    /// `tr` keeps one translator for the process. Tests that call `apply`
    /// take this so a Portuguese catalogue cannot leak into another test.
    fn locale_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCALE: Mutex<()> = Mutex::new(());
        LOCALE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn maps_sys_locale_to_gettext() {
        assert_eq!(gettext_code("pt-BR"), "pt_BR");
        assert_eq!(gettext_code("pt_BR.UTF-8"), "pt_BR");
        assert_eq!(gettext_code("en-US"), "en_US");
        assert_eq!(gettext_code("zh-CN"), "zh_CN");
        assert_eq!(gettext_code("C"), "C");
    }

    /// Issue #452: a CJK UI language picks its own font group, any other
    /// choice leaves the group to the system language.
    #[test]
    fn font_locale_follows_the_system_unless_the_ui_is_cjk() {
        let system = resolved_locale(None);
        assert_eq!(font_locale(None), system);
        assert_eq!(font_locale(Some("en")), system);
        assert_eq!(font_locale(Some("pt_BR")), system);
        assert_eq!(font_locale(Some("ko")), "ko");
        assert_eq!(font_locale(Some("zh_CN")), "zh_CN");
    }

    #[test]
    fn english_choice_shows_the_source_text() {
        let _guard = locale_lock();
        apply(Some("en"));
        assert_eq!(language_name_msgid(), "English");
        assert_eq!(tr!("Add Files..."), "Add Files...");
    }

    /// Issue #466: the capped dry run formats its own sign, since Prevent
    /// clipping can cap below zero, so it reads "-1" and not "+-1", in a
    /// catalogue too.
    #[test]
    fn capped_dry_run_keeps_its_sign() {
        let _guard = locale_lock();
        let capped = |steps| {
            crate::app::FileStatus::DryRunPredicted {
                steps,
                clipping_prevented: true,
            }
            .label()
        };
        let _ = translators::set_locale("en");
        assert_eq!(capped(-1), "Dry run: -1 steps (capped)");
        assert_eq!(capped(2), "Dry run: +2 steps (capped)");
        assert!(translators::set_locale("ja"), "ja.po was not compiled in");
        assert_eq!(capped(-1), "ドライラン: -1 ステップ（上限で制限）");
        let _ = translators::set_locale("en");
    }

    #[test]
    fn pt_br_catalogue_translates_and_names_itself() {
        let _guard = locale_lock();
        assert!(
            translators::set_locale("pt_BR"),
            "pt_BR.po was not compiled in"
        );
        assert_eq!(tr!("System default"), "Padrão do sistema");
        assert_eq!(native_name("pt_BR"), "Português (Brasil)");
        assert_eq!(language_name_msgid(), "Português (Brasil)");
        let _ = translators::set_locale("en");
        assert_eq!(tr!("System default"), "System default");
    }

    /// Every shipped catalogue: no fuzzy flag (`include-po` would compile it
    /// as a finished translation) and the same placeholders as the msgid.
    /// A missing entry is fine; an empty msgstr shows English.
    #[test]
    fn catalogues_are_ready_to_ship() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("po"))
            .collect();
        files.sort();
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy();
            for entry in po_entries(&text) {
                assert!(
                    !entry.fuzzy,
                    "{name} marks an entry fuzzy:\n{}",
                    entry.msgid
                );
                if entry.msgid.is_empty() {
                    continue;
                }
                let plural = entry.msgid_plural.as_deref();
                for (index, msgstr) in &entry.msgstrs {
                    if msgstr.is_empty() {
                        continue;
                    }
                    let source = if *index == 0 {
                        entry.msgid.as_str()
                    } else {
                        plural.unwrap_or(entry.msgid.as_str())
                    };
                    assert_eq!(
                        placeholders(msgstr),
                        placeholders(source),
                        "{name} placeholders differ for {source:?} -> {msgstr:?}"
                    );
                }
            }
        }
    }

    struct PoEntry {
        fuzzy: bool,
        msgid: String,
        msgid_plural: Option<String>,
        msgstrs: Vec<(usize, String)>,
    }

    fn po_entries(text: &str) -> Vec<PoEntry> {
        let mut entries = Vec::new();
        let mut fuzzy = false;
        let mut key: Option<String> = None;
        let mut buf = String::new();
        let mut msgid = String::new();
        let mut msgid_plural = None;
        let mut msgstrs: Vec<(usize, String)> = Vec::new();
        let mut saw = false;

        let flush_key = |key: &mut Option<String>,
                         buf: &mut String,
                         msgid: &mut String,
                         msgid_plural: &mut Option<String>,
                         msgstrs: &mut Vec<(usize, String)>| {
            let Some(key) = key.take() else {
                return;
            };
            let value = std::mem::take(buf);
            if key == "msgid" {
                *msgid = value;
            } else if key == "msgid_plural" {
                *msgid_plural = Some(value);
            } else if key == "msgstr" {
                msgstrs.push((0, value));
            } else if let Some(index) = key
                .strip_prefix("msgstr[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|index| index.parse().ok())
            {
                msgstrs.push((index, value));
            }
        };

        let flush_entry = |entries: &mut Vec<PoEntry>,
                           fuzzy: &mut bool,
                           msgid: &mut String,
                           msgid_plural: &mut Option<String>,
                           msgstrs: &mut Vec<(usize, String)>,
                           saw: &mut bool| {
            if !*saw {
                return;
            }
            entries.push(PoEntry {
                fuzzy: *fuzzy,
                msgid: std::mem::take(msgid),
                msgid_plural: msgid_plural.take(),
                msgstrs: std::mem::take(msgstrs),
            });
            *fuzzy = false;
            *saw = false;
        };

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                flush_key(
                    &mut key,
                    &mut buf,
                    &mut msgid,
                    &mut msgid_plural,
                    &mut msgstrs,
                );
                flush_entry(
                    &mut entries,
                    &mut fuzzy,
                    &mut msgid,
                    &mut msgid_plural,
                    &mut msgstrs,
                    &mut saw,
                );
                continue;
            }
            if trimmed.starts_with("#~") {
                continue;
            }
            if trimmed.starts_with('#') {
                if is_fuzzy_flag(trimmed) {
                    fuzzy = true;
                }
                continue;
            }
            if trimmed.starts_with('"') {
                buf.push_str(unquote(trimmed));
                continue;
            }
            flush_key(
                &mut key,
                &mut buf,
                &mut msgid,
                &mut msgid_plural,
                &mut msgstrs,
            );
            let (name, value) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
            key = Some(name.to_string());
            buf = unquote(value.trim()).to_string();
            saw = true;
        }
        flush_key(
            &mut key,
            &mut buf,
            &mut msgid,
            &mut msgid_plural,
            &mut msgstrs,
        );
        flush_entry(
            &mut entries,
            &mut fuzzy,
            &mut msgid,
            &mut msgid_plural,
            &mut msgstrs,
            &mut saw,
        );
        entries
    }

    fn is_fuzzy_flag(line: &str) -> bool {
        let Some(rest) = line.trim().strip_prefix("#,") else {
            return false;
        };
        rest.split(',').any(|flag| flag.trim() == "fuzzy")
    }

    fn unquote(line: &str) -> &str {
        line.strip_prefix('"')
            .and_then(|line| line.strip_suffix('"'))
            .unwrap_or(line)
    }

    fn placeholders(text: &str) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' && chars.get(i + 1) == Some(&'{') {
                i += 2;
                continue;
            }
            if chars[i] == '}' && chars.get(i + 1) == Some(&'}') {
                i += 2;
                continue;
            }
            if chars[i] == '{' {
                if let Some(end) = chars[i + 1..].iter().position(|c| *c == '}') {
                    let name: String = chars[i + 1..i + 1 + end].iter().collect();
                    *counts.entry(name).or_insert(0) += 1;
                    i += end + 2;
                    continue;
                }
            }
            i += 1;
        }
        counts
    }
}
