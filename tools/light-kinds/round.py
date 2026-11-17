"""round.py: the kinds of the current round and what changed for each.

`fixes()` maps a kind's slug to what the owner said of it (word for word,
from his marks in handover.md) and what changed. A kind he did not mark,
but which a new rule changed, has no `said`.
"""
import os, re

HERE = os.path.dirname(os.path.abspath(__file__))
ROOM, OBJECT = 'tsl-glowing_surface_of_a_room_', 'tsl-glowing_surface_of_a_placeable_'
SHIP = "The ship's own hidden lamps light the room instead, to about today's brightness."
ENCLAVE = "The enclave's own hidden lamps, which stand at the pillar lamps on the walls, light the room instead."

# What changed, by texture. A placeable's kind is under OBJECT.
DID = {
    ROOM + 'dan_leaf02': 'The leaves light nothing now. Four soft daylight lights shine down from the open ceiling windows above the trees. Each reaches about 7 m.',
    ROOM + 'har_wl01': 'This wall panel is not a lamp and lights nothing now. ' + SHIP + ' The new picture looks along the corridor at the door, and the door no longer stands out.',
    ROOM + 'tel_hl1': 'The rule called this lamp faint because it is very large. It now lights the room up to 8 m the way it faces, at five times the strength, and the room is about as bright as today.',
    ROOM + 'tel_sstn2': "This is the far backdrop of the station's buildings. It stands 57 m or more from anywhere a player walks, so it glows as before and lights nothing. The new picture shows its lit windows.",
    OBJECT + 'plc_koltoglass': 'The new picture shows the kolto tube from the side. Its faint light is unchanged.',
    ROOM + 'leh_scre02': 'The new picture shows the console from where a player stands. Each group of small screens has one soft light, not one light for every button.',
    ROOM + 'har_wl07': 'This wall panel is not a lamp and lights nothing now. ' + SHIP + ' The door no longer stands out.',
    ROOM + 'leh_light01': 'The lamp reaches about 3 m now. It reached 5 m before.',
    OBJECT + 'plc_marker1': 'The light now sits on the white strip at the back of each pad and runs along it. It is about a quarter stronger. The new picture looks down at both pads.',
    ROOM + 'nar_lt03': 'The light now sits in the middle of the four tubes and shines out of the lamp the way it faces, so the ceiling around it stays as bright as today.',
    ROOM + 'ond_lt01': 'Each of the two tubes has its own light now, and each light runs along the length of its tube.',
    ROOM + 'har_tr03': 'This trim is not a lamp and lights nothing now. ' + SHIP + ' Red lamps count at a third of their strength, so the room no longer turns red.',
    OBJECT + 'plc_bed1': 'The small screen at the head of the bed lights nothing now.',
    ROOM + 'nar_lt01': 'The first pictures were black, because that module opens on a cutscene. The new pictures show a lit room, with the light just under the middle of the ceiling panel. The ceiling beside the panel is still a little darker than today.',
    OBJECT + 'plc_koltank': "Each tank's panel now has one soft light where it had five small ones. The light is half again as strong and reaches about 2.5 m.",
    ROOM + 'tel_lt03': 'This large red lamp now lights its room in red up to 8 m the way it faces, and the room is about as bright as today.',
    ROOM + 'har_tr01': 'This trim is not a lamp and lights nothing now. ' + SHIP,
    ROOM + 'har_wl04': 'This is a wall panel, not a lamp, and it lights nothing now. ' + SHIP,
    ROOM + 'leh_light02': 'The ceiling strip is about a quarter stronger.',
    ROOM + 'dan_unwal07': 'The wall by the door lights nothing now. ' + ENCLAVE + ' The new picture shows the wall with the pillar lamps on both sides.',
    ROOM + 'har_rd01': 'This wall is not a lamp and lights nothing now. ' + SHIP + ' The red tint is gone.',
    ROOM + 'per_lt02': 'Nothing changed. Your mark was made when the room was darkened to a fifth. With the room at half, this lamp lights it about as brightly as today.',
    ROOM + 'har_wl09': 'This floor lights nothing now. ' + SHIP + ' The floor panels come out paler than today, because the hidden lamps are paler than the orange strips.',
    ROOM + 'nar_lt04': 'These are real tube lamps along the track, so they now light the tunnel up to 8 m from them. The swoop race ignores the camera, so the picture can only show the start line, and no lamp stands within 30 m of it. A room that no fitting reaches keeps all of its baked light, so the start looks as it does today.',
    ROOM + 'dan_trim04': 'This trim is not a lamp and lights nothing now. ' + ENCLAVE + ' The new picture shows the wall and its lamps.',
    ROOM + 'ond_lt03': 'Each thin panel on the desk has its own light along its length. The lights are about a third as strong as before, because 84 of them made the room brighter than today.',
    ROOM + 'har_tr02': 'This trim is not a lamp and lights nothing now. ' + SHIP + ' The picture still looks up at crates, which come out lighter than today.',
    ROOM + 'dan_wall07': 'The door panel lights nothing now. ' + ENCLAVE + ' The new picture is taken from inside the room.',
    OBJECT + 'plc_kolpuz': 'The new picture shows the three kolto machines. They are machines, not lamps, so they keep a faint light of about 2 m.',
    ROOM + 'har_fl01': 'This floor tile lights nothing now. ' + SHIP,
    ROOM + 'leh_light03': 'The wall panel is about a quarter stronger.',
    ROOM + 'dan_grate02': 'This door grate lights nothing now. ' + ENCLAVE + ' The picture still shows only a band of ceiling above the door.',
    ROOM + 'dan_light03': 'The survey had cut each tube lamp in two at its clamp, so each tube had two lights. Each tube now has one light at its middle, at about half the strength.',
    ROOM + 'har_wl06': 'This wall is not a lamp and lights nothing now. ' + SHIP,
    ROOM + 'dxn_lt03': 'Each square in the band has its own small light now. Before, the whole band of 8 squares had one light.',
    ROOM + 'nar_lt02': 'The row of lamp squares is one fitting on the beam. The light now runs along the whole row, and it is about a third stronger.',
    ROOM + 'dan_window01': 'This window frame lights nothing now. ' + ENCLAVE + ' I did not find a better angle. The picture still shows the dark frame from outside.',
    OBJECT + 'dxn_lt03': "The pillar lamp's light is about a third stronger.",
    ROOM + 'har_wl02': 'This wall is not a lamp and lights nothing now. ' + SHIP + ' The room comes out a little brighter than today.',
    ROOM + 'nar_cr8s': 'These are crates beside the swoop track, not lamps, so they light nothing. No lamp stands near the start line, so the area keeps all of its baked light and looks as it does today.',
    ROOM + 'nar_jtrm': 'This is neon trim, not a spotlight. It now glows softly all along its length, in all directions, about 3 m. Its light is pale blue, also where the trim is pink.',
    OBJECT + 'plc_holoart1a': 'The new picture shows the kolto tube from the side, with its glowing ends at the top and the bottom. Its faint light is unchanged.',
    ROOM + 'dan_wall08': 'This wall lights nothing now. ' + ENCLAVE + ' The room is a little darker than today.',
    ROOM + 'dan_wall02': 'This wall lights nothing now. ' + ENCLAVE + ' The dark corners come out brighter than today.',
    ROOM + 'har_wl08': 'This wall panel is not a lamp and lights nothing now. Red lamps count at a third of their strength, so nothing turns red.',
    ROOM + 'tel_st01': "This is the orange paint on the track's walls and floor, not a lamp, so it lights nothing. The track has no lamp at all, so it keeps all of its baked light and looks as it does today.",
    # Kinds he did not mark, changed by the rules of this round.
    ROOM + 'har_lt01': 'You did not mark this one. With one light on every strip and the room at half, the strips made the room white. Each strip is now about a tenth as strong, and its light is amber like the strip.',
    ROOM + 'leh_light04': 'You did not mark this one. With the room at half it came out brighter than today, so it is now less than half as strong.',
    ROOM + 'har_rd02': 'You did not mark this one. Its texture does not glow, so by the new rule it is a wall and lights nothing. ' + SHIP,
    ROOM + 'dan_trim02': 'You did not mark this one. Its texture does not glow, so by the new rule it is trim and lights nothing. ' + ENCLAVE,
    'k1-glowing_surface_of_a_room_leh_bike': "You did not mark this one. It is the swoop bike's body in KOTOR 1. Its texture does not glow, so by the new rule it lights nothing, and the room's own hidden lamps light the room.",
}


def fixes():
    said = {}
    for line in open(os.path.join(HERE, 'handover.md'), encoding='utf-8'):
        mark = re.match(r'tsl \| (\w+) \| proposed (\w+) \| (.*)', line)
        if mark:
            name, key, text = mark.groups()
            # Two kinds share the texture dxn_lt03: he saw the placeable's proposed as a point.
            thing = name.startswith('plc_') or (name == 'dxn_lt03' and key == 'point')
            said[(OBJECT if thing else ROOM) + name] = text.strip()
    missing = set(said) - set(DID)
    assert not missing, missing
    return {slug: {'said': said.get(slug), 'did': did} for slug, did in DID.items()}
