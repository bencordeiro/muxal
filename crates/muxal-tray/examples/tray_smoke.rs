//! Manual smoke test: spawn the tray, hold it briefly, print whether it started.
//! `cargo run -p muxal-tray --example tray_smoke` — then check the bus for a
//! StatusNotifierItem. Not part of CI.

fn main() {
    let icon = muxal_tray::TrayIcon {
        name: "muxal".into(),
        tooltip: "muxal".into(),
        rgba: None,
    };
    let labels = muxal_tray::TrayLabels {
        show: "Show muxal".into(),
        quit: "Quit muxal".into(),
        agents: "Agents".into(),
        notifications: "Notifications".into(),
    };
    match muxal_tray::TrayController::spawn(icon, labels) {
        Some(t) => {
            println!("TRAY_SPAWNED");
            t.update(muxal_tray::TrayModel::default());
            std::thread::sleep(std::time::Duration::from_secs(4));
            println!("TRAY_DONE");
        }
        None => println!("TRAY_NONE"),
    }
}
