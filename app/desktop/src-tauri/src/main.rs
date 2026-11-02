#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() { if floatboard_core::build_identity::handle(){return;} floatboard_native::run() }
