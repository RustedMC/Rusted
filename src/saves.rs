//! Player files under players/.

use super::*;

/// players/<name>.dat: eye position (3 x f32), then the inventory.
pub(crate) fn player_path(name: &str) -> std::path::PathBuf {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    std::path::Path::new("players").join(format!("{safe}.dat"))
}

pub(crate) fn load_player(name: &str, spawn: (f32, f32, f32)) -> ((f32, f32, f32), inventory::Inventory) {
    let fresh = (spawn, inventory::Inventory::default());
    let Ok(b) = std::fs::read(player_path(name)) else { return fresh };
    let f = |i: usize| b.get(i..i + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap()));
    match (f(0), f(4), f(8), b.get(12..).and_then(inventory::Inventory::from_bytes)) {
        (Some(x), Some(y), Some(z), Some(inv)) if x.is_finite() && y.is_finite() && z.is_finite() => ((x, y, z), inv),
        _ => {
            println!("{name}: player save is damaged; starting fresh");
            fresh
        }
    }
}

pub(crate) fn save_xp(name: &str, xp: xp::Xp) {
    let _ = std::fs::create_dir_all("players").and_then(|()| std::fs::write(player_path(name).with_extension("xp"), xp.to_text()));
}

pub(crate) fn save_mode(name: &str, mode: &GameType) {
    let m = match mode {
        GameType::Creative => "creative",
        GameType::Adventure => "adventure",
        GameType::Spectator => "spectator",
        _ => "survival",
    };
    let _ = std::fs::create_dir_all("players").and_then(|()| std::fs::write(player_path(name).with_extension("mode"), m));
}

pub(crate) fn save_player(name: &str, pos: (f32, f32, f32), inv: &inventory::Inventory) {
    let mut b = Vec::new();
    for v in [pos.0, pos.1, pos.2] {
        b.extend(v.to_le_bytes());
    }
    b.extend(inv.to_bytes());
    let path = player_path(name);
    let tmp = path.with_extension("tmp");
    let result = std::fs::create_dir_all("players").and_then(|()| std::fs::write(&tmp, &b)).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = result {
        println!("{name}: failed to save player: {e}");
    }
}

/// Armor points worn (vanilla Bedrock values).
pub(crate) fn armor_points(inv: &inventory::Inventory) -> f32 {
    (inventory::ARMOR..inventory::ARMOR + 4)
        .filter_map(|i| inv.held(i).and_then(|s| items::name_of(s.item.id)))
        .map(|n| match n.trim_start_matches("minecraft:") {
            "leather_helmet" | "leather_boots" | "golden_boots" | "chainmail_boots" => 1.0,
            "leather_leggings" | "golden_helmet" | "chainmail_helmet" | "iron_helmet" | "iron_boots" | "turtle_helmet" => 2.0,
            "leather_chestplate" | "golden_leggings" | "diamond_helmet" | "netherite_helmet" | "diamond_boots" | "netherite_boots" => 3.0,
            "chainmail_leggings" => 4.0,
            "golden_chestplate" | "chainmail_chestplate" | "iron_leggings" => 5.0,
            "iron_chestplate" | "diamond_leggings" | "netherite_leggings" => 6.0,
            "diamond_chestplate" | "netherite_chestplate" => 8.0,
            _ => 0.0,
        })
        .sum()
}

/// Armor soaks mob, arrow and explosion damage (vanilla: up to 80%).
pub(crate) fn after_armor(damage: health::Damage, inv: &inventory::Inventory) -> health::Damage {
    use health::Damage::*;
    let a = armor_points(inv);
    let soak = |d: f32| d * (1.0 - (a - d / 2.0).max(a / 5.0).min(20.0) / 25.0);
    match damage {
        Mob(d, by) => Mob(soak(d), by),
        Shot(d, by) => Shot(soak(d), by),
        Explosion(d, by) => Explosion(soak(d), by),
        other => other,
    }
}

/// Online players' entity ids -> stable pet-owner keys.
pub(crate) fn owner_keys(server: &players::Server) -> HashMap<i64, i64> {
    server.all().into_iter().map(|p| (p.id, mobs::owner_key(&p.name))).collect()
}
