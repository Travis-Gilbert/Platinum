//! Configure the bundled media runtime before any application threads start.
//! Servo loads its pinned plugins from `MacOS/lib`; a bundle must not discover
//! plugins or registry entries from a developer's Homebrew installation.
use std::path::Path;

pub fn init() -> std::io::Result<()> {
    let executable = std::env::current_exe()?;
    if !is_bundled(&executable) {
        return Ok(());
    }
    // SAFETY: called first in main, before GPUI, Servo, Tokio, or worker threads.
    // No other thread can be reading the process environment at this point.
    unsafe {
        let contents = executable.parent().unwrap().parent().unwrap();
        std::env::set_var("GIO_MODULE_DIR", contents.join("PlugIns"));
        std::env::set_var("GIO_EXTRA_MODULES", "");
        for name in [
            "GST_PLUGIN_PATH",
            "GST_PLUGIN_PATH_1_0",
            "GST_PLUGIN_SYSTEM_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
        ] {
            std::env::set_var(name, "");
        }
        // Explicit plugin loading does not need a scanner process or a disk
        // registry. Ignore machine-local caches and avoid writing into the app.
        std::env::set_var("GST_REGISTRY", "/dev/null");
        std::env::set_var("GST_REGISTRY_1_0", "/dev/null");
        std::env::set_var("GST_REGISTRY_UPDATE", "no");
        std::env::set_var("GST_REGISTRY_FORK", "no");
    }
    Ok(())
}

fn is_bundled(executable: &Path) -> bool {
    executable.parent().is_some_and(|macos| {
        macos.file_name().is_some_and(|name| name == "MacOS")
            && macos.parent().is_some_and(|contents| {
                contents.file_name().is_some_and(|name| name == "Contents")
                    && contents.join("Resources/platinum-bundle.json").is_file()
            })
    })
}
