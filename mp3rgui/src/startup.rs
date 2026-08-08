//! Diagnosis of `eframe::run_native` startup failures (issue #282).
//!
//! The raw eframe error is accurate but unhelpful — a user who sees
//! "egui_glow requires opengl 2.0+" has no way to know that installing a
//! graphics driver, or just using the CLI, would solve their problem.

use eframe::Error;

const CLI_HINT: &str =
    "  - Use the mp3rgain command-line tool instead. It does everything the GUI\n\
     \x20   does and needs no graphics driver:\n\
     \x20   https://github.com/M-Igashi/mp3rgain";

/// Why the GUI could not start, as far as eframe's error tells us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupFailure {
    /// No usable OpenGL 2.0+ context.
    NoOpenGl,
    /// No window system to open a window on.
    NoDisplay,
    /// Anything else — no specific advice to give.
    Unknown,
}

/// Map an eframe error onto the failure we can advise on.
///
/// All three glow-related variants mean the same thing to a user: this
/// machine cannot give us an OpenGL 2.0+ context. `NoGlutinConfigs` fires
/// when no GL config matches at all, `Glutin` when context creation fails,
/// and `OpenGL` when the context exists but is too old — the observed case
/// in the winget validation VM.
pub fn classify(err: &Error) -> StartupFailure {
    match err {
        Error::OpenGL(_) | Error::Glutin(_) | Error::NoGlutinConfigs(..) => {
            StartupFailure::NoOpenGl
        }
        Error::Winit(_) | Error::WinitEventLoop(_) => StartupFailure::NoDisplay,
        _ => StartupFailure::Unknown,
    }
}

/// Build the message shown on stderr and in the error dialog.
///
/// `details` is the raw eframe error, kept at the end so bug reports still
/// carry the exact failure.
pub fn startup_error_message(failure: StartupFailure, details: &str) -> String {
    let body = match failure {
        StartupFailure::NoOpenGl => format!(
            "This computer has no usable OpenGL 2.0+ driver, which the GUI requires.\n\
             Common causes are a missing or outdated graphics driver, a virtual\n\
             machine without 3D acceleration, or a remote desktop session that does\n\
             not forward OpenGL.\n\
             \n\
             What you can try:\n\
             \x20 - Install or update your graphics driver.\n\
             \x20 - On a virtual machine, enable 3D acceleration for the guest.\n\
             {CLI_HINT}"
        ),
        StartupFailure::NoDisplay => format!(
            "No display or window system is available, so the GUI cannot open a\n\
             window. This is expected over a plain SSH session, in a container, or\n\
             on a Windows Server Core install.\n\
             \n\
             What you can try:\n\
             {CLI_HINT}"
        ),
        StartupFailure::Unknown => format!(
            "The GUI failed to start for an unexpected reason.\n\
             \n\
             What you can try:\n\
             {CLI_HINT}\n\
             \x20 - Report this at https://github.com/M-Igashi/mp3rgain/issues"
        ),
    };

    format!("mp3rgain GUI could not start.\n\n{body}\n\nDetails: {details}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui_glow::PainterError;

    /// The exact error seen in the winget validation VM (issue #282).
    #[test]
    fn old_opengl_is_classified_and_explained() {
        let err = Error::OpenGL(PainterError::from(
            "egui_glow requires opengl 2.0+. ".to_owned(),
        ));
        assert_eq!(classify(&err), StartupFailure::NoOpenGl);

        let msg = startup_error_message(classify(&err), &err.to_string());
        assert!(msg.contains("OpenGL 2.0+ driver"));
        assert!(msg.contains("graphics driver"));
        // The CLI is the actionable escape hatch on a GPU-less box.
        assert!(msg.contains("mp3rgain command-line tool"));
        // The raw error must survive for bug reports.
        assert!(msg.contains("egui_glow requires opengl 2.0+"));
    }

    #[test]
    fn unknown_failure_still_points_at_the_cli_and_issue_tracker() {
        let err = Error::AppCreation("something else broke".into());
        assert_eq!(classify(&err), StartupFailure::Unknown);

        let msg = startup_error_message(classify(&err), &err.to_string());
        assert!(msg.contains("mp3rgain command-line tool"));
        assert!(msg.contains("issues"));
        assert!(msg.contains("something else broke"));
    }

    /// Every message must open with the same headline and end with the raw
    /// error, whichever branch produced it.
    #[test]
    fn all_variants_share_headline_and_keep_details() {
        for failure in [
            StartupFailure::NoOpenGl,
            StartupFailure::NoDisplay,
            StartupFailure::Unknown,
        ] {
            let msg = startup_error_message(failure, "raw-error-text");
            assert!(msg.starts_with("mp3rgain GUI could not start."));
            assert!(msg.ends_with("Details: raw-error-text"));
        }
    }
}
