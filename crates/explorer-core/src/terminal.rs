//! Terminal emulator for desktop entries marked `Terminal=true` (the app the
//! user launches needs one). Linux Explorer itself uses its built-in panel.


use crate::apps::which;

/// Terminal programs and the flag that sets the working directory
/// (None = rely on the process cwd).
const TERMINALS: &[(&str, Option<&str>)] = &[
    ("ptyxis", Some("--working-directory=")), // Fedora 41+
    ("kgx", Some("--working-directory=")),
    ("gnome-terminal", Some("--working-directory=")),
    ("konsole", Some("--workdir=")),
    ("xfce4-terminal", Some("--working-directory=")),
    ("tilix", Some("--working-directory=")),
    ("mate-terminal", Some("--working-directory=")),
    ("alacritty", Some("--working-directory=")),
    ("kitty", Some("--directory=")),
    ("wezterm", None),
    ("foot", None),
    ("x-terminal-emulator", None),
    ("xterm", None),
];

pub fn detect() -> Option<&'static str> {
    if let Ok(t) = std::env::var("TERMINAL") {
        if let Some(found) = TERMINALS.iter().find(|(n, _)| *n == t) {
            return Some(found.0);
        }
    }
    TERMINALS.iter().find(|(n, _)| which(n).is_some()).map(|(n, _)| *n)
}

/// For desktop entries with Terminal=true.
pub fn wrap_in_terminal(argv: Vec<String>) -> Vec<String> {
    let Some(name) = detect() else { return argv };
    let mut v = vec![name.to_string()];
    match name {
        "gnome-terminal" | "ptyxis" | "kgx" | "tilix" => v.push("--".into()),
        "wezterm" => v.push("start".into()),
        _ => v.push("-e".into()),
    }
    v.extend(argv);
    v
}
