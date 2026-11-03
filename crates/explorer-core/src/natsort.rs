//! Natural ("logical") string ordering, matching Explorer's `StrCmpLogicalW`:
//! case-insensitive, and runs of digits compare by numeric value, so
//! `file2` sorts before `file10`.

use std::cmp::Ordering;

pub fn compare(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return tie_break(a, b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_digits(&mut ai);
                    let nb = take_digits(&mut bi);
                    let ord = cmp_digit_runs(&na, &nb);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                } else {
                    let la = fold(ca);
                    let lb = fold(cb);
                    if la != lb {
                        return la.cmp(&lb);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(&c) = it.peek() {
        if c.is_ascii_digit() {
            s.push(c);
            it.next();
        } else {
            break;
        }
    }
    s
}

fn cmp_digit_runs(a: &str, b: &str) -> Ordering {
    let ta = a.trim_start_matches('0');
    let tb = b.trim_start_matches('0');
    ta.len()
        .cmp(&tb.len())
        .then_with(|| ta.cmp(tb))
        // "01" after "1", like Explorer.
        .then_with(|| a.len().cmp(&b.len()))
}

/// Punctuation sorts before digits, digits before letters (Explorer order).
fn fold(c: char) -> (u8, String) {
    let class = if c.is_alphabetic() {
        2
    } else if c.is_ascii_digit() {
        1
    } else {
        0
    };
    (class, c.to_lowercase().collect())
}

fn tie_break(a: &str, b: &str) -> Ordering {
    // Equal ignoring case: make ordering total and deterministic.
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| compare(a, b));
        v
    }

    #[test]
    fn numbers_compare_by_value() {
        assert_eq!(
            sorted(vec!["file10.txt", "file2.txt", "file1.txt", "file20.txt"]),
            vec!["file1.txt", "file2.txt", "file10.txt", "file20.txt"]
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(sorted(vec!["beta", "Alpha", "alpha2", "Gamma"]), vec!["Alpha", "alpha2", "beta", "Gamma"]);
    }

    #[test]
    fn punctuation_and_digits_before_letters() {
        assert_eq!(sorted(vec!["b", "1a", "_x", "a"]), vec!["_x", "1a", "a", "b"]);
    }

    #[test]
    fn leading_zeros() {
        assert_eq!(compare("img001", "img1"), Ordering::Greater);
        assert_eq!(compare("img002", "img10"), Ordering::Less);
    }

    #[test]
    fn prefix_shorter_first() {
        assert_eq!(compare("New folder", "New folder (2)"), Ordering::Less);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(compare("a99999999999999999999999", "a100000000000000000000000"), Ordering::Less);
    }
}
