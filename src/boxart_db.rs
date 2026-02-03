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
        "AMIGA" => Some(include_boxart_db!("AMIGA")),
        "ARCADE" => Some(include_boxart_db!("ARCADE")),
        "ARDUBOY" => Some(include_boxart_db!("ARDUBOY")),
        "ATARI" => Some(include_boxart_db!("ATARI")),
        "CHAI" => Some(include_boxart_db!("CHAI")),
        "COLECO" => Some(include_boxart_db!("COLECO")),
        "COMMODORE" => Some(include_boxart_db!("COMMODORE")),
        "CPC" => Some(include_boxart_db!("CPC")),
        "DC" => Some(include_boxart_db!("DC")),
        "DOOM" => Some(include_boxart_db!("DOOM")),
        "DOS" => Some(include_boxart_db!("DOS")),
        "FAIRCHILD" => Some(include_boxart_db!("FAIRCHILD")),
        "FBNEO" => Some(include_boxart_db!("FBNEO")),
        "FC" => Some(include_boxart_db!("FC")),
        "FDS" => Some(include_boxart_db!("FDS")),
        "FIFTYTWOHUNDRED" => Some(include_boxart_db!("FIFTYTWOHUNDRED")),
        "GB" => Some(include_boxart_db!("GB")),
        "GBA" => Some(include_boxart_db!("GBA")),
        "GBC" => Some(include_boxart_db!("GBC")),
        "GG" => Some(include_boxart_db!("GG")),
        "INTELLIVISION" => Some(include_boxart_db!("INTELLIVISION")),
        "LYNX" => Some(include_boxart_db!("LYNX")),
        "MD" => Some(include_boxart_db!("MD")),
        "MS" => Some(include_boxart_db!("MS")),
        "MSUMD" => Some(include_boxart_db!("MSUMD")),
        "MSX" => Some(include_boxart_db!("MSX")),
        "N64" => Some(include_boxart_db!("N64")),
        "NDS" => Some(include_boxart_db!("NDS")),
        "NEOCD" => Some(include_boxart_db!("NEOCD")),
        "NEOGEO" => Some(include_boxart_db!("NEOGEO")),
        "NGP" => Some(include_boxart_db!("NGP")),
        "NGPC" => Some(include_boxart_db!("NGPC")),
        "ODYSSEY" => Some(include_boxart_db!("ODYSSEY")),
        "PCE" => Some(include_boxart_db!("PCE")),
        "PCECD" => Some(include_boxart_db!("PCECD")),
        "POKE" => Some(include_boxart_db!("POKE")),
        "PS" => Some(include_boxart_db!("PS")),
        "PSP" => Some(include_boxart_db!("PSP")),
        "QUAKE" => Some(include_boxart_db!("QUAKE")),
        "SATELLAVIEW" => Some(include_boxart_db!("SATELLAVIEW")),
        "SCUMMVM" => Some(include_boxart_db!("SCUMMVM")),
        "SEGACD" => Some(include_boxart_db!("SEGACD")),
        "SEGASGONE" => Some(include_boxart_db!("SEGASGONE")),
        "SEVENTYEIGHTHUNDRED" => Some(include_boxart_db!("SEVENTYEIGHTHUNDRED")),
        "SFC" => Some(include_boxart_db!("SFC")),
        "SGB" => Some(include_boxart_db!("SGB")),
        "SGFX" => Some(include_boxart_db!("SGFX")),
        "SUFAMI" => Some(include_boxart_db!("SUFAMI")),
        "SUPERVISION" => Some(include_boxart_db!("SUPERVISION")),
        "THIRTYTWOX" => Some(include_boxart_db!("THIRTYTWOX")),
        "TIC" => Some(include_boxart_db!("TIC")),
        "VB" => Some(include_boxart_db!("VB")),
        "VECTREX" => Some(include_boxart_db!("VECTREX")),
        "VIC20" => Some(include_boxart_db!("VIC20")),
        "VIDEOPAC" => Some(include_boxart_db!("VIDEOPAC")),
        "WOLF" => Some(include_boxart_db!("WOLF")),
        "WS" => Some(include_boxart_db!("WS")),
        "WSC" => Some(include_boxart_db!("WSC")),
        "X68000" => Some(include_boxart_db!("X68000")),
        "ZXS" => Some(include_boxart_db!("ZXS")),
        _ => None,
    }
}
