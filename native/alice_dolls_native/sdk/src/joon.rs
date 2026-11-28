//! Joon: the trading doll. Her owner sets her trades, as a villager's (something for something,
//! either side may be left empty for a gift), fills her with the stock to pay them out of, and
//! sends her to a player. She shows up at their side with a trade offer, follows them for 200
//! seconds, can't be hurt while she is out, and comes home when the time is up or they send her
//! back; she isn't sent to the same player again for 300 seconds. What she is paid goes into her
//! own inventory.
//!
//! Her screen is the mod's own (`joon_ui`); what it does on the server is `joon_play`.

use crate::stacks::DollStack as ItemStack;
use mod_api::Tag;

/// The most trades she holds: two columns of six.
pub const MAX_TRADES: usize = 12;
/// How long a visit lasts.
pub const VISIT_TICKS: u32 = 200 * 20;
/// How long before she is sent to the same player again, after a visit to them.
pub const COOLDOWN_SECS: u64 = 300;
/// Her storage: a large chest.
pub const SLOTS: usize = 54;

/// Stacks a trade takes, and stacks it gives.
pub const PAY: usize = 4;
pub const GET: usize = 4;
/// A trade's parts, as the screen and its acts number them: what is paid first, then what is got.
pub const PARTS: usize = PAY + GET;

/// One trade: what the visited player pays (up to four stacks) and what they get (up to four).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trade {
    pub cost: [Option<ItemStack>; PAY],
    pub result: [Option<ItemStack>; GET],
}

impl Trade {
    pub fn is_set(&self) -> bool {
        self.result.iter().chain(&self.cost).any(Option::is_some)
    }

    /// What the player gets, or, for a trade that only takes, the first thing it takes: what the
    /// list of trades shows for it.
    pub fn face(&self) -> Option<&ItemStack> {
        self.result.iter().chain(&self.cost).flatten().next()
    }

    /// Part `part` (`0..PAY` paid, then got).
    pub fn part(&self, part: usize) -> Option<&ItemStack> {
        if part < PAY { self.cost[part].as_ref() } else { self.result.get(part - PAY)?.as_ref() }
    }

    pub fn part_mut(&mut self, part: usize) -> Option<&mut Option<ItemStack>> {
        if part < PAY { Some(&mut self.cost[part]) } else { self.result.get_mut(part - PAY) }
    }

    /// Every part, in order.
    pub fn parts(&self) -> [Option<ItemStack>; PARTS] {
        std::array::from_fn(|p| self.part(p).cloned())
    }

    pub fn from_parts(mut parts: Vec<Option<ItemStack>>) -> Trade {
        parts.resize(PARTS, None);
        let mut t = Trade::default();
        for (p, s) in parts.into_iter().enumerate() {
            if let Some(slot) = t.part_mut(p) {
                *slot = s;
            }
        }
        t
    }
}

/// Stacks of the same item put together.
fn summed<'a>(stacks: impl Iterator<Item = &'a ItemStack>) -> Vec<ItemStack> {
    let mut out: Vec<ItemStack> = Vec::new();
    for s in stacks {
        match out.iter_mut().find(|n| same(n, s)) {
            Some(n) => n.count += s.count,
            None => out.push(s.clone()),
        }
    }
    out
}

/// The same item: id, wear and components, whatever the count.
pub fn same(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.damage == b.damage && a.extra == b.extra
}

pub const BOOK: &str = "minecraft:enchanted_book";

/// Whether `have` pays for `want` in a trade. An enchanted book asked for pays with any enchanted
/// book that holds each of its enchantments at that level or higher, whatever else it holds or
/// however it was named or repaired (a book asking for none takes any enchanted book); anything
/// else, only the same item.
pub fn pays(have: &ItemStack, want: &ItemStack) -> bool {
    if want.item != BOOK {
        return same(have, want);
    }
    if have.item != BOOK {
        return false;
    }
    let held = have.stored_enchantments();
    want.stored_enchantments()
        .iter()
        .all(|(id, level)| held.iter().any(|(h, l)| h == id && l >= level))
}

