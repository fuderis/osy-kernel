use crate::prelude::*;
use sysinfo::System;

static SYSTEM_INFO: State<String> = State::new(|| {
    let os_name = System::name().unwrap_or_else(|| "Unknown OS".into());
    let kernel = System::kernel_version().unwrap_or_default();
    let arch = std::env::consts::ARCH;

    if kernel.is_empty() {
        format!("{os_name} {arch}")
    } else {
        format!("{os_name} {arch} (kernel {kernel})")
    }
});

/// Returns human-readable system info (OS kind & version).
pub fn system_info() -> Arc<String> {
    SYSTEM_INFO.get()
}

/// Returns human-readable system info (OS kind & version).
pub fn system_info_cloned() -> String {
    SYSTEM_INFO.get_cloned()
}
