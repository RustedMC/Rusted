# Roadmap

Rough order of what's next. See [PORTING.md](PORTING.md) for where each part stands today.

## Next up
- Firework rockets launched from the ground, with their explosion effects
- Maps and the cartography table (waiting on a fix in bedrock-rs: its map picture packet is missing the type flags)

## After that
- PNX's mob AI for the rest of the mobs: neutrals, villagers, fish and flyers (passive animals and hostiles are on the new brain in `src/ai.rs`)
- Sculk sensors and shriekers (repeater locking, detector rails and lectern signals are done; quasi-connectivity is Java-only)
- `/execute` and the remaining vanilla commands
- More plugin events (movement, inventory clicks, pickup, projectiles, pre-login), plugins loaded at runtime
  (custom items, chest menus, item-use events and player control are done)
- Sky light, NPC dialogs, camera control

## Before 1.0
- Benchmarks against PNX on the same machine
- Replace the block, item and creative data with properly licensed files
- Long-running tests with real players
