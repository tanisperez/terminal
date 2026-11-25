//! Tanis Terminal (`tterm`). Binary entry point: reads the command line and the settings, starts
//! the log, and opens the window.

mod actions;
mod state;
mod theme;
mod views;

rust_i18n::i18n!("locales", fallback = "en");

use std::cell::Cell;
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::*;
use telaio_app::{logging, window};
use tterm_cli::IDENTITY;

use state::settings::LogLevel;
use state::window::WindowState;
use views::AppView;

/// The smallest window, in logical pixels: room for three tabs and the buttons of the bar.
const MIN_WINDOW_SIZE: (f32, f32) = (640.0, 400.0);
/// The size of the first window, before any size is saved: a comfortable 100 x 30 grid.
const FIRST_WINDOW_SIZE: (f32, f32) = (1120.0, 680.0);
/// The sizes of every window.
const WINDOW_SIZES: window::WindowSizes = window::WindowSizes {
    min: MIN_WINDOW_SIZE,
    default: FIRST_WINDOW_SIZE,
};

/// What a new window starts with.
pub enum FirstTabs {
    /// A tab per folder of the command line, or the default profile's without any.
    Folders(Vec<std::path::PathBuf>),
    /// A tab moved from another window, still running, with its panes (#34).
    Moved(views::Tab),
}

/// Opens a window of tterm with `first` in it, at `bounds` (the platform's place and the default
/// size when `None`). `saved` is the size saved by the last window closed, for the first one.
/// Returns whether it opened.
pub fn open_window(
    cx: &mut App,
    settings: state::settings::Settings,
    first: FirstTabs,
    bounds: Option<WindowBounds>,
    saved: Option<WindowState>,
) -> bool {
    let mut window_options = window::options(cx, &IDENTITY, WINDOW_SIZES);
    window_options.window_background = window_background(settings.background_opacity);
    if bounds.is_some() {
        window_options.window_bounds = bounds;
    }
    let theme_choice = settings.theme;
    cx.open_window(window_options, move |window, cx| {
        telaio_ui::ThemeChoice::from(theme_choice).apply(Some(window), cx);
        let view = cx.new(|cx| AppView::new(settings, first, window, cx));
        remember_window_size(saved, &view, window, cx);
        let weak = view.downgrade();
        window::follow_system_appearance(window, move |cx| {
            weak.upgrade()
                .is_some_and(|view| view.read(cx).follows_system_theme())
        });
        // After the theme follows the system (registered later, so it runs later), a
        // translucent window clears the surface the theme brought back (#31).
        view.update(cx, |_, cx| {
            cx.observe_window_appearance(window, |_, _, cx| theme::clear_surface(cx))
                .detach();
        });
        theme::clear_surface(cx);
        cx.new(|cx| Root::new(view, window, cx))
    })
    .map_err(|error| log::error!("could not open a window: {error}"))
    .is_ok()
}

fn main() -> ExitCode {
    // `--help`, `--version` and a wrong option print and exit before anything else.
    let dirs = match tterm_cli::start(std::env::args_os().skip(1)) {
        Ok(dirs) => dirs,
        Err(code) => return code,
    };
    if telaio_app::cli::detach_from_console() == telaio_app::cli::GuiLaunch::Relaunched {
        return ExitCode::SUCCESS;
    }

    // What's new, once after an update. The last version that ran is read before anything is
    // written (a first run has no files yet), and recorded before the window opens, so a crash
    // or a second window never shows it twice.
    let last_seen = state::config::last_seen_version();
    let show_whats_new = state::whats_new::opens_at_startup(
        state::whats_new::VERSION,
        &last_seen,
        state::whats_new::NOTES.is_some(),
    );

    // Settings first, for the log level: the logger is installed before anything else runs.
    let settings = state::settings::load_settings();
    let log_env_var = logging::env_var(&IDENTITY);
    let env_level = std::env::var(&log_env_var).ok();
    let log_level = logging::startup_level(settings.log_level, env_level.as_deref());
    let log_path = logging::init(&IDENTITY, log_level.to_filter());
    let from_env = env_level.as_deref().and_then(LogLevel::parse).is_some();
    log_startup(
        log_level,
        from_env.then_some(log_env_var.as_str()),
        log_path.as_deref(),
    );
    if let Some(value) = env_level.filter(|_| !from_env) {
        log::warn!("ignoring {log_env_var}={value:?}: not a log level");
    }

    if state::whats_new::records(state::whats_new::VERSION) {
        state::config::record_version(state::whats_new::VERSION);
    }
    if show_whats_new {
        log::info!("updated since {last_seen:?}, opening what's new");
        views::whats_new::open_at_startup();
    }

    let language = state::locale::resolve(settings.language);
    state::locale::apply(language);
    log::info!("language: {}", language.code());

    // Pasted images older than the setting go, off the main thread.
    let days = settings.pasted_images_days;
    std::thread::spawn(move || {
        if let Some(dir) = state::images::images_dir() {
            let removed = state::images::prune(&dir, days);
            if removed > 0 {
                log::info!("deleted {removed} pasted images older than {days} days");
            }
        }
    });

    let app = window::application();
    app.run(move |cx| {
        window::init(cx, &IDENTITY);
        actions::bind_keys(cx);
        views::bind_keys(cx);
        theme::install(cx, &settings);

        let saved_window = state::window::load();
        let bounds = saved_window.map(|saved| saved_window_bounds(&saved, WINDOW_SIZES, cx));
        cx.spawn(async move |cx| {
            let opened = cx.update(|cx| {
                open_window(cx, settings, FirstTabs::Folders(dirs), bounds, saved_window)
            });
            // Opening the very first window failing is not recoverable.
            #[allow(clippy::panic)]
            if !opened {
                panic!("failed to open the main window");
            }
        })
        .detach();
    });
    ExitCode::SUCCESS
}

