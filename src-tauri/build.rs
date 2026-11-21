use std::{env, fs, path::Path};

fn main() {
    if env::var("PROFILE").as_deref() == Ok("debug") {
        let release_dir = Path::new("../target/release");
        if fs::create_dir_all(release_dir).is_ok() {
            for binary in [
                "vg-campus-daemon.exe",
                "vg-campus-notifier.exe",
                "vg-campus-setup.exe",
            ] {
                let path = release_dir.join(binary);
                if !path.exists() {
                    let _ = fs::write(&path, []);
                }
            }
        }
    }
    tauri_build::build()
}
