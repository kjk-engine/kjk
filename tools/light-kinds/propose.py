"""propose.py: what each kind of light is proposed to do, from its facts.

`propose(kind)` gives the proposal's key, one plain sentence, a colour (0 to
1), a reach in metres, a strength and how narrow its cone is. The owner
checks these on the page; nobody has to fill them in.

The rules follow the owner's first check (16 November 2026):
- every glowing fitting gives at least a little light;
- the room's light level stays about as it is, so strengths are low;
- a light shines out of its fitting and never onto the panel it sits on;
- a screen, a sign, a crystal or a fire gives only a faint glow.
"""
import glob, json, os, re

FAINT_NAMES = ('scre', 'mon', 'sign', 'comp', 'holo', 'map', 'btl', 'pnl', 'panl', 'term',
               'crystal', 'fire', 'lvl', 'kol', 'bike')
# What the owner said of single kinds, by texture: these are floor or track,
# not lights.
NOT_LIGHTS = ('lsi_trim02', 'lza_flr11', 'lts_lobwal07', 'tel_st01', 'tel_st02', 'tel_gr04', 'nar_cr8s', 'plc_bed1')
# A texture that glows less than this is a wall, a floor, a door or trim that
# the survey took for a fitting (his KOTOR 2 check). It lights nothing. The
# three named ones are small displays, or were never seen in a picture.
LOW_GLOW = 0.1
LOW_GLOW_LIGHTS = ('lma_lite01', 'dan_scren2', 'lts_light01')
# The owner's second check: this one stays too bright whatever it is given.
REMOVED = ('lma_light06',)
# Faint kinds he asked a little more light of, and their strength.
MORE = {'leh_light01': 0.7, }
# His third check: flat lamps that should glow more the way they face, and
# their strength.
GLOW = {'lza_lit01': 1.7}
# Small rectangles that shine as a thin panel of their own width, not a spot.
LINES = ('lsi_lite02',)
# Large windows that light their hall: the reach in metres.
WINDOWS = {'lts_lowin01': 8.0}
# A placeable made of many tiny glowing pieces: one soft light for all of it.
GATHERED = ('plc_btlclrs',)
# One mesh holds several round ceiling lights: spotlights, not a strip.
SPOTS = ('lsi_lite05',)

STRONG, FAINT = 1.1, 0.4   # Strengths of a fight's light (`shine.py`).


def slug(kind):
    return kind['game'] + '-' + re.sub(r'[^a-z0-9]+', '_', (kind['group'] + ' ' + kind['name']).lower()).strip('_')


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
    if name in NOT_LIGHTS or (max(surface['glow']) < LOW_GLOW and name not in LOW_GLOW_LIGHTS):
        return plan('glows', 'Not a light. It lights nothing.')
    if name in REMOVED:
        return plan('glows', 'Removed. It lights nothing.')
    colour = lamps['colour'] if lamps else surface['glow']
    top = max(colour)
    colour = [round(min(1.0, v / top), 3) for v in colour] if top > 0 else [1.0, 1.0, 1.0]
    each = surface['median_m2']
    reach = round(min(5.0, max(2.5, lamps['median_radius'] if lamps else 2.5 + each ** 0.5)), 1)
    flat = surface['flat_share'] > 0.5
    front = 0.2 if flat else 0.0
    no_lamp = surface['with_lamp_share'] < 0.1
    faint = each > 40 or (no_lamp and (near is None or near < 0.9)) or any(part in name for part in FAINT_NAMES)
    if name in WINDOWS:
        return plan('panel', 'Lights the way it faces, about %.0f m.' % WINDOWS[name], colour, WINDOWS[name], STRONG, 0.2)
    if name in GLOW:
        return plan('panel', 'Lights the way it faces, about %.0f m.' % reach, colour, reach, GLOW[name], 0.2)
    if name in LINES:
        return plan('panel', 'A thin panel as wide as the lamp, lit about 2 m the way it faces.', colour, 2.0, STRONG, 0.2,
                    line=True)
    if name in GATHERED:
        return plan('faint', 'One soft light over the whole of it, about 3 m.', colour, 3.0, 0.6, 0.0, True)
    if name in SPOTS or (surface['facing_down_share'] > 0.5 and not faint):
        return plan('down', 'A spotlight straight down, about %.0f m.' % reach, colour, reach, STRONG, 0.7)
    if faint:
        if name == 'lts_loscre01':
            # A display lights the wall around it too, so it has no cone.
            return plan('faint', 'Some light all round it, about 4 m.', colour, 4.0, 1.6, 0.0)
        if name in MORE:
            return plan('faint', 'Some light close by, about 3 m.', colour, 3.0, MORE[name], front)
        return plan('faint', 'A little light close by, about 2 m.', colour, 2.0, FAINT, front)
    halves = sorted(mate[10] for example in kind['examples'] for mate in example.get('mates', []))
    if halves and halves[len(halves) // 2] >= 2.0:
        return plan('strip', 'A strip that lights along its length, about %.0f m out.' % reach, colour, reach, STRONG, front)
    if flat:
        return plan('panel', 'Lights the way it faces, about %.0f m.' % reach, colour, reach, STRONG, 0.2)
    return plan('point', 'Lights all ways, about %.0f m.' % reach, colour, reach, STRONG, 0.0)


# Trials of single kinds. Each `overrides-*.json` beside this file maps a
# kind's slug or texture name to the fields of its proposal that differ from
# the rules (`key`, `text`, `colour`, `reach`, `strength`, `cone`, `gather`,
# `line`, `off`), and to `example` and `view`, which choose the place and
# the camera of its pictures (`place`). A slug's entry wins over a name's.
PLACE_FIELDS = ('example', 'view')
OVERRIDES = {}
for path in sorted(glob.glob(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'overrides-*.json'))):
    for name, fields in json.load(open(path)).items():
        OVERRIDES.setdefault(name, {}).update(fields)


def override(kind):
    return {**OVERRIDES.get(kind['name'], {}), **OVERRIDES.get(slug(kind), {})}


def propose(kind):
    """The rules' proposal for a kind, with the trial fields of its override."""
    changed = {k: v for k, v in override(kind).items() if k not in PLACE_FIELDS}
    return {**by_rule(kind), **changed}


def place(kind, chosen):
    """Where a kind's pictures are taken: `chosen` (its entry in shots.json),
    or the example and the camera its override names."""
    fields = override(kind)
    if 'example' in fields:
        chosen = kind['examples'][fields['example']]
    return {**chosen, 'view': fields.get('view', chosen['view'])}
