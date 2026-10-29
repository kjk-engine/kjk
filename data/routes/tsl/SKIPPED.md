# TSL chain: what the walkthrough routes skip

This lists everything the TSL chain routes leave out, so each item can get
its own test route later. The main chain plays one light-side path; every
side quest, skill-gated branch, dark-side or alternative choice and
workaround below still has to be shown to work.

Each milestone's PR adds its own items here. Tick an item when a route in
`data/routes/tsl/` plays it (name the route) or when it is judged not to
need one (say why).

The **kind** tags are:
- `side` is an optional quest or encounter;
- `skill` is a branch gated by a skill or attribute the chain's Exile lacks;
- `choice` is a reply or path not taken (dark side, the other companion, paid instead of persuaded);
- `feature` is engine support that is missing;
- `blocker` is a route workaround for an engine bug (those need the engine fix, then the route undone).

## Peragus (PR 11)

- [ ] `side` **peragus_t3:** the broken droid and the footlocker on T3's way through the fuel depot hallway, not found by tag.
- [ ] `choice` **Persuade:** the ways that need it, skipped throughout (the route header says "the way without Persuade").
- [ ] `choice` **Dark side and other replies:** HK-50's talk, the Harbinger's crew, Atton's interrogation.
- [ ] `feature` **The turret:** the party's AI keeps running during the 107per turret. That's not reproduced as a bug, but a test should pin it down.

## Telos (PR 14)

- [ ] `blocker` **telos_base_shuttle:** the route leaves the bay control room by `TelosDoor2@7`/`@6`. The nav mesh blocks a face only when most of it lies under a placeable, so it plans between two barrels in the south doorway.
- [ ] `side` **Ramana**'s quest.
- [ ] `side` **The pazaak and swoop records** on Citadel Station.
- [ ] `side` **Suspicious Goods.**
- [ ] `side` **The Czerka escort.**
- [ ] `side` **The Handmaiden duels and sparring** at the academy.
- [ ] `side` **Kreia's and Atton's influence talks.**
- [ ] `skill` **The Czerka console's [Computer] option** never appeared.
- [ ] `skill` **Corrun Falt** was paid, not persuaded.
- [ ] `skill` **Atris's Persuade** fails at the chain's ranks (still the Light Path).
- [ ] `choice` **The bay doors:** only "Come with me" with Bao-Dur and Kreia was played; the other party choices weren't.
- [ ] `choice` **The Exchange's and Czerka's dark side or mercenary paths** on Citadel Station, and the Ithorians betrayed.
- [ ] `feature` **`leader player`** after Bao-Dur's pad scene: `tr_baodurpad` makes him leader and nothing in the route's path switches back. Check how the original returns control.

## Dantooine (PR 15)

- [ ] `blocker` **Khoonda's militia desk corner (-6.9, -15.3):** every walk back from Zherron's post sticks there, because the nav mesh's rounding is cut short and the body box catches the desk's sloped nonwalk foot. Zherron's three optional visits are skipped because of it.
- [ ] `side` **Zherron's kinrath job** and its 2000-credit reward (blocked by the desk corner above).
- [ ] `side` **The cave Matriarch**, who only spawns with Zherron's job.
- [ ] `choice` **The will given to Zherron**: the chain gives it to Daraala.
- [ ] `skill` **The battered droid's diagnostic and repair** (Repair).
- [ ] `skill` **The sensor surveillance branch** (Computer, Awareness).
- [ ] `skill` **Ralon's holocron:** both light-side ways need Awareness 4.
- [ ] `skill` **The protocol droids, conduits and storage overload** in the Enclave sublevel.
- [ ] `skill` **Jorran's Persuade:** the chain pays him.
- [ ] `feature` **The landing pad's "Plasteel Cylinder{Med, Blast}":** DSimpson mines it open, and kjk can set mines on doors only.
- [ ] `side` **The pazaak Ithorian.**
- [ ] `side` **Nallek.**
- [ ] `side` **Dopak's message.**
- [ ] `side` **Khoonda's defence tasks** (turrets, mines, droids, militia) before the battle.
- [ ] `side` **Akkere's hydrospanner.**
- [ ] `side` **The lightsaber build:** the lens fixture comes only from a forged will.
- [ ] `feature` **The party swap before the battle** (Handmaiden in, Kreia out): the harness has no goal to open the party screen away from the Hawk.
- [ ] `choice` **Berun's conversation** ends on the harness's default replies.
- [ ] `choice` **The mercenaries' side**, Azkul joined, the dark side Vrook rescue.

## Nar Shaddaa (PR to come)

- [ ] `side` **Vogga's room and the dance.** It needs the Handmaiden (or the Disciple) as the dancer, and with it go Vogga's help against Goto, the Telos fuel and his share of the Exchange's notice.
- [ ] `side` **The swoop droid** and the swoop races.
- [ ] `side` **The Exchange deal.**
- [ ] `side` **IT-31.**
- [ ] `side` **Pazaak.**
- [ ] `side` **Geriel.**
- [ ] `side` **Nadaa.**
- [ ] `side` **Saquesh.**
- [ ] `side` **The airspeeder.**
- [ ] `side` **Lasavvou** (helping him gives no notice).
- [ ] `skill` **The Serroco persuaded**: the option wasn't offered, so they are fought.
- [ ] `choice` **Kaalah-Nah:** the chain pays him 2000 credits for his good word (+3 notice). Test the other ways of reaching the 8 notice Visquis's call needs.
- [ ] `choice` **The HK-50s** talked down, not fought.
- [ ] `choice` **Hanharr instead of Mira** (the dark-side companion).
- [ ] `feature` **Breath Control** is never actually cast; `--god` covers the poison gas in the Jekk'Jekk Tarr.
- [ ] `feature` **Visas's ambush on the Ebon Hawk:** `003EBO_VISAS_ARRIVES` is set now, but the ambush hadn't played by the start of Goto's yacht.

## Later milestones

The drafts in `drafts/` list their skips in their notes files, and each
milestone's PR moves them here.

## Whole-game items

- [ ] `choice` **The dark side playthrough,** start to end.
- [ ] `choice` **A female Exile:** the Disciple joins, not the Handmaiden.
- [ ] `choice` **Influence-gated content:** each companion's high and low influence talks, and the Jedi trainings (Atton, Bao-Dur, Mira, Handmaiden/Disciple, Visas).
- [ ] `side` **HK-47's parts** and his assembly on the Ebon Hawk.
- [ ] `side` **Workbench crafting, lightsaber building and the crystal's growth.**
