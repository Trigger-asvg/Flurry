#![windows_subsystem = "windows"]
//! Flurry — живая погода поверх рабочего стола Windows.

mod app;
mod auto;
mod brand;
mod ipc;
mod osd;
mod render;
mod settings;
mod ui;
mod weather;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--settings") {
        let page = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)).cloned();
        ui::run(page);
        return;
    }
    // команды уже запущенному экземпляру
    let cmds = [
        ("--toggle", ipc::cmd::TOGGLE),
        ("--clear", ipc::cmd::CLEAR),
        ("--next-mode", ipc::cmd::NEXT_MODE),
        ("--storm", ipc::cmd::STORM),
        ("--reload", ipc::cmd::RELOAD),
        ("--exit", ipc::cmd::EXIT),
    ];
    let mut sent = false;
    if let Some(m) = args.iter().position(|a| a == "--mode").and_then(|i| args.get(i + 1)) {
        let m = m.to_lowercase();
        if let Some(i) = settings::Mode::ALL.iter().position(|x| format!("{x:?}").to_lowercase() == m) {
            ipc::post(100 + i);
            sent = true;
        }
    }
    for (flag, c) in cmds {
        if has(flag) {
            ipc::post(c);
            sent = true;
        }
    }
    if sent {
        return;
    }
    if !brand::verify() {
        settings::log("watermark check failed: modified build");
    }
    app::run(has("--tray"));
}
