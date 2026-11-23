//! The list of tabs and which one is active, as plain data: what opening, closing, moving
//! between and naming tabs does, tested without a window. Each tab holds a `T`, the view of its
//! terminal in the app.

/// The tabs of the window, in order, and the active one.
#[derive(Debug, Clone)]
pub struct Tabs<T> {
    tabs: Vec<T>,
    active: usize,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
        }
    }
}

impl<T> Tabs<T> {
    /// Whether there is no tab (the window closes then).
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Whether the tab bar shows: only with two tabs or more, as in the design (A2).
    pub fn shows_bar(&self) -> bool {
        self.tabs.len() > 1
    }

    /// The index of the active tab.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The active tab.
    pub fn active(&self) -> Option<&T> {
        self.tabs.get(self.active)
    }

    /// Every tab, in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.tabs.iter()
    }

    /// Adds a tab after the active one and activates it.
    pub fn open(&mut self, tab: T) {
        let at = if self.tabs.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.tabs.insert(at, tab);
        self.active = at;
    }

    /// Removes the tab at `index` and returns it. The active tab stays the same one when another
    /// tab closes; when the active one closes, the one that took its place (or the previous, at
    /// the end) becomes active.
    pub fn close(&mut self, index: usize) -> Option<T> {
        if index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        if index < self.active || (index == self.active && self.active >= self.tabs.len()) {
            self.active = self.active.saturating_sub(1);
        }
        Some(tab)
    }

    /// Activates the tab at `index`, if there is one.
    pub fn activate(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Activates the next tab, wrapping around.
    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Activates the previous tab, wrapping around.
    pub fn previous(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// The index of the first tab that matches.
    pub fn position(&self, matches: impl Fn(&T) -> bool) -> Option<usize> {
        self.tabs.iter().position(matches)
    }
}

/// The label of a tab: the title the program set, or else the shell and the folder it is in
/// (`zsh · postino`), as in the design.
pub fn tab_label(title: &str, shell: &str, folder: Option<&str>) -> String {
    let title = title.trim();
    // Windows names a console after the program's path (`C:\...\powershell.exe`): the
    // profile's name says it better.
    let program_path = title.to_ascii_lowercase().ends_with(".exe");
    if !title.is_empty() && !program_path {
        return title.to_string();
    }
    match folder.filter(|folder| !folder.is_empty()) {
        Some(folder) => format!("{shell} · {folder}"),
        None => shell.to_string(),
    }
}

/// The last component of a folder path, `~` for the home folder.
pub fn folder_name(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    if home.is_some_and(|home| home == path) {
        return "~".to_string();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The folder of an OSC 7 report (`file://host/path`), as a path. Other forms are taken as a
/// plain path.
pub fn pwd_path(report: &str) -> std::path::PathBuf {
    let rest = report.strip_prefix("file://").map(|rest| {
        // Skip the host: the path starts at the first slash after it.
        rest.find('/').map_or(rest, |slash| &rest[slash..])
    });
    std::path::PathBuf::from(percent_decode(rest.unwrap_or(report)))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn tabs(names: &[&'static str]) -> Tabs<&'static str> {
        let mut tabs = Tabs::default();
        for name in names {
            tabs.open(*name);
        }
        tabs
    }

    fn names(tabs: &Tabs<&'static str>) -> Vec<&'static str> {
        tabs.iter().copied().collect()
    }

    #[test]
    fn a_new_tab_opens_after_the_active_one_and_activates() {
        let mut list = tabs(&["a", "b", "c"]);
        list.activate(0);
        list.open("d");
        assert_eq!(names(&list), vec!["a", "d", "b", "c"]);
        assert_eq!(list.active(), Some(&"d"));
    }

    #[test]
    fn the_bar_shows_from_two_tabs() {
        assert!(!tabs(&["a"]).shows_bar());
        assert!(tabs(&["a", "b"]).shows_bar());
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut list = tabs(&["a", "b", "c"]);
        list.activate(2);
        assert_eq!(list.close(0), Some("a"));
        assert_eq!(list.active(), Some(&"c"));
    }

    #[test]
    fn closing_the_active_tab_activates_its_neighbour() {
        let mut list = tabs(&["a", "b", "c"]);
        list.activate(1);
        list.close(1);
        assert_eq!(list.active(), Some(&"c"));
        list.close(1);
        assert_eq!(list.active(), Some(&"a"));
        list.close(0);
        assert!(list.is_empty());
        assert_eq!(list.close(0), None);
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let mut list = tabs(&["a", "b", "c"]);
        list.next();
        assert_eq!(list.active(), Some(&"a"));
        list.previous();
        assert_eq!(list.active(), Some(&"c"));
    }

    #[test]
    fn the_label_is_the_title_or_the_shell_and_folder() {
        assert_eq!(tab_label("vim", "zsh", Some("postino")), "vim");
        assert_eq!(tab_label("  ", "zsh", Some("postino")), "zsh · postino");
        assert_eq!(tab_label("", "bash", None), "bash");
        assert_eq!(
            tab_label(r"C:\WINDOWS\System32\cmd.exe", "Símbolo del sistema", None),
            "Símbolo del sistema"
        );
    }

    #[test]
    fn folder_names_use_a_tilde_for_home() {
        let home = Path::new("/home/me");
        assert_eq!(folder_name(home, Some(home)), "~");
        assert_eq!(folder_name(Path::new("/home/me/dev"), Some(home)), "dev");
    }

    #[test]
    fn osc7_reports_become_paths() {
        assert_eq!(
            pwd_path("file://host/home/me/my%20dev"),
            Path::new("/home/me/my dev")
        );
        assert_eq!(pwd_path("/tmp"), Path::new("/tmp"));
    }
}
