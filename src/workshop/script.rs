//! Port of `BfmeWorkshopScriptManager`: the tiny `package.wps` language used by scripted mods.
//!
//! ```text
//! let <name> be <value>
//! let <name> be <selector> from <HKLM\...|C:\file.ini|C:\file.json>
//! require "<description> <url>" if <lhs> equals|!equals|contains|!contains <rhs>
//! files be from <directory>
//! print <expr>
//! ```

use anyhow::{Result, anyhow};
use indexmap::IndexMap;

use super::model::Entry;
use crate::{bfme, http, util};

#[derive(Debug)]
pub struct ScriptSyntaxError(pub String);

impl std::fmt::Display for ScriptSyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ScriptSyntaxError {}

pub struct ScriptArtifacts {
    /// requirement name -> satisfied
    pub requirements: IndexMap<String, bool>,
    pub files_directory: String,
}

/// Runs the package script if the entry is a scripted mod. May drop files from `entry`
/// (the `files be from` command), just like the original mutated the shared list.
pub fn run_if_scripted(entry: &mut Entry) -> Result<ScriptArtifacts> {
    let mut out = ScriptArtifacts { requirements: IndexMap::new(), files_directory: String::new() };
    let Some(script_file) = entry.files.iter().find(|f| f.name == "package.wps") else {
        return Ok(out);
    };
    if entry.r#type != 1 {
        return Ok(out);
    }
    let script = http::get_string(&script_file.url, &[])?;
    run_script(&script, entry, &mut out)?;
    Ok(out)
}

fn run_script(script: &str, entry: &mut Entry, out: &mut ScriptArtifacts) -> Result<()> {
    let mut variables: IndexMap<String, String> = IndexMap::new();

    for (idx, line) in script.replace('\r', "").split('\n').enumerate() {
        let line_no = idx + 1;
        if line.starts_with("//") {
            continue;
        }

        let syn = |msg: &str| -> anyhow::Error {
            ScriptSyntaxError(format!("Syntax error on line {line_no}. {msg}")).into()
        };

        // Tokenizer (faithful to the original char-by-char behaviour).
        let mut tokens: Vec<String> = vec![];
        let mut token = String::new();
        for c in line.chars() {
            token.push(c);
            if token.chars().count() <= 1 {
                continue;
            }
            if token.starts_with('"') {
                if c == '"' {
                    for (k, v) in &variables {
                        token = token.replace(&format!("{{{k}}}"), v);
                    }
                    tokens.push(token.trim_matches('"').to_string());
                    token.clear();
                }
            } else if c == ' ' {
                let t = token.trim_matches(' ');
                if !t.is_empty() {
                    tokens.push(t.to_string());
                }
                token.clear();
            }
        }
        if token.starts_with('"') {
            return Err(syn("String terminator expected."));
        } else if !token.trim_matches(' ').is_empty() {
            tokens.push(token.trim_matches(' ').to_string());
        }
        if tokens.is_empty() {
            continue;
        }

        let verify = |i: usize, expected: &str| -> Result<String> {
            tokens.get(i).cloned().ok_or_else(|| syn(&format!("Expected '{expected}' missing.")))
        };
        let keyword = |i: usize, kw: &str| -> Result<()> {
            if tokens.get(i).map(String::as_str) != Some(kw) {
                return Err(syn(&format!("Expected '{kw}' keyword missing.")));
            }
            Ok(())
        };

        match tokens[0].as_str() {
            "require" => {
                let name = verify(1, "requirement name")?;
                keyword(2, "if")?;
                let lhs = verify(3, "requirement comparison left hand side expression")?;
                let cond = verify(4, "requirement comparison mode")?;
                let rhs = verify(5, "requirement comparison right hand side expression")?;
                let result = match cond.as_str() {
                    "equals" => lhs == rhs,
                    "!equals" => lhs != rhs,
                    "contains" => lhs.contains(&rhs),
                    "!contains" => !lhs.contains(&rhs),
                    _ => return Err(syn("Invalid requirement comparison mode.")),
                };
                if out.requirements.contains_key(&name) {
                    return Err(anyhow!("An item with the same key has already been added. Key: {name}"));
                }
                let all_ok = !out.requirements.values().any(|v| !v);
                out.requirements.insert(name, !result && all_ok);
            }
            "files" => {
                keyword(1, "be")?;
                keyword(2, "from")?;
                out.files_directory = verify(3, "files source expression")?;
                entry.files.retain(|f| f.name == "package.wps");
            }
            "let" => {
                let name = verify(1, "variable name")?;
                keyword(2, "be")?;
                let value = if tokens.len() > 4 {
                    let selector = verify(3, "variable value expression key selector")?;
                    keyword(4, "from")?;
                    let source = verify(5, "variable value expression source selector")?;
                    read_variable_source(&selector, &source).unwrap_or_default()
                } else {
                    verify(3, "variable value expression")?
                };
                if variables.contains_key(&name) {
                    return Err(anyhow!("An item with the same key has already been added. Key: {name}"));
                }
                variables.insert(name, value);
            }
            "print" => {
                let expr = verify(1, "print expression")?;
                eprintln!("PRINT: '{expr}'");
            }
            _ => {}
        }
    }
    Ok(())
}

