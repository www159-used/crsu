use std::collections::BTreeMap;

pub struct Defaults {
    pub url: String,
    pub user: String,
}

pub fn defaults() -> Defaults {
    let path = std::env::var_os("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".atlassian/crucible.conf"));
    let contents = path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    parse(&contents)
}

fn parse(contents: &str) -> Defaults {
    let mut sections = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut section = "DEFAULT".to_owned();
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            line[1..line.len() - 1].clone_into(&mut section);
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(section.clone())
                .or_default()
                .insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let default = sections.get("DEFAULT");
    let url = default
        .and_then(|values| values.get("url"))
        .cloned()
        .unwrap_or_else(|| "http://192.168.1.98:8060".to_owned());
    let user = sections
        .get(&url)
        .and_then(|values| values.get("user"))
        .cloned()
        .unwrap_or_default();
    Defaults { url, user }
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn reads_default_url_and_user() {
        let defaults = parse("[DEFAULT]\nurl = http://cru\n\n[http://cru]\nuser = ww\n");
        assert_eq!(defaults.url, "http://cru");
        assert_eq!(defaults.user, "ww");
    }
}
