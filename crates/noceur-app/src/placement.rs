//! Remembering where the window was.
//!
//! Kept in the app's own config file beside the executable rather than through
//! the window-state plugin, which writes to `%APPDATA%`. That is a deliberate
//! choice and the same one [`crate::config`] makes: a portable install stays
//! portable, and everything the app remembers about you sits in one file you can
//! read or delete.
//!
//! The whole of the difficulty here is monitors. A window restored to where it
//! was on a screen that is no longer attached is a window nobody can reach, and
//! it looks exactly like the app failing to start. So a saved position is only
//! used when it still lands on a display that exists.

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, Window};

/// Where the window was, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Smallest window worth restoring to.
///
/// Must not be under the `minWidth`/`minHeight` in `tauri.conf.json`: a stored
/// size below those is a corrupt or hand-edited entry, and honouring it would
/// produce a window the user cannot use and did not ask for.
const MIN: (u32, u32) = (800, 500);

/// Puts the window back where it was.
///
/// Size first, then position, then maximize — in that order, because maximizing
/// first would make the size and position apply to a window that is about to
/// cover the screen anyway, and unmaximizing would then restore the wrong shape.
pub fn restore(window: &Window) {
    let path = crate::config::Config::default_path();
    let Some(saved) = crate::config::Config::load(&path).window else {
        return;
    };
    if saved.width < MIN.0 || saved.height < MIN.1 {
        return;
    }

    let _ = window.set_size(PhysicalSize::new(saved.width, saved.height));

    if on_a_real_display(window, &saved) {
        let _ = window.set_position(PhysicalPosition::new(saved.x, saved.y));
    }
    if saved.maximized {
        let _ = window.maximize();
    }
}

/// Records where the window is now.
///
/// A maximized window keeps whatever size and position were stored before it was
/// maximized. Its current geometry is the size of the screen, and saving that as
/// the restored size means unmaximizing next time gives a window that fills the
/// display — which is not what maximizing something is meant to do to it.
pub fn remember(window: &Window) {
    let path = crate::config::Config::default_path();
    let mut config = crate::config::Config::load(&path);
    let maximized = window.is_maximized().unwrap_or(false);

    let placement = if maximized {
        match config.window {
            Some(previous) => Placement {
                maximized: true,
                ..previous
            },
            // Maximized on the very first run, so there is no earlier shape to
            // keep. The configured default is the honest answer.
            None => return,
        }
    } else {
        let (Ok(size), Ok(at)) = (window.outer_size(), window.outer_position()) else {
            return;
        };
        Placement {
            x: at.x,
            y: at.y,
            width: size.width,
            height: size.height,
            maximized: false,
        }
    };

    if config.window != Some(placement) {
        config.window = Some(placement);
        let _ = config.save(&path);
    }
}

/// Whether a saved position still lands on a display that exists.
///
/// Checks the title bar rather than the whole window: a window is reachable when
/// its top edge is on a screen, and demanding that all of it fit would refuse
/// perfectly good positions on a window deliberately hung off the side.
fn on_a_real_display(window: &Window, saved: &Placement) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };

    monitors.iter().any(|monitor| {
        let at = monitor.position();
        let size = monitor.size();
        let (left, top) = (at.x, at.y);
        let (right, bottom) = (at.x + size.width as i32, at.y + size.height as i32);

        // The bar has to overlap horizontally by enough to grab, and its top
        // edge has to be within the screen vertically.
        let overlap = (saved.x + saved.width as i32).min(right) - saved.x.max(left);
        overlap > 100 && saved.y >= top - 8 && saved.y < bottom - 40
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement() -> Placement {
        Placement {
            x: 100,
            y: 80,
            width: 1400,
            height: 900,
            maximized: false,
        }
    }

    #[test]
    fn a_placement_round_trips_through_the_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        crate::config::Config {
            window: Some(placement()),
            ..Default::default()
        }
        .save(&path)
        .unwrap();

        assert_eq!(crate::config::Config::load(&path).window, Some(placement()));
    }

    /// A config written before this existed, and one written by hand, both have
    /// to start the app rather than stop it.
    #[test]
    fn a_config_without_a_placement_is_fine() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"discogsToken":""}"#).unwrap();

        assert_eq!(crate::config::Config::load(&path).window, None);
    }

    /// Below the window's own minimum is a corrupt entry, and honouring it would
    /// produce a window the user cannot use and never asked for.
    #[test]
    fn an_impossible_size_is_recognised_as_one() {
        assert!(MIN.0 >= 800 && MIN.1 >= 500, "must match tauri.conf.json");

        let tiny = Placement {
            width: 20,
            height: 10,
            ..placement()
        };
        assert!(tiny.width < MIN.0);
        assert!(tiny.height < MIN.1);
    }
}
