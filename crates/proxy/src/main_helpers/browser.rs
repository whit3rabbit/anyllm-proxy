//! Best-effort default-browser launcher for the zero-arg web UI default.

/// Open `url` in the default browser. Fire-and-forget: the child is spawned
/// detached and any error (headless box, missing opener) is ignored so this
/// never blocks or fails server startup.
pub fn open(url: &str) {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };

    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };

    #[cfg(target_os = "windows")]
    let mut cmd = {
        // `start` is a cmd builtin; the empty "" is the (ignored) window title so a
        // quoted URL isn't consumed as the title.
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url; // unsupported platform: nothing to do
        return;
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let _ = cmd.spawn(); // detached; ignore result
    }
}
