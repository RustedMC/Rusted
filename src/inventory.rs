//! Server-side inventory that answers the client's item stack requests. Every request
//! needs a reply listing the slots it changed or the client snaps the item back
//! (worked out from a recorded PNX session).

use bedrock::protocol::V2225;
use bedrock::protocol::v662::packets::RequestsEntry;
use bedrock::protocol::v2168::enums::ItemStackNetResult;
use bedrock::protocol::v2168::types::{ItemStackRequestSlotInfo, NetworkItemStackDescriptorV2, RedactableString};
use bedrock::protocol::v2193::types::{ItemStackResponseInfo, ItemStackResponseSlotInfo};
use bedrock::protocol::v712::types::ItemStackResponseContainerInfo;
use bedrock::protocol::v729::types::FullContainerName;
use bedrock::protocol::v2225::enums::{ContainerEnumName, ItemStackRequestActionType};

type Protocol = V2225;


#[derive(Clone, Debug)]
pub struct Stack {
    /// `count` is the real count, `stack_size` in here is ignored.
    pub item: NetworkItemStackDescriptorV2,
    pub count: i8,
    pub net_id: i32,
}

impl Stack {
    fn same_item(&self, other: &Stack) -> bool {
        self.item.id == other.item.id
            && self.item.aux_value == other.item.aux_value
            && self.item.block_runtime_id == other.item.block_runtime_id
    }

    pub fn to_network(&self) -> NetworkItemStackDescriptorV2 {
        let mut item = self.item.clone();
        item.stack_size = self.count as u16;
        item.net_id = Some(self.net_id);
        item
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    Main(usize),
    Cursor,
    Created,
    /// slot in the open chest/barrel/furnace
    Open(usize),
    /// crafting grid, 2x2 in the inventory or 3x3 at a table
    Craft(usize),
}

/// Client's crafting grid slot numbers, 28-31 (2x2) and 32-40 (3x3).
const CRAFT_FIRST: usize = 28;
const CRAFT_SLOTS: usize = 13;

/// UI grid slots for the anvil, grindstone and enchanting table.
pub const ANVIL_INPUT: usize = 1;
pub const ANVIL_MATERIAL: usize = 2;
pub const GRIND_INPUT: usize = 3;
pub const GRIND_EXTRA: usize = 4;
pub const ENCHANT_INPUT: usize = 5;
pub const ENCHANT_LAPIS: usize = 6;
pub const SMITH_INPUT: usize = 7;
pub const SMITH_MATERIAL: usize = 8;
pub const SMITH_TEMPLATE: usize = 9;
pub const TRADE_A: usize = 10;
pub const TRADE_B: usize = 11;

pub const CHEST_SLOTS: usize = 27;

/// 0-8 hotbar, 9-35 main, 36-39 armor (head, chest, legs, feet), 40 offhand.
pub const MAIN: usize = 36;
pub const ARMOR: usize = 36;
pub const OFFHAND: usize = 40;
pub const SLOTS: usize = 41;

pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    cursor: Option<Stack>,
    created: Option<Stack>,
    /// Open container, its slots live here until it's closed.
    pub open: Option<Vec<Option<Stack>>>,
    /// Stuff thrown out during the last `handle`, the session spawns it on the ground.
    pub dropped: Vec<Stack>,
    craft: Vec<Option<Stack>>,
    next_net_id: i32,
    /// Hotbar slot in hand (mirrors the session's, for things like totems).
    pub selected: usize,
    /// Sneaking, which raises a held shield.
    pub sneaking: bool,
    /// Set by the session before `handle` so table costs can be checked.
    pub levels: i32,
    pub creative: bool,
    /// Levels spent (anvil, enchanting) and XP given (grindstone) by the last `handle`.
    pub spent_levels: i32,
    pub gained_xp: i32,
    /// Current enchanting offers, (option net id, offer).
    pub offers: Vec<(u32, crate::tables::Offer)>,
    /// Villager with the trade screen open, plus its offers.
    pub trading: Option<(i64, Vec<crate::villagers::OpenOffer>)>,
    /// Trades done in the last `handle`: (offer index, times, trader XP each).
    pub traded: Vec<(usize, i32, i32)>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS], cursor: None, created: None, open: None, dropped: vec![], craft: vec![None; CRAFT_SLOTS], next_net_id: 1, selected: 0, sneaking: false,
            levels: 0, creative: false, spent_levels: 0, gained_xp: 0, offers: vec![], trading: None, traded: vec![],
        }
    }
}

