// Copyright (C) 2026 SpruceOS Team
// Licensed under CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)

/// Embedded boxart database files
/// These are included at compile time from the assets/boxartdb directory

macro_rules! include_boxart_db {
    ($name:expr) => {
        include_str!(concat!("../assets/boxartdb/", $name, "_games.txt"))
    };
}

/// Get the boxart database content for a system
pub fn get_boxart_db(system: &str) -> Option<&'static str> {
    match system.to_uppercase().as_str() {
        "NES" => Some(include_boxart_db!("FC")),
        "MASTER SYSTEM" => Some(include_boxart_db!("MS")),
        "GAME BOY" => Some(include_boxart_db!("GB")),
        "GENESIS" => Some(include_boxart_db!("MD")),
        "TURBOGRAFX-16" => Some(include_boxart_db!("PCE")),
        "PCECD" => Some(include_boxart_db!("PCECD")),
        "GAME GEAR" => Some(include_boxart_db!("GG")),
        "SNES" => Some(include_boxart_db!("SFC")),
        "NEO GEO POCKET" => Some(include_boxart_db!("NGP")),
        "GAME BOY COLOR" => Some(include_boxart_db!("GBC")),
        "NEO GEO POCKET COLOR" => Some(include_boxart_db!("NGPC")),
        "GAME BOY ADVANCE" => Some(include_boxart_db!("GBA")),
        _ => None,
    }
}
