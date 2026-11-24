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

    /// Whether a tab can be closed from the bar: only with two tabs or more. The bar always
    /// shows, and a single tab has no close button: closing it is closing the window (A4).
    pub fn closable(&self) -> bool {
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

    /// The tab at `index`, to change it (its panes, #33).
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.tabs.get_mut(index)
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

    /// Moves the tab at `from` to the place of the tab at `to` (past the end: to the end), as
    /// dragging it onto that tab does. The active tab stays the same one.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        let to = to.min(self.tabs.len() - 1);
        if from == to {
            return;
        }
        let active = self.active;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
    }

    /// The indices of the tabs a Close item of the tab menu closes, in order (telaio's
    /// [`telaio_ui::components::close_targets`], with no unsaved tabs).
    pub fn close_targets(
        &self,
        index: usize,
        scope: telaio_ui::components::CloseScope,
    ) -> Vec<usize> {
        telaio_ui::components::close_targets(&vec![false; self.tabs.len()], index, scope)
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

/// A path for the tab's tooltip: under `~` when it is in the home folder (design A5).
pub fn home_path(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// The folder of an OSC 7 report, as a path: `file://host/path` (percent encoded), or
/// `kitty-shell-cwd://host/path` (raw, as Ghostty's shell integration sends it). Other forms
/// are taken as a plain path.
pub fn pwd_path(report: &str) -> std::path::PathBuf {
    // Skip the host: the path starts at the first slash after it.
    let path_of = |rest: &str| {
        rest.find('/')
            .map_or(String::new(), |slash| rest[slash..].to_string())
    };
    if let Some(rest) = report.strip_prefix("kitty-shell-cwd://") {
        return std::path::PathBuf::from(path_of(rest));
    }
    if let Some(rest) = report.strip_prefix("file://") {
        return std::path::PathBuf::from(percent_decode(&path_of(rest)));
    }
    std::path::PathBuf::from(report)
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
    fn a_tab_can_be_changed_in_place() {
        let mut tabs = tabs(&["a", "b"]);
        if let Some(tab) = tabs.get_mut(0) {
            *tab = "z";
        }
        assert_eq!(names(&tabs), vec!["z", "b"]);
        assert!(tabs.get_mut(5).is_none());
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
    fn moving_a_tab_keeps_the_active_one() {
        let mut list = tabs(&["a", "b", "c", "d"]);
        list.activate(1);
        list.move_tab(0, 2);
        assert_eq!(names(&list), vec!["b", "c", "a", "d"]);
        assert_eq!(list.active(), Some(&"b"));
        list.move_tab(3, 0);
        assert_eq!(names(&list), vec!["d", "b", "c", "a"]);
        assert_eq!(list.active(), Some(&"b"));
        // The dragged tab is the active one; past the end means the end.
        list.move_tab(1, usize::MAX);
        assert_eq!(names(&list), vec!["d", "c", "a", "b"]);
        assert_eq!(list.active(), Some(&"b"));
        list.move_tab(9, 0);
        assert_eq!(names(&list), vec!["d", "c", "a", "b"]);
    }

    #[test]
    fn the_close_items_name_their_tabs() {
        use telaio_ui::components::CloseScope;
        let list = tabs(&["a", "b", "c"]);
        assert_eq!(list.close_targets(1, CloseScope::Others), vec![0, 2]);
        assert_eq!(list.close_targets(1, CloseScope::Right), vec![2]);
        assert_eq!(list.close_targets(1, CloseScope::Left), vec![0]);
        assert_eq!(list.close_targets(1, CloseScope::All), vec![0, 1, 2]);
    }

    #[test]
    fn a_single_tab_is_not_closed_from_the_bar() {
        assert!(!tabs(&["a"]).closable());
        assert!(tabs(&["a", "b"]).closable());
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
    fn tooltip_paths_start_with_a_tilde_under_home() {
        let home = Path::new("/home/me");
        assert_eq!(home_path(home, Some(home)), "~");
        assert_eq!(
            home_path(Path::new("/home/me/dev/terminal"), Some(home)),
            format!("~{}dev/terminal", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(home_path(Path::new("/etc"), Some(home)), "/etc");
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
        assert_eq!(
            pwd_path("kitty-shell-cwd://thinkpad/home/me/dev/my%20dir"),
            Path::new("/home/me/dev/my%20dir")
        );
    }
}