fn place_of(info: &ItemStackRequestSlotInfo<Protocol>) -> Option<Place> {
    let slot = usize::try_from(info.slot).ok()?;
    match info.container_name.container {
        ContainerEnumName::HotbarContainer
        | ContainerEnumName::InventoryContainer
        | ContainerEnumName::CombinedHotbarAndInventoryContainer if slot < MAIN => Some(Place::Main(slot)),
        ContainerEnumName::ArmorContainer if slot < 4 => Some(Place::Main(ARMOR + slot)),
        ContainerEnumName::OffhandContainer if slot == 0 => Some(Place::Main(OFFHAND)),
        ContainerEnumName::CursorContainer => Some(Place::Cursor),
        ContainerEnumName::CreatedOutputContainer => Some(Place::Created),
        ContainerEnumName::LevelEntityContainer | ContainerEnumName::BarrelContainer if slot < 2 * CHEST_SLOTS => Some(Place::Open(slot)),
        ContainerEnumName::FurnaceIngredientContainer
        | ContainerEnumName::BlastFurnaceIngredientContainer
        | ContainerEnumName::SmokerIngredientContainer => Some(Place::Open(0)),
        ContainerEnumName::FurnaceFuelContainer => Some(Place::Open(1)),
        ContainerEnumName::FurnaceResultContainer => Some(Place::Open(2)),
        ContainerEnumName::StonecutterInputContainer => Some(Place::Craft(0)),
        ContainerEnumName::BrewingStandInputContainer => Some(Place::Open(0)),
        ContainerEnumName::BrewingStandResultContainer if (1..4).contains(&slot) => Some(Place::Open(slot)),
        ContainerEnumName::BrewingStandFuelContainer => Some(Place::Open(4)),
        // table screens keep their inputs in the UI grid until closed
        ContainerEnumName::AnvilInputContainer => Some(Place::Craft(ANVIL_INPUT)),
        ContainerEnumName::AnvilMaterialContainer => Some(Place::Craft(ANVIL_MATERIAL)),
        ContainerEnumName::GrindstoneInputContainer => Some(Place::Craft(GRIND_INPUT)),
        ContainerEnumName::GrindstoneAdditionalContainer => Some(Place::Craft(GRIND_EXTRA)),
        ContainerEnumName::SmithingTableInputContainer => Some(Place::Craft(SMITH_INPUT)),
        ContainerEnumName::Trade2Ingredient1Container | ContainerEnumName::TradeIngredient1Container => Some(Place::Craft(TRADE_A)),
        ContainerEnumName::Trade2Ingredient2Container | ContainerEnumName::TradeIngredient2Container => Some(Place::Craft(TRADE_B)),
        ContainerEnumName::SmithingTableMaterialContainer => Some(Place::Craft(SMITH_MATERIAL)),
        ContainerEnumName::SmithingTableTemplateContainer => Some(Place::Craft(SMITH_TEMPLATE)),
        ContainerEnumName::ShulkerBoxContainer if slot < CHEST_SLOTS => Some(Place::Open(slot)),
        ContainerEnumName::EnchantingInputContainer => Some(Place::Craft(ENCHANT_INPUT)),
        ContainerEnumName::EnchantingMaterialContainer => Some(Place::Craft(ENCHANT_LAPIS)),
        ContainerEnumName::CraftingInputContainer if (CRAFT_FIRST..CRAFT_FIRST + CRAFT_SLOTS).contains(&slot) => {
            Some(Place::Craft(slot - CRAFT_FIRST))
        }
        _ => None,
    }
}

/// Quick 0..1 random for wear chances.
fn roll01() -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static S: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut x = S.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    S.store(x, Ordering::Relaxed);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Slot format: present byte, item id i16, count i8, aux u32, block u32 (LE).
/// Present = 2 means NBT user data follows as u16 length + bytes.
pub fn encode_slots(slots: &[Option<Stack>]) -> Vec<u8> {
    let mut b = Vec::with_capacity(slots.len() * 12);
    for s in slots {
        match s {
            None => b.push(0),
            Some(s) => {
                let extra = s.item.user_data_buffer.iter().any(|&x| x != 0);
                b.push(if extra { 2 } else { 1 });
                b.extend(s.item.id.to_le_bytes());
                b.push(s.count as u8);
                b.extend(s.item.aux_value.to_le_bytes());
                b.extend(s.item.block_runtime_id.to_le_bytes());
                if extra {
                    b.extend((s.item.user_data_buffer.len() as u16).to_le_bytes());
                    b.extend(&s.item.user_data_buffer);
                }
            }
        }
    }
    b
}

/// Reads up to `max` slots, older saves can stop after `min`. Returns (slots, bytes read).
/// Stack ids are left at 0.
pub fn decode_slots(b: &[u8], max: usize, min: usize) -> Option<(Vec<Option<Stack>>, usize)> {
    let mut out = vec![None; max];
    let mut i = 0;
    for (n, slot) in out.iter_mut().enumerate() {
        if n >= min && i == b.len() {
            break;
        }
        match *b.get(i)? {
            0 => i += 1,
            kind => {
                let r = b.get(i + 1..i + 12)?;
                let mut item = NetworkItemStackDescriptorV2 {
                    id: i16::from_le_bytes([r[0], r[1]]),
                    stack_size: 1,
                    aux_value: u32::from_le_bytes(r[3..7].try_into().ok()?),
                    net_id: None,
                    block_runtime_id: u32::from_le_bytes(r[7..11].try_into().ok()?),
                    user_data_buffer: vec![0; 10],
                };
                let count = r[2] as i8;
                i += 12;
                if kind == 2 {
                    let len = u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize;
                    item.user_data_buffer = b.get(i + 2..i + 2 + len)?.to_vec();
                    i += 2 + len;
                }
                *slot = Some(Stack { item, count, net_id: 0 });
            }
        }
    }
    Some((out, i))
}

impl Inventory {
    fn fresh_id(&mut self) -> i32 {
        self.next_net_id += 1;
        self.next_net_id
    }