/// What a trade asks for when `stack` is put in a price slot: an enchanted book becomes just its
/// enchantments (not its name or repair cost), so any book with them pays; anything else as it is.
pub fn asked_for(stack: ItemStack) -> ItemStack {
    if stack.item != BOOK {
        return stack;
    }
    let mut book = ItemStack::book_of(&stack.stored_enchantments());
    book.count = stack.count;
    book
}

/// An enchanted book's enchantments as a player reads them: "Sharpness III, Mending". Empty for
/// anything else.
pub fn book_note(s: &ItemStack) -> String {
    if s.item != BOOK {
        return String::new();
    }
    s.stored_enchantments()
        .iter()
        .map(|(id, level)| {
            let name = id.rsplit(':').next().unwrap_or(id);
            let mut name: String = name
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ");
            if *level > 1 {
                name.push(' ');
                name.push_str(&roman(*level));
            }
            name
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn roman(n: u32) -> String {
    const R: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
    R.get(n as usize - 1).map_or_else(|| n.to_string(), |r| (*r).to_owned())
}

/// How many of `want` are in `items`.
pub fn count_in(items: &[Option<ItemStack>], want: &ItemStack) -> u32 {
    items.iter().flatten().filter(|s| same(s, want)).map(|s| s.count).sum()
}

/// What the player pays for `trade` once, out of `have`: for each slot, how many it gives. Each
/// thing asked for takes from the stacks that pay for it, in slot order; None when they can't pay.
pub fn payment(have: &[Option<ItemStack>], trade: &Trade) -> Option<Vec<(usize, u32)>> {
    let mut left: Vec<u32> = have.iter().map(|s| s.as_ref().map_or(0, |s| s.count)).collect();
    let mut out: Vec<(usize, u32)> = Vec::new();
    for want in trade.cost.iter().flatten() {
        let mut need = want.count;
        for (i, s) in have.iter().enumerate() {
            if need == 0 {
                break;
            }
            let Some(s) = s else { continue };
            if left[i] == 0 || !pays(s, want) {
                continue;
            }
            let k = left[i].min(need);
            left[i] -= k;
            need -= k;
            match out.iter_mut().find(|(slot, _)| *slot == i) {
                Some((_, n)) => *n += k,
                None => out.push((i, k)),
            }
        }
        if need > 0 {
            return None;
        }
    }
    Some(out)
}

/// Takes `n` of `want` out of `items`, from the last slot back; all or nothing.
pub fn take_from(items: &mut [Option<ItemStack>], want: &ItemStack, n: u32) -> bool {
    if count_in(items, want) < n {
        return false;
    }
    let mut left = n;
    for slot in items.iter_mut().rev() {
        if left == 0 {
            break;
        }
        let Some(s) = slot else { continue };
        if !same(s, want) {
            continue;
        }
        let k = s.count.min(left);
        s.count -= k;
        left -= k;
        if s.count == 0 {
            *slot = None;
        }
    }
    true
}

/// Puts `stack` into `items`, onto stacks like it (up to `max` a slot) and then into empty slots;
/// what doesn't fit comes back.
pub fn put_into(items: &mut [Option<ItemStack>], mut stack: ItemStack, max: u32) -> Option<ItemStack> {
    let max = max.max(1);
    for slot in items.iter_mut().flatten() {
        if stack.count == 0 {
            return None;
        }
        if same(slot, &stack) && slot.count < max {
            let k = (max - slot.count).min(stack.count);
            slot.count += k;
            stack.count -= k;
        }
    }
    for slot in items.iter_mut() {
        if stack.count == 0 {
            return None;
        }
        if slot.is_none() {
            let mut s = stack.clone();
            s.count = stack.count.min(max);
            stack.count -= s.count;
            *slot = Some(s);
        }
    }
    (stack.count > 0).then_some(stack)
}

/// Whether her stock pays `trade` out once.
pub fn in_stock(items: &[Option<ItemStack>], trade: &Trade) -> bool {
    stock_for(items, trade).is_none_or(|n| n > 0)
}

/// How many times her stock pays `trade` out; None when it pays nothing out.
pub fn stock_for(items: &[Option<ItemStack>], trade: &Trade) -> Option<u32> {
    summed(trade.result.iter().flatten()).iter().map(|r| count_in(items, r) / r.count.max(1)).min()
}

/// Her side of one trade: what it pays out comes out of `items` and what she is paid (`paid`, the
/// very stacks the player gave) goes in. `max` is an item's stack size. Unchanged, and Err with
/// why, when it can't be done.
pub fn joon_side(items: &mut [Option<ItemStack>], trade: &Trade, paid: &[ItemStack], max: impl Fn(&str) -> u32) -> Result<(), &'static str> {
    let mut after = items.to_vec();
    for r in summed(trade.result.iter().flatten()) {
        if !take_from(&mut after, &r, r.count) {
            return Err("out of stock");
        }
    }
    for c in paid {
        if put_into(&mut after, c.clone(), max(&c.item)).is_some() {
            return Err("full");
        }
    }
    items.clone_from_slice(&after);
    Ok(())
}

/// Whether `have` (the player's inventory) holds what `trade` takes.
pub fn can_pay(have: &[Option<ItemStack>], trade: &Trade) -> bool {
    payment(have, trade).is_some()
}

/// The stacks `payment` takes out of `have`, as they are (a book with all its enchantments).
pub fn paid_stacks(have: &[Option<ItemStack>], payment: &[(usize, u32)]) -> Vec<ItemStack> {
    payment
        .iter()
        .filter_map(|&(i, n)| {
            let mut s = have.get(i)?.clone()?;
            s.count = n;
            Some(s)
        })
        .collect()
}

// ---- Saving ----

/// The order a trade's parts are saved in: the three of the first trades (two paid, one got),
/// where they always were, then the rest.
const SAVED_ORDER: [usize; PARTS] = [0, 1, PAY, 2, 3, PAY + 1, PAY + 2, PAY + 3];
/// Saved places a trade had before it had more parts.
const OLD_PARTS: usize = 3;

fn saved(t: &Trade) -> Vec<Option<ItemStack>> {
    SAVED_ORDER.iter().map(|&p| t.part(p).cloned()).collect()
}

fn from_saved(mut s: Vec<Option<ItemStack>>) -> Trade {
    s.resize(PARTS, None);
    let mut parts = vec![None; PARTS];
    for (k, stack) in s.into_iter().enumerate() {
        parts[SAVED_ORDER[k]] = stack;
    }
    Trade::from_parts(parts)
}

/// Her trades as saved with her: a list of compounds, each its stacks by slot (`SAVED_ORDER`).
pub fn encode_trades(trades: &[Trade]) -> Tag {
    Tag::List(
        trades
            .iter()
            .map(|t| Tag::Compound([("Items".into(), crate::stacks::encode_items(&saved(t)))].into()))
            .collect(),
    )
}

pub fn decode_trades(tag: Option<&Tag>) -> Vec<Trade> {
    let Some(Tag::List(list)) = tag else { return Vec::new() };
    list.iter().take(MAX_TRADES).map(|t| from_saved(crate::stacks::decode_items(t.get("Items"), PARTS))).collect()
}

/// Where a doll item keeps her trades, in its container: three slots a trade, past its baubles and
/// before its favour.
pub const TRADES_IN_ITEM: usize = 210;
/// The rest of each trade's parts ride in one more stack's own container, here: the slots under
/// 200 are her inventory's, and there is no room for them past it.
pub const MORE_TRADES_IN_ITEM: u8 = (TRADES_IN_ITEM + MAX_TRADES * OLD_PARTS) as u8;
const MORE: usize = PARTS - OLD_PARTS;

/// Her trades, as her item carries them.
pub fn trades_to_item(trades: &[Trade]) -> Vec<(u8, ItemStack)> {
    let mut out = Vec::new();
    let mut more = Vec::new();
    for (i, t) in trades.iter().take(MAX_TRADES).enumerate() {
        for (k, s) in saved(t).into_iter().enumerate() {
            let Some(s) = s else { continue };
            if k < OLD_PARTS {
                out.push(((TRADES_IN_ITEM + i * OLD_PARTS + k) as u8, s));
            } else {
                more.push(((i * MORE + k - OLD_PARTS) as u8, s));
            }
        }
    }
    if !more.is_empty() {
        let mut holder = ItemStack::new("minecraft:paper", 1);
        holder.extra = Some(Box::new(crate::stacks::StackData { container: more, ..Default::default() }));
        out.push((MORE_TRADES_IN_ITEM, holder));
    }
    out
}

pub fn trades_from_item(container: &[(u8, ItemStack)]) -> Vec<Trade> {
    let mut saved = vec![vec![None; PARTS]; MAX_TRADES];
    for (slot, s) in container {
        if *slot == MORE_TRADES_IN_ITEM {
            for (k, part) in s.container() {
                let k = usize::from(*k);
                if k < MAX_TRADES * MORE {
                    saved[k / MORE][OLD_PARTS + k % MORE] = Some(part.clone());
                }
            }
            continue;
        }
        let Some(at) = usize::from(*slot).checked_sub(TRADES_IN_ITEM).filter(|&a| a < MAX_TRADES * OLD_PARTS) else { continue };
        saved[at / OLD_PARTS][at % OLD_PARTS] = Some(s.clone());
    }
    let mut trades: Vec<Trade> = saved.into_iter().map(from_saved).collect();
    while trades.last().is_some_and(|t| !t.is_set()) {
        trades.pop();
    }
    trades
}

// ---- The screens ----

/// A player she can be sent to, as the list shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct Visitee {
    pub uuid: u128,
    pub name: String,
    /// Seconds before she can go to them again; 0 now.
    pub wait: u32,
    pub you: bool,
}

/// Seconds as m:ss.
pub fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stacks::StackData;

    fn st(id: &str, n: u32) -> ItemStack {
        ItemStack::new(id, n)
    }

    /// A trade of what is paid for what is got.
    fn tr(pay: &[ItemStack], get: &[ItemStack]) -> Trade {
        let mut t = Trade::default();
        for (k, s) in pay.iter().enumerate() {
            t.cost[k] = Some(s.clone());
        }
        for (k, s) in get.iter().enumerate() {
            t.result[k] = Some(s.clone());
        }
        t
    }

    /// What a trade asks for, as the player pays it.
    fn costs(t: &Trade) -> Vec<ItemStack> {
        t.cost.iter().flatten().cloned().collect()
    }

    fn book(e: &[(&str, u32)]) -> ItemStack {
        ItemStack::book_of(&e.iter().map(|(id, l)| ((*id).to_owned(), *l)).collect::<Vec<_>>())
    }

    #[test]
    fn a_trade_can_ask_for_a_given_enchanted_book() {
        let sharpness3 = book(&[("minecraft:sharpness", 3)]);
        // Saved and read back, it keeps its enchantment.
        let back = crate::stacks::decode_items(Some(&crate::stacks::encode_items(&[Some(sharpness3.clone())])), 1)[0].clone().unwrap();
        assert_eq!(back.stored_enchantments(), vec![("minecraft:sharpness".to_owned(), 3)]);
        assert_eq!(book_note(&sharpness3), "Sharpness III");
        assert_eq!(book_note(&book(&[("minecraft:mending", 1), ("minecraft:fire_aspect", 2)])), "Fire Aspect II, Mending");
        // That level or higher pays, with anything else on it; a lower level, another
        // enchantment or a plain book doesn't.
        assert!(pays(&book(&[("minecraft:sharpness", 3)]), &sharpness3));
        assert!(pays(&book(&[("minecraft:sharpness", 5), ("minecraft:mending", 1)]), &sharpness3));
        assert!(!pays(&book(&[("minecraft:sharpness", 2)]), &sharpness3));
        assert!(!pays(&book(&[("minecraft:smite", 3)]), &sharpness3));
        assert!(!pays(&st("minecraft:book", 1), &sharpness3));
        // A book asking for nothing takes any enchanted book.
        assert!(pays(&book(&[("minecraft:smite", 1)]), &st(BOOK, 1)));
        // Put in a price slot, a book asks for its enchantments alone, not its repair cost.
        let mut worn = book(&[("minecraft:mending", 1)]);
        worn.extra.as_mut().unwrap().opaque_components.insert("minecraft:repair_cost".into(), mod_api::Tag::Int(3));
        let asked = asked_for(worn.clone());
        assert_eq!(asked, book(&[("minecraft:mending", 1)]));
        assert!(pays(&worn, &asked));
        // The player pays from the slot with a good book, and Joon gets that very book.
        let t = tr(&[st("minecraft:emerald", 2), sharpness3.clone()], &[st("minecraft:diamond", 1)]);
        let have = vec![
            Some(book(&[("minecraft:sharpness", 1)])),
            Some(st("minecraft:emerald", 5)),
            Some(book(&[("minecraft:sharpness", 4), ("minecraft:looting", 2)])),
        ];
        let pay = payment(&have, &t).unwrap();
        assert_eq!(pay, vec![(1, 2), (2, 1)]);
        let paid = paid_stacks(&have, &pay);
        assert_eq!(paid[1], book(&[("minecraft:sharpness", 4), ("minecraft:looting", 2)]));
        let mut stock = vec![Some(st("minecraft:diamond", 3)), None, None];
        assert_eq!(joon_side(&mut stock, &t, &paid, |_| 64), Ok(()));
        assert!(stock.iter().flatten().any(|s| *s == paid[1]));
        assert!(payment(&have[..2], &t).is_none());
    }

    fn emeralds_for_bread() -> Trade {
        tr(&[st("minecraft:emerald", 2)], &[st("minecraft:bread", 6)])
    }

    #[test]
    fn a_trade_takes_and_gives_several_stacks_at_once() {
        let t = tr(
            &[st("minecraft:emerald", 3), st("minecraft:gold_ingot", 2), st("minecraft:emerald", 1)],
            &[st("minecraft:bread", 6), st("minecraft:apple", 2), st("minecraft:bread", 2)],
        );
        // The two emerald costs are paid from the same stacks.
        assert!(!can_pay(&[Some(st("minecraft:emerald", 3)), Some(st("minecraft:gold_ingot", 2))], &t));
        assert!(can_pay(&[Some(st("minecraft:emerald", 4)), Some(st("minecraft:gold_ingot", 2))], &t));
        let mut stock = vec![None; 6];
        stock[0] = Some(st("minecraft:bread", 17));
        stock[1] = Some(st("minecraft:apple", 5));
        // Eight loaves a trade: two from her 17; the apples allow two too.
        assert_eq!(stock_for(&stock, &t), Some(2));
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Ok(()));
        assert_eq!(count_in(&stock, &st("minecraft:bread", 1)), 9);
        assert_eq!(count_in(&stock, &st("minecraft:apple", 1)), 3);
        assert_eq!(count_in(&stock, &st("minecraft:emerald", 1)), 4);
        assert_eq!(count_in(&stock, &st("minecraft:gold_ingot", 1)), 2);
        assert_eq!(stock_for(&stock, &t), Some(1));
        stock[1] = Some(st("minecraft:apple", 1));
        assert!(!in_stock(&stock, &t));
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Err("out of stock"));
    }

    #[test]
    fn a_trade_saved_before_it_had_more_parts_reads_back() {
        // 0.44.2's item: three slots a trade from 210, nothing more.
        let old = vec![(210u8, st("minecraft:emerald", 2)), (212u8, st("minecraft:bread", 6)), (215u8, st("minecraft:cake", 1))];
        assert_eq!(trades_from_item(&old), vec![emeralds_for_bread(), tr(&[], &[st("minecraft:cake", 1)])]);
        let old_tag = Tag::List(vec![Tag::Compound(
            [("Items".into(), crate::stacks::encode_items(&[Some(st("minecraft:emerald", 2)), None, Some(st("minecraft:bread", 6))]))].into(),
        )]);
        assert_eq!(decode_trades(Some(&old_tag)), vec![emeralds_for_bread()]);
    }

    #[test]
    fn she_pays_out_of_her_stock_and_keeps_what_she_is_paid() {
        let mut stock = vec![None; SLOTS];
        stock[0] = Some(st("minecraft:bread", 10));
        let t = emeralds_for_bread();
        assert!(in_stock(&stock, &t));
        assert_eq!(stock_for(&stock, &t), Some(1));
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Ok(()));
        assert_eq!(count_in(&stock, &st("minecraft:bread", 1)), 4);
        assert_eq!(count_in(&stock, &st("minecraft:emerald", 1)), 2);
        // Four loaves left: not enough for six. Nothing changes.
        let before = stock.clone();
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Err("out of stock"));
        assert!(!in_stock(&stock, &t));
        assert_eq!(stock, before);
    }

    #[test]
    fn a_full_joon_takes_nothing() {
        let mut stock: Vec<Option<ItemStack>> = (0..SLOTS).map(|_| Some(st("minecraft:dirt", 64))).collect();
        stock[0] = Some(st("minecraft:bread", 6));
        let t = emeralds_for_bread();
        // The bread's slot empties, so the emeralds go there.
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Ok(()));
        stock[0] = Some(st("minecraft:dirt", 64));
        stock[1] = Some(st("minecraft:bread", 12));
        let before = stock.clone();
        assert_eq!(joon_side(&mut stock, &t, &costs(&t), |_| 64), Err("full"));
        assert_eq!(stock, before);
    }

    #[test]
    fn gifts_either_way() {
        let gift = tr(&[], &[st("minecraft:cake", 1)]);
        let tip = tr(&[st("minecraft:emerald", 1)], &[]);
        assert!(gift.is_set() && tip.is_set() && !Trade::default().is_set());
        assert!(can_pay(&[], &gift));
        assert!(in_stock(&[], &tip));
        let mut stock = vec![None; 3];
        assert_eq!(joon_side(&mut stock, &tip, &costs(&tip), |_| 64), Ok(()));
        assert_eq!(stock[0], Some(st("minecraft:emerald", 1)));
        assert_eq!(tip.face(), Some(&st("minecraft:emerald", 1)));
    }

    #[test]
    fn the_player_pays_both_costs_of_one_item_from_the_same_stacks() {
        let t = tr(&[st("minecraft:emerald", 40), st("minecraft:emerald", 30)], &[]);
        assert!(!can_pay(&[Some(st("minecraft:emerald", 64))], &t));
        assert!(can_pay(&[Some(st("minecraft:emerald", 64)), Some(st("minecraft:emerald", 6))], &t));
    }

    #[test]
    fn an_item_is_the_same_only_with_the_same_components() {
        let mut named = st("minecraft:emerald", 1);
        named.extra = Some(Box::new(StackData { enchantments: vec![("minecraft:unbreaking".into(), 1)], ..StackData::default() }));
        assert!(!same(&named, &st("minecraft:emerald", 1)));
        assert_eq!(count_in(&[Some(named.clone())], &st("minecraft:emerald", 1)), 0);
    }

    #[test]
    fn her_trades_are_saved_with_her_and_in_her_item() {
        let full = Trade::from_parts((0..PARTS).map(|p| Some(st("minecraft:dirt", p as u32 + 1))).collect());
        let mut diamond = Trade::default();
        diamond.cost[1] = Some(st("minecraft:diamond", 1));
        let trades = vec![emeralds_for_bread(), Trade::default(), diamond, full];
        let back = decode_trades(Some(&encode_trades(&trades)));
        assert_eq!(back, trades);
        let in_item = trades_to_item(&trades);
        assert!(in_item.iter().all(|(s, _)| (210..250).contains(s)));
        let mut twelve = vec![trades[3].clone(); MAX_TRADES];
        twelve[11] = emeralds_for_bread();
        assert_eq!(trades_from_item(&trades_to_item(&twelve)), twelve);
        assert_eq!(trades_from_item(&in_item), trades);
        assert_eq!(trades_from_item(&[]), Vec::new());
        assert_eq!(decode_trades(None), Vec::new());
    }

    #[test]
    fn a_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(200), "3:20");
        assert_eq!(clock(5), "0:05");
    }
}
