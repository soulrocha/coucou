//! Out of the way of fullscreen video.
//!
//! While a fullscreen app runs — a video, a game, a presentation — the island,
//! Mochi on the desktop and the resting dictation pill hide, and come back as they were
//! when it ends. A dictation the user starts still shows its pill: they asked
//! for it. Windows says when something is fullscreen
//! (SHQueryUserNotificationState); one cheap call every second and a half.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// A fullscreen app is in front.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Mochi was on the desktop when fullscreen began, so he comes back after.
static MOCHI_WAS_SHOWN: AtomicBool = AtomicBool::new(false);

const EVERY: Duration = Duration::from_millis(1500);

/// Whether a fullscreen app is in front, as last seen.
pub fn active() -> bool {
    FULLSCREEN.load(Ordering::Relaxed)
}

/// What Windows' notification state means here: someone is watching or
/// presenting something fullscreen. It only ever looks at the main display, so
/// `covers_a_display` backs it up for the others.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_fullscreen_state(state: i32) -> bool {
    // QUNS_BUSY (2), QUNS_RUNNING_D3D_FULL_SCREEN (3), QUNS_PRESENTATION_MODE (4).
    matches!(state, 2 | 3 | 4)
}

#[cfg(windows)]
fn query(app: &AppHandle) -> bool {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;
    unsafe { SHQueryUserNotificationState() }.map(|s| is_fullscreen_state(s.0)).unwrap_or(false) || covers_a_display(app)
}

#[cfg(not(windows))]
fn query(_app: &AppHandle) -> bool {
    false
}

/// What the foreground window looks like, for `is_fullscreen`.
struct Foreground {
    ours: bool,
    class: String,
    zoomed: bool,
    captioned: bool,
    /// left, top, right, bottom, in physical px
    rect: (i32, i32, i32, i32),
}

/// A window that covers a whole display is a game or a video in full screen,
/// except our own windows, the desktop, and a maximized app with a title bar
/// (it reaches the display edges when the taskbar auto-hides). A game that
/// maximizes a caption-less popup still counts.
fn is_fullscreen(fg: &Foreground, displays: &[(i32, i32, u32, u32)]) -> bool {
    let desktop = fg.class == "Progman" || fg.class == "WorkerW";
    let maximized_app = fg.zoomed && fg.captioned;
    !fg.ours && !desktop && !maximized_app && displays.iter().any(|&d| covers(fg.rect, d))
}

/// A game that is not DPI-aware on a scaled display comes out a pixel or two
/// short of the display after Windows rounds its size, hence the slack.
const SLACK: i32 = 2;

fn covers(r: (i32, i32, i32, i32), (x, y, w, h): (i32, i32, u32, u32)) -> bool {
    r.0 <= x + SLACK && r.1 <= y + SLACK && r.2 >= x + w as i32 - SLACK && r.3 >= y + h as i32 - SLACK
}

/// A fullscreen app on any display, the main one or not.
#[cfg(windows)]
fn covers_a_display(app: &AppHandle) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsZoomed,
        GWL_STYLE, WS_CAPTION,
    };
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(fg, &mut name) }.max(0) as usize;
    let mut r = RECT::default();
    if unsafe { GetWindowRect(fg, &mut r) }.is_err() {
        return false;
    }
    let caption = WS_CAPTION.0 as isize;
    let fg = Foreground {
        ours: pid == unsafe { GetCurrentProcessId() },
        class: String::from_utf16_lossy(&name[..len]),
        zoomed: unsafe { IsZoomed(fg) }.as_bool(),
        captioned: unsafe { GetWindowLongPtrW(fg, GWL_STYLE) } & caption == caption,
        rect: (r.left, r.top, r.right, r.bottom),
    };
    let displays: Vec<_> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    is_fullscreen(&fg, &displays)
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(EVERY);
        let now = query(&app);
        if now != FULLSCREEN.swap(now, Ordering::Relaxed) {
            crate::log::line(format!("fullscreen: {}", if now { "on, hiding" } else { "off, back" }));
            apply(&app, now);
        }
    });
}

fn apply(app: &AppHandle, fullscreen: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(island) = handle.get_webview_window(crate::island::WINDOW_LABEL) {
            if fullscreen {
                let _ = island.hide();
            } else {
                let _ = island.show();
                let _ = island.set_always_on_top(true);
            }
        }
        if let Some(mochi) = handle.get_webview_window(crate::desktop::LABEL) {
            if fullscreen {
                MOCHI_WAS_SHOWN.store(mochi.is_visible().unwrap_or(false), Ordering::Relaxed);
                let _ = mochi.hide();
            } else if MOCHI_WAS_SHOWN.swap(false, Ordering::Relaxed) {
                let _ = mochi.show();
            }
        }
    });
    // The pill decides for itself: it re-reads `active()` when it updates.
    crate::flow::pill::update(app, |_| {});
}

#[cfg(test)]
mod tests {
    use super::{is_fullscreen, is_fullscreen_state, Foreground};

    const MAIN: (i32, i32, u32, u32) = (0, 0, 1920, 1080);
    const SIDE: (i32, i32, u32, u32) = (-1536, -6, 1536, 864);

    fn game(rect: (i32, i32, i32, i32)) -> Foreground {
        Foreground { ours: false, class: "UnityWndClass".into(), zoomed: false, captioned: false, rect }
    }

    #[test]
    fn a_game_on_any_display_counts() {
        assert!(is_fullscreen(&game((0, 0, 1920, 1080)), &[MAIN, SIDE]), "main display");
        assert!(is_fullscreen(&game((-1536, -6, 0, 858)), &[MAIN, SIDE]), "the display Windows does not look at");
        assert!(is_fullscreen(&game((-1535, -5, 0, 858)), &[MAIN, SIDE]), "a pixel short after DPI rounding");
        assert!(!is_fullscreen(&game((100, 100, 900, 700)), &[MAIN, SIDE]), "windowed game");
        assert!(!is_fullscreen(&game((-1536, -6, 0, 820)), &[MAIN, SIDE]), "stops above the taskbar");
        let popup = Foreground { zoomed: true, ..game((0, 0, 1920, 1080)) };
        assert!(is_fullscreen(&popup, &[MAIN]), "a game that maximizes a caption-less popup");
    }

    #[test]
    fn the_desktop_a_maximized_app_and_our_windows_do_not() {
        let whole = (-8, -8, 1928, 1088);
        let maximized = Foreground { zoomed: true, captioned: true, ..game(whole) };
        assert!(!is_fullscreen(&maximized, &[MAIN]), "maximized app, taskbar auto-hidden");
        for class in ["Progman", "WorkerW"] {
            assert!(!is_fullscreen(&Foreground { class: class.into(), ..game(whole) }, &[MAIN]), "{class}");
        }
        assert!(!is_fullscreen(&Foreground { ours: true, ..game(whole) }, &[MAIN]), "our own window");
    }

    #[test]
    fn only_fullscreen_states_hide() {
        // QUNS_NOT_PRESENT, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_QUIET_TIME, QUNS_APP.
        for state in [1, 5, 6, 7] {
            assert!(!is_fullscreen_state(state), "{state}");
        }
        for state in [2, 3, 4] {
            assert!(is_fullscreen_state(state), "{state}");
        }
    }
}
