"""round.py: the kinds of the current round and what changed for each.

`fixes()` maps a kind's slug to what the owner said of it (word for word,
from marks.txt, which holds what he copied from the page) and what changed.
The round before this one is in the file's history.
"""
import os, re

HERE = os.path.dirname(os.path.abspath(__file__))
ROOM, OBJECT = 'tsl-glowing_surface_of_a_room_', 'tsl-glowing_surface_of_a_placeable_'
# What changed, by slug. A placeable's kind is under OBJECT.
DID = {
    ROOM + 'ond_lt01': "Each of the two floor tubes has its own light that shines out over the floor in front of it. The lights reach 1.6 m instead of 5 m. Onderon's rooms are not baked, so the darkening did not reach them before. It does now, and the room away from the tubes is darker than today. The pool of light at the tubes is still faint in a picture this small.",
    OBJECT + 'plc_bed1': 'Only the small screen at the head of the bed sheds light now: a little, about 1.5 m the way it faces. The bed itself lights nothing.',
    ROOM + 'nar_lt01': 'Each tile of the ceiling panel is its own light now. The panel in the picture has 8 tiles. Each tile lights down out of itself, about 4 m. I do not object. The one cost is that the game uses only the 32 lights nearest the camera, and tiles use them up sooner.',
    ROOM + 'tel_lt03': 'The light reached 8 m and filled the whole wall evenly, so it looked the same as the baked light. Each red tube now lights a red pool about 2 m around it on the wall and the ledge below. The large ceiling panels only add a weak red light to the hall. The hall is about a third darker than today away from the tubes.',
    ROOM + 'per_lt02': 'The lamp is about a quarter weaker.',
    ROOM + 'leh_light04': 'The lamp is a third weaker.',
    ROOM + 'ond_lt03': "Each thin panel now lights only a small pool about 1 m around it. The darkening now reaches Onderon's unbaked rooms, so the room between the panels is a little darker than today and the panels stand out.",
    OBJECT + 'plc_kolpuz': "Yes, they are fixed in the darkened picture now. The kolto machines and the plasteel cylinder lose half of their light like the room. Before, the darkening reached only baked room surfaces, and objects kept all of their light. In the game today an object is lit by the area's flat light and by the hidden lamps, which often push it to full brightness. That is why objects look brighter than the room around them.",
    OBJECT + 'dxn_lt03': 'The lights did not reach the pillar at all before, because the game counts it as a fixed object. They reach it now. The pillar still looks black, because its texture is almost black (9 of 255), and no light can make a black texture bright. It is just as black in the game today. Lighting it needs a brighter texture, not a stronger lamp.',
    ROOM + 'nar_jtrm': 'The neon trim is a third stronger.',
}


def fixes():
    said = {}
    for line in open(os.path.join(HERE, 'marks.txt'), encoding='utf-8'):
        mark = re.match(r'tsl \| (\w+) \| proposed (\w+) \| should be \S+ \| (.*)', line)
        if mark:
            name, key, text = mark.groups()
            # Two kinds share the texture dxn_lt03: the placeable's was proposed as a point.
            thing = name.startswith('plc_') or (name == 'dxn_lt03' and key == 'point')
            said[(OBJECT if thing else ROOM) + name] = text.strip()
    assert set(said) == set(DID), set(said) ^ set(DID)
    return {slug: {'said': said[slug], 'did': did} for slug, did in DID.items()}
