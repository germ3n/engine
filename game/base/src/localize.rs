use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const DEFAULT_LANGUAGE: &str = "english";

struct State {
    language: String,
    current: HashMap<String, String>,
    fallback: HashMap<String, String>,
    custom: HashMap<String, HashMap<String, String>>,
}

fn state() -> &'static RwLock<State> {
    static STATE: OnceLock<RwLock<State>> = OnceLock::new();

    STATE.get_or_init(|| {
        let fallback = load_language(DEFAULT_LANGUAGE);

        RwLock::new(State {
            language: DEFAULT_LANGUAGE.to_string(),
            current: fallback.clone(),
            fallback,
            custom: HashMap::new(),
        })
    })
}

fn load_language(language: &str) -> HashMap<String, String> {
    let path = format!("lang/{language}.txt");
    let mut table = HashMap::new();

    match crate::fs::try_global() {
        Some(fs) if fs.exists(&path) => match fs.read_string(&path) {
            Ok(text) => parse(&text, &mut table),
            Err(err) => log::warn!("[localize] {err}"),
        },
        _ => log::warn!("[localize] no language file {path}"),
    }

    table
}

pub fn parse(text: &str, table: &mut HashMap<String, String>) {
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();

        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_start_matches('#');

        if key.is_empty() {
            continue;
        }

        table.insert(key.to_string(), unescape(value.trim()));
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);

            continue;
        }

        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }

    out
}

/// Replaces `{:N}` with `args[N - 1]`. Placeholders without a matching argument are kept as written.
pub fn format(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{:") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();

        if digits > 0 && tail[digits..].starts_with('}') {
            let arg = tail[..digits]
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|idx| args.get(idx));

            match arg {
                Some(value) => out.push_str(value),
                None => out.push_str(&rest[start..start + 3 + digits]),
            }

            rest = &tail[digits + 1..];
        } else {
            out.push_str("{:");
            rest = tail;
        }
    }

    out.push_str(rest);

    out
}

/// Resolves `#Token` and formats it. Text that does not start with `#` is returned unchanged
/// (no formatting). Unknown tokens come back as written.
pub fn get(text: &str, args: &[String]) -> String {
    let Some(token) = text.strip_prefix('#') else {
        return text.to_string();
    };
    let state = state().read().unwrap();

    let custom = |language: &str| state.custom.get(language).and_then(|table| table.get(token));
    let found = custom(&state.language)
        .or_else(|| state.current.get(token))
        .or_else(|| custom(DEFAULT_LANGUAGE))
        .or_else(|| state.fallback.get(token));

    match found {
        Some(template) => format(template, args),
        None => text.to_string(),
    }
}

/// Registers a string for `language`. It replaces a lang file string with the same token.
pub fn add(language: &str, token: &str, text: &str) {
    let token = token.trim_start_matches('#');

    if token.is_empty() {
        return;
    }

    state()
        .write()
        .unwrap()
        .custom
        .entry(language.to_string())
        .or_default()
        .insert(token.to_string(), text.to_string());
}

pub fn set_language(language: &str) {
    let current = if language == DEFAULT_LANGUAGE {
        None
    } else {
        Some(load_language(language))
    };
    let mut state = state().write().unwrap();

    state.language = language.to_string();
    state.current = current.unwrap_or_else(|| state.fallback.clone());
}

pub fn language() -> String {
    state().read().unwrap().language.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn format_uses_one_based_indices() {
        assert_eq!(format("{:2} then {:1}", &args(&["a", "b"])), "b then a");
        assert_eq!(format("{:1}{:1}", &args(&["x"])), "xx");
    }

    #[test]
    fn format_keeps_unmatched_placeholders() {
        assert_eq!(format("{:3} {:0} {:} {:x", &args(&["a"])), "{:3} {:0} {:} {:x");
    }

    #[test]
    fn custom_strings_override_files_and_fall_back_to_english() {
        add("english", "#Custom_Hi", "Hi {:1}");
        add("klingon", "Custom_Hi", "nuqneH {:1}");
        assert_eq!(get("#Custom_Hi", &args(&["a"])), "Hi a");
        set_language("klingon");
        assert_eq!(get("#Custom_Hi", &args(&["a"])), "nuqneH a");
        add("english", "Only_English", "yes");
        assert_eq!(get("#Only_English", &[]), "yes");
        set_language("english");
    }

    #[test]
    fn parse_reads_tokens_comments_and_escapes() {
        let mut table = HashMap::new();
        parse("// c\n#Bla_bla = Hi {:1}\\nthere\nbad line\n", &mut table);
        assert_eq!(table.get("Bla_bla").unwrap(), "Hi {:1}\nthere");
        assert_eq!(table.len(), 1);
    }
}