/// Where a window with a saved size opens: centered, as the first time.
fn saved_window_bounds(saved: &WindowState, sizes: window::WindowSizes, cx: &App) -> WindowBounds {
    let visible = cx.primary_display().map(|display| display.visible_bounds());
    let (width, height) = saved.fitted(
        visible.map(|area| (f32::from(area.size.width), f32::from(area.size.height))),
        sizes.min,
    );
    let size = size(px(width), px(height));
    let bounds = match visible {
        Some(area) => Bounds::centered_at(area.center(), size),
        None => Bounds::new(point(px(0.0), px(0.0)), size),
    };
    if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

/// Keeps the window's last size in memory while it changes and saves it when the window closes.
/// Nothing runs while the window is not being resized.
fn remember_window_size(
    saved: Option<WindowState>,
    view: &Entity<AppView>,
    window: &mut Window,
    cx: &mut App,
) {
    // On X11 gpui asks to maximize before the window is mapped, and the request is lost.
    if saved.is_some_and(|saved| saved.maximized) && cx.compositor_name() == "X11" {
        window.on_next_frame(|window, _| {
            if !window.is_maximized() {
                window.zoom_window();
            }
        });
    }
    let wayland = cx.compositor_name() == "Wayland";
    let opened = reopening_size(wayland, window);
    let last = Rc::new(Cell::new(saved.unwrap_or(WindowState {
        width: opened.width.into(),
        height: opened.height.into(),
        maximized: false,
    })));
    let resized = last.clone();
    view.update(cx, |_, cx| {
        cx.observe_window_bounds(window, move |_, window, _| {
            let size = reopening_size(wayland, window);
            if !state::window::usable_size(size.width.into(), size.height.into()) {
                return;
            }
            let mut state = resized.get();
            state.maximized = window.is_maximized();
            if !state.maximized && !window.is_fullscreen() {
                state.width = size.width.into();
                state.height = size.height.into();
            }
            resized.set(state);
        })
        .detach();
    });
    // Every window hears every window close (#34): only its own close saves its size, so the
    // last window closed decides the size of the next start.
    let id = window.window_handle().window_id();
    cx.on_window_closed(move |_, closed| {
        if closed == id {
            state::window::save(&last.get());
        }
    })
    .detach();
}

/// The size that opens the window as it is now (see ted's `main.rs`: the shadow of the client
/// side decorations counts differently on Wayland and X11). A compositor that draws the
/// decorations itself, as the headless sway of the UI tests, makes gpui report a size that grows
/// on every run; the desktops tterm runs on (KDE, GNOME, Windows, macOS) do not.
fn reopening_size(wayland: bool, window: &Window) -> Size<Pixels> {
    if wayland {
        window.bounds().size
    } else {
        window.inner_window_bounds().get_bounds().size
    }
}

/// Logs where and how tterm runs: the first lines of every session.
fn log_startup(level: LogLevel, from_env: Option<&str>, log_path: Option<&Path>) {
    log::info!(
        "Tanis Terminal {} starting ({} build) on {} {}, libghostty-vt {}",
        tterm_cli::version_label(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
        tterm_term::ghostty_version(),
    );
    log::info!(
        "log level {level:?}{}, log file {}",
        from_env.map_or_else(String::new, |variable| format!(" (from {variable})")),
        log_path.map_or_else(|| "none".to_string(), |path| path.display().to_string()),
    );
    log::info!(
        "settings file {}",
        state::settings::settings_path()
            .map_or_else(|| "unknown".to_string(), |path| path.display().to_string()),
    );
}

/// The window's background for an opacity of the terminal (#31): see-through below 1.0.
pub fn window_background(opacity: f32) -> gpui_kit::WindowBackgroundAppearance {
    if opacity < 1.0 {
        gpui_kit::WindowBackgroundAppearance::Transparent
    } else {
        gpui_kit::WindowBackgroundAppearance::Opaque
    }
}
