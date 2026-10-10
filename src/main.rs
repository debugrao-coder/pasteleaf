#![windows_subsystem = "windows"]

mod ui;

fn main() {
    if let Err(error) = ui::run() {
        ui::error_dialog(&error);
        std::process::exit(1);
    }
}
