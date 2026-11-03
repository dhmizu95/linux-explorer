//! Rename many items at once (Dolphin's multi-rename and Explorer's
//! "name (1), name (2)" behaviour).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Case {
    Keep,
    Lower,
    Upper,
    Title,
}

#[derive(Debug, Clone)]
pub enum Rule {
    /// `#` is replaced by a number padded to the count of `#`s ("Photo ###" →
    /// "Photo 001"). Without `#`, Explorer style " (n)" is appended.
    Pattern { pattern: String, start: u64 },
    /// Plain text find and replace in the stem.
    Replace { find: String, replace: String, case_sensitive: bool },
    Case(Case),
}

fn split(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir {
        return (name, "");
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = true;
    for c in s.chars() {
        if up && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.extend(c.to_lowercase());
        }
        if c.is_whitespace() || c == '-' || c == '_' {
            up = true;
        }
    }
    out
}

/// Compute new names (extensions are kept) in the given order.
pub fn plan(paths: &[PathBuf], rule: &Rule) -> Vec<(PathBuf, PathBuf)> {
    paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let (stem, ext) = split(&name, p.is_dir());
            let new_stem = match rule {
                Rule::Pattern { pattern, start } => {
                    let n = start + i as u64;
                    if let Some(first) = pattern.find('#') {
                        let run = pattern[first..].chars().take_while(|c| *c == '#').count();
                        format!("{}{:0width$}{}", &pattern[..first], n, &pattern[first + run..], width = run)
                    } else {
                        format!("{pattern} ({n})")
                    }
                }
                Rule::Replace { find, replace, case_sensitive } => {
                    if find.is_empty() {
                        stem.to_string()
                    } else if *case_sensitive {
                        stem.replace(find.as_str(), replace)
                    } else {
                        replace_ci(stem, find, replace)
                    }
                }
                Rule::Case(c) => match c {
                    Case::Keep => stem.to_string(),
                    Case::Lower => stem.to_lowercase(),
                    Case::Upper => stem.to_uppercase(),
                    Case::Title => title(stem),
                },
            };
            (p.clone(), p.with_file_name(format!("{new_stem}{ext}")))
        })
        .collect()
}

fn replace_ci(s: &str, find: &str, rep: &str) -> String {
    let lower = s.to_lowercase();
    let f = find.to_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&f) {
        out.push_str(&s[i..i + pos]);
        out.push_str(rep);
        i += pos + f.len();
    }
    out.push_str(&s[i..]);
    out
}

/// Problems that make a plan unsafe to apply.
pub fn validate(plan: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let mut targets = HashSet::new();
    let sources: HashSet<&Path> = plan.iter().map(|(s, _)| s.as_path()).collect();
    for (src, dst) in plan {
        let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.trim().is_empty() || name.contains('/') {
            return Err(format!("Invalid name for {}", src.display()));
        }
        if !targets.insert(dst.clone()) {
            return Err(format!("Two items would be named \"{name}\"."));
        }
        if src != dst && fs::symlink_metadata(dst).is_ok() && !sources.contains(dst.as_path()) {
            return Err(format!("\"{name}\" already exists."));
        }
    }
    Ok(())
}

/// Apply via temporary names so swaps (a→b, b→a) work. Returns done pairs.
pub fn apply(plan: &[(PathBuf, PathBuf)]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    validate(plan)?;
    let changes: Vec<&(PathBuf, PathBuf)> = plan.iter().filter(|(s, d)| s != d).collect();
    let mut temps = Vec::new();
    for (i, (src, _)) in changes.iter().enumerate() {
        let tmp = src.with_file_name(format!(".le-rename-{}-{i}", std::process::id()));
        fs::rename(src, &tmp).map_err(|e| format!("{}: {e}", src.display()))?;
        temps.push(tmp);
    }
    let mut done = Vec::new();
    for ((src, dst), tmp) in changes.iter().map(|p| (&p.0, &p.1)).zip(temps) {
        fs::rename(&tmp, dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        done.push((src.clone(), dst.clone()));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[(PathBuf, PathBuf)]) -> Vec<String> {
        v.iter().map(|(_, d)| d.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn patterns() {
        let t = tempfile::tempdir().unwrap();
        let ps: Vec<PathBuf> = ["a.jpg", "b.jpg", "c.png"].iter().map(|n| t.path().join(n)).collect();
        for p in &ps {
            fs::write(p, b"").unwrap();
        }
        let p = plan(&ps, &Rule::Pattern { pattern: "Holiday ###".into(), start: 1 });
        assert_eq!(names(&p), vec!["Holiday 001.jpg", "Holiday 002.jpg", "Holiday 003.png"]);
        let p = plan(&ps, &Rule::Pattern { pattern: "Trip".into(), start: 1 });
        assert_eq!(names(&p), vec!["Trip (1).jpg", "Trip (2).jpg", "Trip (3).png"]);
        apply(&p).unwrap();
        assert!(t.path().join("Trip (2).jpg").exists() && !t.path().join("a.jpg").exists());
    }

    #[test]
    fn replace_case_and_swap() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("IMG_one.txt");
        let b = t.path().join("img_two.txt");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        let p = plan(&[a.clone(), b.clone()], &Rule::Replace { find: "img_".into(), replace: "Pic ".into(), case_sensitive: false });
        assert_eq!(names(&p), vec!["Pic one.txt", "Pic two.txt"]);
        let p = plan(&[a.clone()], &Rule::Case(Case::Title));
        assert_eq!(names(&p), vec!["Img_One.txt"]);
        // Swap names.
        let swap = vec![(a.clone(), b.clone()), (b.clone(), a.clone())];
        apply(&swap).unwrap();
        assert_eq!(fs::read(&a).unwrap(), b"2");
        assert_eq!(fs::read(&b).unwrap(), b"1");
    }

    #[test]
    fn rejects_collisions() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.txt");
        let b = t.path().join("b.txt");
        fs::write(&a, b"").unwrap();
        fs::write(&b, b"").unwrap();
        fs::write(t.path().join("taken.txt"), b"").unwrap();
        let p = plan(&[a.clone(), b.clone()], &Rule::Pattern { pattern: "same".into(), start: 1 });
        assert!(validate(&p).is_ok());
        let clash = vec![(a.clone(), t.path().join("x.txt")), (b.clone(), t.path().join("x.txt"))];
        assert!(validate(&clash).is_err());
        assert!(validate(&[(a, t.path().join("taken.txt"))]).is_err());
    }
}
