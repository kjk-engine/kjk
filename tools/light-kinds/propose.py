"""propose.py: what each kind of light is proposed to do, from its facts.

`propose(kind)` gives the proposal's key, one plain sentence, a colour (0 to
1), a reach in metres, a strength and how narrow its cone is. `kinds.py`
writes the proposals into `data/lights/<game>.toml`.

The rules are the game's rule data, `data/lights/fittings.toml`: the
general rules' numbers (`[rules]`), the kinds that differ from them, each
with why (`[[fitting]]`), and the lights placed by hand in one area
(`[[placed]]`). `by_rule(kind)` is the general rules alone. This file holds
how they are applied and nothing the owner decided.
"""
import os, tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
DATA = tomllib.load(open(os.path.join(ROOT, 'data', 'lights', 'fittings.toml'), 'rb'))
RULES = DATA['rules']
# What a kind's group is called in the rule data.
ON = {'glowing surface of a room': 'room', 'glowing surface of a placeable': 'placeable'}
# A `[[fitting]]`'s own names for the fields of a proposal.
FIELDS = {'form': 'key', 'says': 'text'}
# The fields of a `[[fitting]]` that say which kind it is and why.
NAMING = ('game', 'texture', 'on', 'reason', 'lamp')


def named(entry, kind):
    """Whether a `[[fitting]]` or a `[[placed]]` entry names this kind. An
    entry without `game` or `on` holds for every game or group."""
    return (entry['texture'] == kind['name'] and entry.get('game', kind['game']) == kind['game']
            and entry.get('on', ON.get(kind['group'])) == ON.get(kind['group']))


def plan(key, text, colour=None, reach=None, strength=0.0, cone=0.0, gather=False, line=False):
    return {'key': key, 'text': text, 'colour': colour, 'reach': reach, 'strength': strength, 'cone': cone,
            'gather': gather, 'line': line}


def by_rule(kind):
    surface, lamps, near = kind.get('surface'), kind.get('lamps'), kind.get('near_bake')
    if kind['group'] == 'sun':
        return plan('keep', 'Keep as it is.')
    if not surface:
        return plan('keep', 'Keep as it is: it lights characters within %.1f m.' % lamps['median_radius'])
    name = kind['name']
    if max(surface['glow']) < RULES['low_glow']:
        return plan('glows', 'Not a light. It lights nothing.')
    colour = lamps['colour'] if lamps else surface['glow']
    top = max(colour)
    colour = [round(min(1.0, v / top), 3) for v in colour] if top > 0 else [1.0, 1.0, 1.0]
    each = surface['median_m2']
    reach = lamps['median_radius'] if lamps else RULES['reach_least'] + each ** 0.5
    reach = round(min(RULES['reach_most'], max(RULES['reach_least'], reach)), 1)
    flat = surface['flat_share'] > RULES['flat_share']
    front = RULES['cone'] if flat else 0.0
    no_lamp = surface['with_lamp_share'] < RULES['with_lamp_share']
    faint = (each > RULES['faint_area'] or (no_lamp and (near is None or near < RULES['near_bake']))
             or any(part in name for part in RULES['faint_names']) or kind.get('by_colour', False))
    if surface['facing_down_share'] > RULES['down_share'] and not faint:
        return plan('down', 'A spotlight straight down, about %.0f m.' % reach, colour, reach, RULES['strong'],
                    RULES['down_cone'])
    if faint:
        screen = any(part in name for part in RULES['screen_names'])
        far, much = (RULES['screen_reach'], RULES['screen']) if screen else (RULES['faint_reach'], RULES['faint'])
        return plan('faint', 'A little light close by, about %.0f m.' % far, colour, far, much, front)
    halves = sorted(mate[10] for example in kind['examples'] for mate in example.get('mates', []))
    if halves and halves[len(halves) // 2] >= RULES['strip_half']:
        return plan('strip', 'A strip that lights along its length, about %.0f m out.' % reach, colour, reach,
                    RULES['strong'], front)
    if flat:
        return plan('panel', 'Lights the way it faces, about %.0f m.' % reach, colour, reach, RULES['strong'],
                    RULES['cone'])
    return plan('point', 'Lights all ways, about %.0f m.' % reach, colour, reach, RULES['strong'], 0.0)


def override(kind):
    """The fields of a kind that differ from the general rules: its
    `[[fitting]]` entries in the rule data, a later one over an earlier
    one, and the lights placed by hand for it (`extra`). `lamp = false`
    says it is no lamp, so it has no colour. Three fields choose the
    kind's fittings, which the game finds as an area loads (`fitted_lights.rs`):
    - `lamps = "bright"` takes them from the survey's `bright_mates`, the
      lamps the texture shows, in place of `mates`, the welded pieces of
      the mesh;
    - `longest` leaves out fittings longer than so many metres, where a
      kind holds tubes and hall-sized panels;
    - `tint = true` gives each fitting the colour of its own bright texels.
    """
    fields = {}
    for entry in DATA.get('fitting', []):
        if named(entry, kind):
            fields.update({FIELDS.get(k, k): v for k, v in entry.items() if k not in NAMING})
            if entry.get('lamp') is False:
                fields['colour'] = None
    extra = [{**{k: v for k, v in light.items() if k not in ('game', 'texture', 'on', 'area', 'reason')},
              'rooms': True} for light in DATA.get('placed', []) if named(light, kind)]
    if extra:
        fields['extra'] = extra
    return fields


def propose(kind):
    """The rules' proposal for a kind, with the fields its entry changes. A
    kind the survey found by its colour (`by_colour`: a red or an orange
    texture, dim by brightness) is faint by the general rules, and each of
    its fittings has the colour of its own bright texels. The owner has not
    seen these kinds yet (17 November 2026)."""
    found = {'tint': True} if kind.get('by_colour') else {}
    return {**by_rule(kind), **found, **override(kind)}

