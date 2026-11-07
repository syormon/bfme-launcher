//! Per-game launch options in the format Steam uses:
//!
//! ```text
//! PROTON_LOG=1 WINEDLLOVERRIDES="d3d9=n,b" gamemoderun %command% -nologo
//! ```
//!
//! Leading `NAME=value` words are environment variables, other words before `%command%` wrap the
//! game (`gamemoderun`, `mangohud`, ...), and words after it are extra game arguments. Without
//! `%command%`, everything that isn't a variable is an extra game argument.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchOptions {
    pub env: Vec<(String, String)>,
    pub wrapper: Vec<String>,
    pub args: Vec<String>,
}

pub fn parse(text: &str) -> LaunchOptions {
    let words = split_words(text);
    let mut out = LaunchOptions::default();
    let command_at = words.iter().position(|w| w == "%command%");
    let (before, after) = match command_at {
        Some(i) => (&words[..i], &words[i + 1..]),
        None => (&words[..], &[][..]),
    };
    let mut in_env = true;
    for w in before {
        if in_env && let Some((k, v)) = env_assignment(w) {
            out.env.push((k, v));
            continue;
        }
        in_env = false;
        if command_at.is_some() {
            out.wrapper.push(w.clone());
        } else {
            out.args.push(w.clone());
        }
    }
    out.args.extend(after.iter().cloned());
    out
}

fn env_assignment(word: &str) -> Option<(String, String)> {
    let (k, v) = word.split_once('=')?;
    let valid = !k.is_empty() && !k.starts_with(|c: char| c.is_ascii_digit()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then(|| (k.to_string(), v.to_string()))
}

/// Splits like a shell would for simple cases: whitespace separates words, single and double quotes
/// group (and are removed), a backslash escapes the next character outside single quotes.
fn split_words(text: &str) -> Vec<String> {
    let mut words = vec![];
    let mut cur = String::new();
    let mut has_word = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => cur.push(c),
            (_, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                has_word = true;
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if has_word || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                has_word = false;
            }
            (None, c) => {
                cur.push(c);
                has_word = true;
            }
        }
    }
    if has_word || !cur.is_empty() {
        words.push(cur);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn steam_style() {
        let o = parse(r#"PROTON_LOG=1 WINEDLLOVERRIDES="d3d9=n,b" gamemoderun %command% -nologo -x "two words""#);
        assert_eq!(o.env, vec![("PROTON_LOG".into(), "1".into()), ("WINEDLLOVERRIDES".into(), "d3d9=n,b".into())]);
        assert_eq!(o.wrapper, s(&["gamemoderun"]));
        assert_eq!(o.args, s(&["-nologo", "-x", "two words"]));
    }

    #[test]
    fn without_command_everything_else_is_an_argument() {
        let o = parse("DXVK_HUD=fps -nologo -xres 1920");
        assert_eq!(o.env, vec![("DXVK_HUD".into(), "fps".into())]);
        assert!(o.wrapper.is_empty());
        assert_eq!(o.args, s(&["-nologo", "-xres", "1920"]));
    }

    #[test]
    fn empty_and_odd_input() {
        assert_eq!(parse("   "), LaunchOptions::default());
        // `=` inside an argument after the variables is not a variable.
        let o = parse("-mod=foo A=1");
        assert!(o.env.is_empty());
        assert_eq!(o.args, s(&["-mod=foo", "A=1"]));
        assert_eq!(parse("%command%"), LaunchOptions::default());
        assert_eq!(parse("''").args, s(&[""]));
    }
}
