// Bez konzolového okna v sestavení pro hráče.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    aae_lib::run();
}
