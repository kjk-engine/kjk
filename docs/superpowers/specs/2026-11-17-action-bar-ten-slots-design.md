# Ten action bar slots

Approved by the owner on 2026-11-17.

## Intent

The action bar has six slots, on keys 1 to 6. A player with many powers and items wants more of them on keys. The bar gets four more slots, on keys 7, 8, 9 and 0. A player who never assigns anything to the new slots sees the same six-slot bar as before.

## Goals

- The bar has ten slots. Slots 7 to 10 start empty.
- The player can assign anything to slots 7 to 10 that can be assigned to slots 1 to 6.
- The keys of slots 7 to 10 can be rebound, including to mouse buttons. Their defaults are 7, 8, 9 and 0. These four keys have no default binding today.
- The HUD always shows slots 1 to 6. It shows the extra slots only up to the last one that has something assigned. If only slot 9 is assigned, the HUD shows slots 7 and 8 as empty boxes, so each box stays under its own key.
- The bar setup screen always shows all ten slots.
- A save made before this change loads with slots 7 to 10 empty.

## Non-goals

- The controller keeps four slots. Its cross-shaped bar does not change.
- The game never fills slots 7 to 10 on its own. The automatic fill keeps using the first four slots.
- The power wheel does not change. It keeps listing what is not on the bar.

## Approaches

1. Ten real slots, with the HUD hiding the empty ones past the last assigned slot. This is the chosen approach. Each box stays in the position of its key.
2. Ten real slots, with the HUD showing only the assigned extra slots and skipping the empty ones between them. The bar is shorter, but a box no longer sits in the position of its key. The owner chose approach 1.
3. A second bar page switched with a key. This adds a mode to remember in a fight, and the owner asked for direct keys.

## How

- `sim::combat::bar::SLOTS` becomes 10. `FILLED` stays 4.
- The snapshot format gets version 110. A snapshot of version 100 to 109 has six slots, and its bar is read with slots 7 to 10 empty.
- `settings::Action` gets `Bar7` to `Bar0`, and `settings::BAR` has ten entries. Their setting names are `bar_7`, `bar_8`, `bar_9` and `bar_10`.
- The HUD's count of shown slots is `max(6, last assigned slot)` on the keyboard and `PAD_BAR` on a controller.
- In a conversation, the keys of slots 7 to 10 are kept from the bar in the same way as the first six.

## Left to the builder

- The exact width of the bar's row of boxes on the HUD and on the setup screen when ten slots are shown.