fn registry_path(source: &str) -> String {
    source.replacen(r"HKLM\", "", 1).replace(r"SOFTWARE\", r"SOFTWARE\WOW6432Node\")
}

fn read_variable_source(selector: &str, source: &str) -> Option<String> {
    if selector == "all" {
        if source.starts_with(r"HKLM\") {
            let values = bfme::hklm_values(&registry_path(source))?;
            return Some(values.iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join("\n"));
        } else if source.contains(r":\") {
            return Some(util::read_text(source, ""));
        }
        return Some(String::new());
    }

    if source.starts_with(r"HKLM\") {
        return bfme::hklm_string(&registry_path(source), selector);
    } else if source.contains(r":\") {
        let text = util::read_text(source, "");
        return match util::extension(source) {
            ".ini" => {
                let mut map = IndexMap::new();
                for l in text.split('\n').filter(|l| l.contains('=')) {
                    let mut parts = l.split('=');
                    let k = parts.next().unwrap_or("").trim_matches(' ').to_string();
                    let v = parts.next().unwrap_or("").trim_matches(' ').to_string();
                    if map.insert(k, v).is_some() {
                        return None; // ToDictionary would throw on duplicates
                    }
                }
                map.get(selector).map(|v: &String| v.replace(['\r', '\n'], ""))
            }
            ".json" => {
                let v: serde_json::Value = serde_json::from_str(&text).ok()?;
                v.get(selector)?.as_str().map(str::to_string)
            }
            _ => Some(text),
        };
    }
    Some(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workshop::model::WorkshopFile;

    fn run(script: &str) -> Result<(ScriptArtifacts, Entry)> {
        let mut entry = Entry {
            r#type: 1,
            files: vec![
                WorkshopFile { name: "package.wps".into(), ..Default::default() },
                WorkshopFile { name: "data.big".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let mut out = ScriptArtifacts { requirements: IndexMap::new(), files_directory: String::new() };
        run_script(script, &mut entry, &mut out)?;
        Ok((out, entry))
    }

    #[test]
    fn requirements_and_variables() {
        let (out, entry) = run(
            "// comment\nlet ver be \"1.06\"\nrequire \"Install patch {ver} <https://x.y>\" if \"{ver}\" !equals \"1.06\"\nrequire \"Other\" if a equals b\nfiles be from \"C:/Mods/X\"",
        )
        .unwrap();
        assert_eq!(out.requirements.get("Install patch 1.06 <https://x.y>"), Some(&true));
        assert_eq!(out.requirements.get("Other"), Some(&true));
        assert_eq!(out.files_directory, "C:/Mods/X");
        assert_eq!(entry.files.len(), 1);
    }

    #[test]
    fn failing_requirement_blocks_following() {
        let (out, _) = run("require \"A\" if x equals x\nrequire \"B\" if x equals y").unwrap();
        assert_eq!(out.requirements.get("A"), Some(&false));
        assert_eq!(out.requirements.get("B"), Some(&false));
    }

    #[test]
    fn syntax_errors() {
        let e = run("require \"A\" if x\n").err().unwrap();
        assert!(e.to_string().contains("line 1"));
        assert!(run("let x be \"unterminated").is_err());
        assert!(run("require A if x maybe y").is_err());
    }
}
