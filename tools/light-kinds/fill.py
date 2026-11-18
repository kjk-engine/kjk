"""fill.py: a room lit by its area's own hidden lamps.

A kind that lights nothing (a wall, a floor or a door the survey took for a
fitting) leaves its darkened room too dark. The owner asked for such rooms
to be lit some other way, "from the lamps on the sides". Every area has
hidden lamps in its room models, which light characters today. `room_fill`
turns those lamps into lights for the darkened picture: each lamp at its
own place, with its own colour and radius, at one strength for the area.

`python fill.py <room-lights.json> ...` writes room-lamps.json beside this
file from the output of `kjk room-lights all --out <folder>`.
"""
import json, math, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
LAMPS = os.path.join(HERE, 'room-lamps.json')
STRONG_COLOUR = 0.35    # A lamp of one strong colour counts at this much, or the room turns that colour.
GAIN = 3.0              # Fitted on the Harbinger (2.8) and the Dantooine enclave (3.1).
NEAREST = 32            # The game uses only this many lights, the nearest to the camera.


def falloff(distance, radius):
    """How much of a fight's light is left at a distance (odyssey.wgsl, `fight_falloff`)."""
    if distance >= radius:
        return 0.0
    edge = max(0.0, 1 - (distance / radius) ** 4)
    return edge * edge / (1 + (distance / (0.45 * radius)) ** 2)


def power(lamp):
    """A lamp's colour as it lights a room: at its multiplier, and held back if it is one strong colour."""
    x, y, z, radius, red, green, blue, multiplier = lamp
    colour = sorted((red, green, blue))
    strong = colour[2] > 2 * colour[1]
    return [v * multiplier * (STRONG_COLOUR if strong else 1.0) for v in (red, green, blue)]


def strength(area):
    """The one strength for an area's lamps that puts back the half of the
    baked light the darkening removes. It compares the area's mean baked
    light with the light all lamps together shed 1.5 m under a typical lamp."""
    lamps = area['lamps']
    if not lamps or not area['mean']:
        return 0.0
    under = sorted(sum(max(power(other)) * falloff(math.dist((lamp[0], lamp[1], lamp[2] - 1.5), other[:3]), other[3])
                       for other in lamps) for lamp in lamps)
    middle = under[len(under) // 2]
    return min(2.0, max(0.05, GAIN * area['mean'] / 2 / middle)) if middle > 0 else 0.0


def room_fill(game, module, area, eye):
    """The lights of an area's hidden lamps nearest the camera at `eye`."""
    areas = json.load(open(LAMPS))[game]
    found = areas.get(module + '/' + area)
    scale = strength(found) if found else 0.0
    if not scale:
        return []
    near = sorted(found['lamps'], key=lambda lamp: math.dist(lamp[:3], eye))[:NEAREST]
    return [{'at': lamp[:3], 'radius': lamp[3], 'rooms': True, 'colour': [v * scale for v in power(lamp)]}
            for lamp in near]


if __name__ == '__main__':
    games = {}
    for path in sys.argv[1:]:
        for game in json.load(open(path)):
            games[game['game']] = {
                area['module'] + '/' + area['area']: {
                    'mean': area['lightmap_mean'],
                    'lamps': [[round(v, 3) for v in lamp['position'] + [lamp['radius']] + lamp['color'] + [lamp['multiplier']]]
                              for lamp in area['lamps'] if lamp['owner'] == 'room' and not lamp['ambient_only']]}
                for area in game['areas']}
    json.dump(games, open(LAMPS, 'w'), separators=(',', ':'))
    print({game: len(areas) for game, areas in games.items()}, os.path.getsize(LAMPS) // 1024, 'KB')