    fn at(&mut self, p: Place) -> &mut Option<Stack> {
        match p {
            Place::Main(i) => &mut self.slots[i],
            Place::Cursor => &mut self.cursor,
            Place::Created => &mut self.created,
            Place::Open(i) => &mut self.open.as_mut().expect("checked by valid()")[i],
            Place::Craft(i) => &mut self.craft[i],
        }
    }

    /// For pick block and /give.
    pub fn set_slot(&mut self, slot: usize, item: NetworkItemStackDescriptorV2, count: i8) -> Option<&Stack> {
        let net_id = self.fresh_id();
        let s = self.slots.get_mut(slot)?;
        *s = Some(Stack { item, count, net_id });
        s.as_ref()
    }

    /// Hotbar first.
    pub fn first_empty(&self) -> Option<usize> {
        self.slots[..MAIN].iter().position(Option::is_none)
    }

    /// Puts the cursor item back when the screen closes, like vanilla.
    /// False if there's no room (the item stays on the cursor).
    pub fn return_cursor(&mut self) -> bool {
        let Some(c) = self.cursor.take() else { return true };
        let target = self.slots[..MAIN]
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.same_item(&c) && s.count + c.count <= crate::items::max_stack(c.item.id)))
            .or_else(|| self.first_empty());
        match target {
            Some(i) => {
                let merged = match self.slots[i].take() {
                    Some(s) => Stack { count: s.count + c.count, ..s },
                    None => c,
                };
                self.slots[i] = Some(merged);
                true
            }
            None => {
                self.cursor = Some(c);
                false
            }
        }
    }

    fn container_packet(&self, inventory_id: u32, container: ContainerEnumName, range: std::ops::Range<usize>) -> Protocol {
        use bedrock::protocol::v1001::packets::InventoryContentPacket;
        Protocol::InventoryContentPacket(Box::new(InventoryContentPacket {
            inventory_id,
            slots: self.slots[range].iter().map(|s| s.as_ref().map_or_else(crate::spawn::air, Stack::to_network)).collect(),
            container_name_data: FullContainerName { container, dynamic_id: None },
            storage_item: crate::spawn::air(),
        }))
    }

    /// Main, armor (window 120) and offhand (window 119), sent on join.
    pub fn all_packets(&self) -> Vec<Protocol> {
        vec![
            self.content_packet(),
            self.container_packet(120, ContainerEnumName::ArmorContainer, ARMOR..ARMOR + 4),
            self.container_packet(119, ContainerEnumName::OffhandContainer, OFFHAND..OFFHAND + 1),
        ]
    }

    pub fn content_packet(&self) -> Protocol {
        use bedrock::protocol::v1001::packets::InventoryContentPacket;
        Protocol::InventoryContentPacket(Box::new(InventoryContentPacket {
            inventory_id: 0,
            slots: self.slots[..MAIN].iter().map(|s| s.as_ref().map_or_else(crate::spawn::air, Stack::to_network)).collect(),
            container_name_data: FullContainerName {
                container: ContainerEnumName::CombinedHotbarAndInventoryContainer,
                dynamic_id: None,
            },
            storage_item: crate::spawn::air(),
        }))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        encode_slots(&self.slots)
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut inv = Self::default();
        // old saves have 36 slots, newer ones 41
        let (slots, _) = decode_slots(b, SLOTS, MAIN)?;
        for (i, s) in slots.into_iter().enumerate() {
            if let Some(s) = s {
                inv.set_slot(i, s.item, s.count);
            }
        }
        Some(inv)
    }

    /// Merges into matching stacks first, then empty slots (hotbar first).
    /// Returns how many didn't fit.
    pub fn add(&mut self, item: NetworkItemStackDescriptorV2, mut count: i8) -> i8 {
        let max = crate::items::max_stack(item.id);
        let probe = Stack { item: item.clone(), count: 0, net_id: 0 };
        for s in self.slots[..MAIN].iter_mut().flatten() {
            if count > 0 && s.same_item(&probe) && s.count < max {
                let n = count.min(max - s.count);
                s.count += n;
                count -= n;
            }
        }
        while count > 0 {
            let Some(i) = self.first_empty() else { break };
            let n = count.min(max);
            self.set_slot(i, item.clone(), n);
            count -= n;
        }
        count
    }

    /// Empties everything out, for death drops.
    pub fn take_all(&mut self) -> Vec<Stack> {
        
        self.slots.iter_mut().chain([&mut self.cursor]).chain(self.craft.iter_mut()).filter_map(Option::take).collect()
    }

    /// For /clear <player> <item>, returns how many were removed.
    pub fn clear_item(&mut self, name: &str) -> u32 {
        let full = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let mut n = 0;
        for s in self.slots.iter_mut() {
            if s.as_ref().is_some_and(|st| crate::items::name_of(st.item.id) == Some(full.as_str())) {
                n += s.take().map_or(0, |st| st.count as u32);
            }
        }
        n
    }

    /// Crafting grid leftovers go back to the inventory on close. Returns what didn't fit.
    pub fn return_craft(&mut self) -> Vec<Stack> {
        let left: Vec<Stack> = self.craft.iter_mut().filter_map(Option::take).collect();
        left.into_iter()
            .filter_map(|s| {
                let n = self.add(s.item.clone(), s.count);
                (n > 0).then_some(Stack { count: n, ..s })
            })
            .collect()
    }

    pub fn enchant_input(&self) -> Option<&Stack> {
        self.craft[ENCHANT_INPUT].as_ref()
    }

    /// The slot of a raised shield (sneaking with one in the main hand or offhand).
    pub fn shield(&self) -> Option<usize> {
        let shield = |i: &usize| self.slots.get(*i).and_then(|s| s.as_ref()).and_then(|s| crate::items::name_of(s.item.id)) == Some("minecraft:shield");
        if !self.sneaking {
            return None;
        }
        [self.selected, OFFHAND].into_iter().find(shield)
    }

    /// Uses up a totem of undying in the main hand or offhand. True if there was one.
    pub fn use_totem(&mut self) -> bool {
        let totem = |s: &Option<Stack>| s.as_ref().and_then(|s| crate::items::name_of(s.item.id)) == Some("minecraft:totem_of_undying");
        let Some(slot) = [self.selected, OFFHAND].into_iter().find(|&i| self.slots.get(i).is_some_and(totem)) else { return false };
        self.use_one(slot);
        true
    }

    pub fn held(&self, slot: usize) -> Option<&Stack> {
        self.slots.get(slot)?.as_ref()
    }

    /// Damages the item in `slot` (PNX Item.setDamage). None if it has no durability,
    /// Some(true) if it broke.
    pub fn wear(&mut self, slot: usize, uses: i32) -> Option<bool> {
        let s = self.slots.get_mut(slot)?;
        let st = s.as_mut()?;
        let max = crate::items::max_durability(crate::items::name_of(st.item.id)?)?;
        if uses <= 0 {
            return Some(false);
        }
        let name = crate::items::name_of(st.item.id).unwrap_or("");
        let armor = ["_helmet", "_chestplate", "_leggings", "_boots"].iter().any(|s| name.ends_with(s));
        let lvl = crate::enchant::level(&st.item.user_data_buffer, crate::enchant::UNBREAKING);
        let uses = (0..uses).filter(|_| crate::enchant::wears(lvl, armor, roll01())).count() as i32;
        if uses == 0 {
            return Some(false);
        }
        let d = crate::items::damage(&st.item) + uses;
        if d >= max {
            *s = None;
            return Some(true);
        }
        crate::items::set_damage(&mut st.item, d);
        Some(false)
    }

    /// Eating or placing in survival. Returns the new stack.
    pub fn use_one(&mut self, slot: usize) -> Option<Option<&Stack>> {
        let s = self.slots.get_mut(slot)?;
        let st = s.as_mut()?;
        st.count -= 1;
        if st.count <= 0 {
            *s = None;
        }
        Some(s.as_ref())
    }

    /// Clears everything including the cursor, returns the item count.
    pub fn clear(&mut self) -> u32 {
        let n = self.slots.iter().chain([&self.cursor]).flatten().map(|s| s.count.max(0) as u32).sum();
        self.slots.iter_mut().for_each(|s| *s = None);
        self.cursor = None;
        n
    }

    /// Loads a chest's slots in with fresh stack ids (client wants new ones).
    pub fn open_container(&mut self, slots: Vec<Option<Stack>>) {
        let slots = slots
            .into_iter()
            .map(|s| s.map(|s| Stack { net_id: self.fresh_id(), ..s }))
            .collect();
        self.open = Some(slots);
    }

    pub fn open_packet(&self, window: u32, container: ContainerEnumName) -> Option<Protocol> {
        use bedrock::protocol::v1001::packets::InventoryContentPacket;
        let slots = self.open.as_ref()?;
        Some(Protocol::InventoryContentPacket(Box::new(InventoryContentPacket {
            inventory_id: window,
            slots: slots.iter().map(|s| s.as_ref().map_or_else(crate::spawn::air, Stack::to_network)).collect(),
            container_name_data: FullContainerName { container, dynamic_id: None },
            storage_item: crate::spawn::air(),
        })))
    }

    /// Merges into a matching stack if there is one.
    fn transfer(&mut self, from: Place, to: Place, amount: i8) -> bool {
        let Some(src) = self.at(from).clone() else { return false };
        if amount <= 0 || amount > src.count {
            return false;
        }
        let moved = match self.at(to).clone() {
            None => {
                let net_id = if amount == src.count { src.net_id } else { self.fresh_id() };
                Stack { item: src.item.clone(), count: amount, net_id }
            }
            Some(dst) if dst.same_item(&src) && dst.count + amount <= crate::items::max_stack(dst.item.id) => {
                Stack { count: dst.count + amount, ..dst }
            }
            Some(_) => return false,
        };
        *self.at(to) = Some(moved);
        let left = src.count - amount;
        *self.at(from) = (left > 0).then_some(Stack { count: left, ..src });
        true
    }

    fn remove(&mut self, from: Place, amount: i8) -> bool {
        let Some(src) = self.at(from).clone() else { return false };
        if amount <= 0 || amount > src.count {
            return false;
        }
        let left = src.count - amount;
        *self.at(from) = (left > 0).then_some(Stack { count: left, ..src });
        true
    }

    /// Handles one client request. Any bad action rolls everything back and replies Error.
    pub fn handle(
        &mut self,
        req: &RequestsEntry<Protocol>,
        creative: impl Fn(u32) -> Option<NetworkItemStackDescriptorV2>,
    ) -> ItemStackResponseInfo<Protocol> {
        let backup = (self.slots.clone(), self.cursor.clone(), self.created.clone(), self.open.clone(), self.craft.clone());
        self.dropped.clear();
        self.spent_levels = 0;
        self.gained_xp = 0;
        // container slots only count while one is open, and only up to its size (27 or 54)
        self.traded.clear();
        let mut server_consumed = false;
        let open_len = self.open.as_ref().map_or(0, Vec::len);
        let valid = move |p: Option<Place>| p.filter(|p| !matches!(p, Place::Open(i) if *i >= open_len));
        // (client's container, slot, place) for everything we need to report back
        let mut touched: Vec<(FullContainerName<Protocol>, i8, Place)> = Vec::new();
        let mut touch = |info: &ItemStackRequestSlotInfo<Protocol>, p: Place| {
            if !touched.iter().any(|(_, s, q)| *q == p && *s == info.slot) {
                touched.push((info.container_name.clone(), info.slot, p));
            }
        };

        let mut ok = true;
        for action in &req.actions {
            ok &= match action {
                ItemStackRequestActionType::CraftCreative { creative_item_network_id, .. } => {
                    match creative(*creative_item_network_id) {
                        Some(item) => {
                            let net_id = self.fresh_id();
                            let count = crate::items::max_stack(item.id);
                            self.created = Some(Stack { item, count, net_id });
                            true
                        }
                        None => false,
                    }
                }
                ItemStackRequestActionType::Take { amount, source, destination }
                | ItemStackRequestActionType::Place { amount, source, destination } => {
                    match (valid(place_of(source)), valid(place_of(destination))) {
                        (Some(from), Some(to)) => {
                            touch(source, from);
                            touch(destination, to);
                            self.transfer(from, to, *amount)
                        }
                        _ => false,
                    }
                }
                ItemStackRequestActionType::Swap { source, destination } => {
                    match (valid(place_of(source)), valid(place_of(destination))) {
                        (Some(a), Some(b)) => {
                            touch(source, a);
                            touch(destination, b);
                            let x = self.at(a).take();
                            let y = std::mem::replace(self.at(b), x);
                            *self.at(a) = y;
                            true
                        }
                        _ => false,
                    }
                }
                ItemStackRequestActionType::Drop { amount, source, .. } => match valid(place_of(source)) {
                    Some(from) => {
                        touch(source, from);
                        let thrown = self.at(from).clone().map(|s| Stack { count: *amount, ..s });
                        let ok = self.remove(from, *amount);
                        if ok {
                            self.dropped.extend(thrown);
                        }
                        ok
                    }
                    None => false,
                },
                // after a trade the server already took the payment (PNX markServerConsumed)
                ItemStackRequestActionType::Consume { source, .. }
                    if server_consumed && matches!(place_of(source), Some(Place::Craft(TRADE_A | TRADE_B))) =>
                {
                    touch(source, place_of(source).unwrap());
                    true
                }
                ItemStackRequestActionType::Destroy { amount, source }
                | ItemStackRequestActionType::Consume { amount, source } => match valid(place_of(source)) {
                    Some(from) => {
                        touch(source, from);
                        self.remove(from, *amount)
                    }
                    None => false,
                },
                // TODO: check the grid against the recipe shape (anti-cheat). For now the client's
                // Consume actions still have to take real items from the grid.
                // enchanting table, using the option ids we sent
                ItemStackRequestActionType::CraftRecipe { recipe_network_id, .. } if self.offers.iter().any(|o| o.0 == *recipe_network_id) => {
                    let offer = self.offers.iter().find(|o| o.0 == *recipe_network_id).unwrap().1.clone();
                    let lapis = self.craft[ENCHANT_LAPIS].as_ref().filter(|s| crate::items::name_of(s.item.id) == Some("minecraft:lapis_lazuli")).map_or(0, |s| s.count as i32);
                    match self.craft[ENCHANT_INPUT].clone() {
                        Some(input) if self.creative || (self.levels >= offer.level && lapis >= offer.cost) => {
                            let net_id = self.fresh_id();
                            self.created = Some(Stack { item: crate::tables::enchanted(&input.item, &offer), count: 1, net_id });
                            self.spent_levels = if self.creative { 0 } else { offer.cost };
                            self.offers.clear();
                            true
                        }
                        _ => false,
                    }
                }
                // villager trade (PNX CraftRecipeActionProcessor), pay from the two trade slots
                ItemStackRequestActionType::CraftRecipe { recipe_network_id, number_of_requested_crafts }
                    if self.trading.as_ref().is_some_and(|(_, o)| o.iter().any(|o| o.net_id == *recipe_network_id)) =>
                {
                    let n = (*number_of_requested_crafts).max(1) as i32;
                    let offer = self.trading.as_ref().unwrap().1.iter().find(|o| o.net_id == *recipe_network_id).unwrap().clone();
                    let t = &offer.trade;
                    let pays = |slot: &Option<Stack>, want: &crate::trades::Stack| {
                        slot.as_ref().is_some_and(|s| crate::items::name_of(s.item.id) == Some(want.name.as_str()) && s.count as i32 >= want.count as i32 * n)
                    };
                    let ok = offer.uses + n <= t.max_uses
                        && pays(&self.craft[TRADE_A], &t.buy_a)
                        && t.buy_b.as_ref().is_none_or(|b| pays(&self.craft[TRADE_B], b));
                    if ok {
                        let take = |slot: &mut Option<Stack>, k: i32| {
                            if let Some(s) = slot.as_mut() {
                                s.count -= k as i8;
                                if s.count <= 0 {
                                    *slot = None;
                                }
                            }
                        };
                        take(&mut self.craft[TRADE_A], t.buy_a.count as i32 * n);
                        if let Some(b) = &t.buy_b {
                            take(&mut self.craft[TRADE_B], b.count as i32 * n);
                        }
                        let item = crate::villagers::to_item(&t.sell);
                        let net_id = self.fresh_id();
                        self.created = item.map(|item| Stack { item, count: (t.sell.count as i32 * n).min(64) as i8, net_id });
                        self.traded.push((offer.index, n, t.trader_exp));
                        if t.reward_exp {
                            self.gained_xp += 3 * n; // vanilla gives 3-6; PNX 1 per trade
                        }
                        if let Some((_, open)) = self.trading.as_mut()
                            && let Some(o) = open.iter_mut().find(|o| o.net_id == offer.net_id)
                        {
                            o.uses += n;
                        }
                        server_consumed = true;
                    }
                    ok && self.created.is_some()
                }
                // smithing: netherite upgrades keep enchants and damage (PNX SmithingRecipe),
                // trims add the template's pattern in that material
                ItemStackRequestActionType::CraftRecipe { recipe_network_id, .. }
                    if crate::recipes::get().smithing.contains_key(recipe_network_id) || crate::recipes::get().trims.contains(recipe_network_id) =>
                {
                    let r = crate::recipes::get();
                    match self.craft[SMITH_INPUT].clone() {
                        Some(base) => {
                            let item = match r.smithing.get(recipe_network_id) {
                                Some(result) => {
                                    let mut out = result.clone();
                                    out.user_data_buffer = base.item.user_data_buffer.clone();
                                    out
                                }
                                None => {
                                    let name = |s: &Option<Stack>| s.as_ref().and_then(|s| crate::items::name_of(s.item.id)).unwrap_or("").trim_start_matches("minecraft:").to_string();
                                    let pattern = name(&self.craft[SMITH_TEMPLATE]).replace("_armor_trim_smithing_template", "");
                                    let material = name(&self.craft[SMITH_MATERIAL]).replace("_ingot", "").replace("_shard", "").replace("_lazuli", "").replace("_brick", "");
                                    let mut d = crate::itemdata::ItemData::read(&base.item.user_data_buffer);
                                    d.set("Trim", crate::itemdata::Tag::Compound(vec![
                                        ("Material".into(), crate::itemdata::Tag::String(material)),
                                        ("Pattern".into(), crate::itemdata::Tag::String(pattern)),
                                    ]));
                                    let mut out = base.item.clone();
                                    out.user_data_buffer = d.write();
                                    out
                                }
                            };
                            let net_id = self.fresh_id();
                            self.created = Some(Stack { item, count: 1, net_id });
                            true
                        }
                        None => false,
                    }
                }
                // anvil rename comes in through the filtered strings
                ItemStackRequestActionType::CraftRecipeOptional { filtered_strings_index, .. } => {
                    let rename = usize::try_from(*filtered_strings_index).ok().and_then(|i| req.strings_to_filter.get(i)).map(String::as_str);
                    let material = self.craft[ANVIL_MATERIAL].as_ref().map(|s| (&s.item, s.count));
                    match self.craft[ANVIL_INPUT].as_ref().and_then(|t| crate::tables::anvil(&t.item, material, rename)) {
                        Some((item, _, cost)) if self.creative || (cost < 40 && self.levels >= cost) => {
                            let net_id = self.fresh_id();
                            self.created = Some(Stack { item, count: 1, net_id });
                            self.spent_levels = if self.creative { 0 } else { cost };
                            true
                        }
                        _ => false,
                    }
                }
                ItemStackRequestActionType::CraftRepairAndDisenchant { .. } => {
                    let a = self.craft[GRIND_INPUT].as_ref().map(|s| &s.item);
                    let b = self.craft[GRIND_EXTRA].as_ref().map(|s| &s.item);
                    match crate::tables::grindstone(a, b) {
                        Some((item, xp)) => {
                            let net_id = self.fresh_id();
                            self.created = Some(Stack { item, count: 1, net_id });
                            self.gained_xp = xp;
                            true
                        }
                        None => false,
                    }
                }
                ItemStackRequestActionType::CraftRecipe { recipe_network_id, number_of_requested_crafts }
                | ItemStackRequestActionType::CraftRecipeAuto { recipe_network_id, number_of_requested_crafts, .. } => {
                    match crate::recipes::get().crafted(*recipe_network_id) {
                        Some((item, n)) => {
                            let net_id = self.fresh_id();
                            let count = (n as i16 * (*number_of_requested_crafts).max(1) as i16).min(64) as i8;
                            self.created = Some(Stack { item, count, net_id });
                            true
                        }
                        None => false,
                    }
                }
                // client just repeats what the craft made, nothing to do
                #[allow(deprecated)]
                ItemStackRequestActionType::CraftResults { .. } => true,
                _ => false,
            };
            if !ok {
                break;
            }
        }
        // creative output slot is scratch space, never leave stuff in it
        self.created = None;

        if !ok {
            (self.slots, self.cursor, self.created, self.open, self.craft) = backup;
            self.dropped.clear();
            self.spent_levels = 0;
            self.gained_xp = 0;
            self.traded.clear();
            return ItemStackResponseInfo {
                result: ItemStackNetResult::Error,
                client_request_id: req.client_request_id,
                containers: None,
            };
        }

        // group changed slots by container using the client's names
        let mut containers: Vec<ItemStackResponseContainerInfo<Protocol>> = Vec::new();
        for (name, slot, place) in touched {
            if place == Place::Created {
                continue;
            }
            let stack = self.at(place).clone();
            let info = ItemStackResponseSlotInfo {
                requested_slot: slot,
                slot,
                amount: stack.as_ref().map_or(0, |s| s.count),
                item_stack_net_id: Some(stack.as_ref().map_or(0, |s| s.net_id)),
                custom_name: RedactableString { unredacted: String::new(), redacted: None },
                durability_correction: 0,
            };
            let key = format!("{:?}", name.container);
            match containers.iter_mut().find(|c| format!("{:?}", c.container_name.container) == key) {
                Some(c) => c.slots.push(info),
                None => containers.push(ItemStackResponseContainerInfo { container_name: name, slots: vec![info] }),
            }
        }
        ItemStackResponseInfo {
            result: ItemStackNetResult::Success,
            client_request_id: req.client_request_id,
            containers: Some(containers),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock::protocol::v662::enums::TextProcessingEventOrigin;

    fn item(id: i16) -> NetworkItemStackDescriptorV2 {
        NetworkItemStackDescriptorV2 { id, stack_size: 1, aux_value: 0, net_id: None, block_runtime_id: 0, user_data_buffer: vec![0; 10] }
    }

    fn at(container: ContainerEnumName, slot: i8) -> ItemStackRequestSlotInfo<Protocol> {
        ItemStackRequestSlotInfo { container_name: FullContainerName { container, dynamic_id: None }, slot, raw_id: 0 }
    }

    fn request(actions: Vec<ItemStackRequestActionType<Protocol>>) -> RequestsEntry<Protocol> {
        RequestsEntry { client_request_id: -1, actions, strings_to_filter: vec![], strings_to_filter_origin: TextProcessingEventOrigin::Unknown }
    }

    fn success(r: &ItemStackResponseInfo<Protocol>) -> bool {
        matches!(r.result, ItemStackNetResult::Success)
    }

    #[test]
    fn creative_take_then_place_in_hotbar() {
        let mut inv = Inventory::default();
        // same as the PNX recording: craft creative, take 64 to cursor
        let r = inv.handle(
            &request(vec![
                ItemStackRequestActionType::CraftCreative { creative_item_network_id: 5, number_of_requested_crafts: 1 },
                ItemStackRequestActionType::Take {
                    amount: 64,
                    source: at(ContainerEnumName::CreatedOutputContainer, 50),
                    destination: at(ContainerEnumName::CursorContainer, 0),
                },
            ]),
            |_| Some(item(5)),
        );
        assert!(success(&r));
        let cursor_report = &r.containers.as_ref().unwrap()[0].slots[0];
        assert_eq!(cursor_report.amount, 64);

        let r = inv.handle(
            &request(vec![ItemStackRequestActionType::Place {
                amount: 64,
                source: at(ContainerEnumName::CursorContainer, 0),
                destination: at(ContainerEnumName::HotbarContainer, 2),
            }]),
            |_| None,
        );
        assert!(success(&r));
        assert_eq!(inv.slots[2].as_ref().unwrap().count, 64);
        assert!(inv.cursor.is_none());
        assert_eq!(r.containers.unwrap().len(), 2, "cursor and hotbar both reported");
    }

    #[test]
    fn armor_offhand_and_cursor_return() {
        let mut inv = Inventory::default();
        inv.set_slot(0, item(300), 1);
        // Drag a helmet into armor slot 0, then something into the offhand.
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Place {
            amount: 1, source: at(ContainerEnumName::HotbarContainer, 0), destination: at(ContainerEnumName::ArmorContainer, 0),
        }]), |_| None)));
        assert_eq!(inv.slots[ARMOR].as_ref().unwrap().item.id, 300);
        inv.set_slot(1, item(5), 3);
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Swap {
            source: at(ContainerEnumName::HotbarContainer, 1), destination: at(ContainerEnumName::OffhandContainer, 0),
        }]), |_| None)));
        assert_eq!(inv.slots[OFFHAND].as_ref().unwrap().count, 3);
        // leave 2 on the cursor and close, they should merge back in
        inv.set_slot(2, item(9), 10);
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Take {
            amount: 2, source: at(ContainerEnumName::HotbarContainer, 2), destination: at(ContainerEnumName::CursorContainer, 0),
        }]), |_| None)));
        assert!(inv.return_cursor());
        assert_eq!(inv.slots[2].as_ref().unwrap().count, 10);
        // Armor and offhand survive a save.
        let back = Inventory::from_bytes(&inv.to_bytes()).unwrap();
        assert_eq!(back.slots[ARMOR].as_ref().unwrap().item.id, 300);
        assert_eq!(back.slots[OFFHAND].as_ref().unwrap().count, 3);
        // old 36-slot saves still load
        let mut old = vec![0u8; MAIN]; // 36 empty slots, the pre-armor format
        old[0] = 1;
        old.splice(1..1, [9, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0]); // slot 0: item 9 x4
        let loaded = Inventory::from_bytes(&old).unwrap();
        assert_eq!(loaded.slots[0].as_ref().unwrap().count, 4);
        assert!(loaded.slots[ARMOR].is_none());
    }

    #[test]
    fn chest_moves_and_requires_open_chest() {
        let mut inv = Inventory::default();
        inv.set_slot(0, item(5), 10);
        let to_chest = request(vec![ItemStackRequestActionType::Place {
            amount: 4, source: at(ContainerEnumName::HotbarContainer, 0), destination: at(ContainerEnumName::LevelEntityContainer, 3),
        }]);
        assert!(!success(&inv.handle(&to_chest, |_| None)), "no chest open");
        inv.open_container(vec![None; CHEST_SLOTS]);
        assert!(success(&inv.handle(&to_chest, |_| None)));
        let chest = inv.open.take().unwrap();
        assert_eq!(chest[3].as_ref().unwrap().count, 4);
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 6);
        // chest contents round-trip (same encoding as containers.dat)
        let (back, used) = decode_slots(&encode_slots(&chest), CHEST_SLOTS, CHEST_SLOTS).unwrap();
        assert_eq!(used, encode_slots(&chest).len());
        assert_eq!(back[3].as_ref().unwrap().count, 4);
    }

    #[test]
    fn tools_wear_break_and_keep_damage_when_saved() {
        let mut inv = Inventory::default();
        let pick = crate::items::item_by_name("minecraft:golden_pickaxe").unwrap();
        inv.set_slot(0, pick, 1);
        assert_eq!(inv.wear(0, 10), Some(false));
        assert_eq!(crate::items::damage(&inv.held(0).unwrap().item), 10);
        let (back, _) = decode_slots(&encode_slots(&inv.slots), inv.slots.len(), 0).unwrap();
        assert_eq!(crate::items::damage(&back[0].as_ref().unwrap().item), 10, "damage survives a save");
        assert_eq!(inv.wear(0, 22), Some(true), "32 uses: broken");
        assert!(inv.held(0).is_none());
        inv.set_slot(1, crate::items::item_by_name("minecraft:dirt").unwrap(), 5);
        assert_eq!(inv.wear(1, 1), None, "dirt has no durability");
    }

    #[test]
    fn save_round_trip() {
        let mut inv = Inventory::default();
        inv.set_slot(3, item(7), 12);
        inv.set_slot(35, item(-5), 1);
        let back = Inventory::from_bytes(&inv.to_bytes()).unwrap();
        assert_eq!(back.slots[3].as_ref().unwrap().count, 12);
        assert_eq!(back.slots[35].as_ref().unwrap().item.id, -5);
        assert!(back.slots[0].is_none());
        assert!(Inventory::from_bytes(&[1, 2]).is_none(), "truncated data is rejected");
    }

    #[test]
    fn split_merge_swap_and_destroy() {
        let mut inv = Inventory::default();
        inv.set_slot(0, item(1), 10);
        inv.set_slot(1, item(2), 5);
        // take half of slot 0, drop it into slot 9
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Take {
            amount: 5, source: at(ContainerEnumName::HotbarContainer, 0), destination: at(ContainerEnumName::CursorContainer, 0),
        }]), |_| None)));
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Place {
            amount: 5, source: at(ContainerEnumName::CursorContainer, 0), destination: at(ContainerEnumName::InventoryContainer, 9),
        }]), |_| None)));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 5);
        assert_eq!(inv.slots[9].as_ref().unwrap().count, 5);
        assert_ne!(inv.slots[0].as_ref().unwrap().net_id, inv.slots[9].as_ref().unwrap().net_id);
        // different items can't merge, whole request fails
        let r = inv.handle(&request(vec![ItemStackRequestActionType::Place {
            amount: 5, source: at(ContainerEnumName::HotbarContainer, 0), destination: at(ContainerEnumName::HotbarContainer, 1),
        }]), |_| None);
        assert!(!success(&r));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 5);
        // but swapping them is fine
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Swap {
            source: at(ContainerEnumName::HotbarContainer, 0), destination: at(ContainerEnumName::HotbarContainer, 1),
        }]), |_| None)));
        assert_eq!(inv.slots[0].as_ref().unwrap().item.id, 2);
        // creative trash
        assert!(success(&inv.handle(&request(vec![ItemStackRequestActionType::Destroy {
            amount: 5, source: at(ContainerEnumName::HotbarContainer, 0),
        }]), |_| None)));
        assert!(inv.slots[0].is_none());
    }
}

#[cfg(test)]
mod totem_tests {
    use super::*;

    #[test]
    fn totem_in_offhand_is_used_once() {
        crate::blocks::init();
        let mut inv = Inventory::default();
        assert!(!inv.use_totem());
        inv.set_slot(OFFHAND, crate::items::item_by_name("minecraft:totem_of_undying").unwrap(), 1);
        assert!(inv.use_totem());
        assert!(!inv.use_totem(), "only one");
        // A shield only counts while sneaking.
        inv.set_slot(OFFHAND, crate::items::item_by_name("minecraft:shield").unwrap(), 1);
        assert_eq!(inv.shield(), None);
        inv.sneaking = true;
        assert_eq!(inv.shield(), Some(OFFHAND));
    }
}
