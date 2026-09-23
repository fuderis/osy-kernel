use crate::prelude::*;

use std::{
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

/// RAII wrapper for sudo session management.
///
/// When Drops - cancels the background `keep-alive` task
/// and resets sudo password cache (`sudo -k`).
pub struct SudoGuard {
    running: Arc<AtomicBool>,
}

impl SudoGuard {
    /// Initializes sudo session and returns SudoGuard.
    #[cfg(unix)]
    pub fn new() -> Result<Self> {
        // 1. Check for already active timestamp
        let check_status = Command::new("sudo").arg("-n").arg("-v").status()?;

        // 2. If it's not active, ask for the password once
        if !check_status.success() {
            println!("🔐 Sudo rights are required.");

            let auth_status = Command::new("sudo").arg("-v").status()?;

            if !auth_status.success() {
                return Err("Couldn't get sudo rights.".into());
            }
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);

        // 3. Launching background timestamp update task
        atoman::spawn(async move {
            let mut interval = atoman::time::interval(Duration::from_secs(60));
            interval.tick().await;

            while running_clone.load(Ordering::Relaxed) {
                interval.tick().await;

                if !running_clone.load(Ordering::Relaxed) {
                    break;
                }

                let _ = atoman::process::Command::new("sudo")
                    .arg("-n")
                    .arg("-v")
                    .status()
                    .await;
            }
        });

        Ok(Self { running })
    }

    /// A stub for Windows.
    #[cfg(windows)]
    pub fn new() -> Result<()> {
        ()
    }
}

impl Drop for SudoGuard {
    fn drop(&mut self) {
        // signal completion of the background task
        self.running.store(false, Ordering::Relaxed);

        // reset sudo cache (close the session)
        let _ = Command::new("sudo").arg("-k").status();
    }
}
