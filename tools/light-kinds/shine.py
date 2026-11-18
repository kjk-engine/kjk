"""shine.py [--retake] [slug ...]: each pictured light fitting shining in a darkened room.

For every kind that has a picture and a proposal that lights something, it
runs the game once with the room's baked light at half
(KJK_SHADOW_DEBUG=7) and a light of the proposed colour and reach at every
fitting of the kind in the area (KJK_FOUND_LIGHTS). The camera is the first
picture's. The pictures land in shots/<slug>.shine.png. Two games run at a
time. A kind whose picture exists is skipped.

With `--retake`, it takes both pictures of each named kind again, today's
(shots/<slug>.png) and the darkened one, from the place and the camera of
the kind's override (`propose.place`).
"""
import glob, json, os, shutil, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
from fill import room_fill
from propose import fittings, place, propose, slug

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
KJK = os.path.join(ROOT, 'target', 'release', 'kjk.exe' if os.name == 'nt' else 'kjk')
SETTINGS = os.path.join(ROOT, 'target', 'detail-bake', 'shots', 'enhanced.json')
# Every launch keeps its saves and its last-played note in a scratch folder.
SAVES = os.path.join(ROOT, 'target', 'detail-bake', 'saves')
OFF = 0.25      # How far off its surface a fitting's light stands (metres).

shots = os.path.join(HERE, 'shots')


def lights_of(plan, mates):
    """The lights of a proposal, one for each fitting in `mates`."""
    if not plan['colour']:
        return []
    colour = [v * plan['strength'] for v in plan['colour']]
    if plan['gather']:
        # Many tiny pieces of one object: one soft light above their middle.
        groups = []
        for mate in mates:
            near = next((g for g in groups if sum((a - b) ** 2 for a, b in zip(g[0][:3], mate[:3])) < 9.0), None)
            if near:
                near.append(mate)
            else:
                groups.append([mate])
        middles = [[sum(m[i] for m in group) / len(group) for i in range(3)] for group in groups]
        return [{'at': [x, y, z + 0.6], 'colour': colour, 'radius': plan['reach'], 'rooms': True}
                for x, y, z in middles]
    off = plan.get('off', OFF)
    lights = []
    for mate in mates:
        x, y, z, fx, fy, fz, width, lx, ly, lz, half = mate[:11]
        own = colour
        if plan.get('tint') and max(mate[11:14], default=0) > 0:
            # The fitting's own colour, from its bright texels.
            own = [v * plan['strength'] for v in mate[11:14]]
        light = {'at': [x + fx * off, y + fy * off, z + fz * off], 'colour': own, 'radius': plan['reach'],
                 'rooms': True}
        if plan['cone'] and fx * fx + fy * fy + fz * fz > 0.25:
            # It shines out of its fitting, not onto what it sits on.
            light.update(aim=[fx, fy, fz], cone=plan['cone'])
        if plan['line']:
            # The whole width of the lamp shines, not its middle.
            light.update(way=[lx, ly, lz], half=half)
        elif half > 1.0 and plan['key'] != 'down':
            # A long surface shines as a strip along its length.
            light.update(way=[lx, ly, lz], half=half - 0.3)
        lights.append(light)
    return lights


def picture(at, out, lights=None, dim=True):
    """One game launch that pictures the place `at` into the file `out`: as
    the game is today, or with `lights` in the darkened room. With `dim`
    off, the room keeps all of its baked light: no fitting is near enough
    to light it, so nothing would make up for a darkening."""
    work = tempfile.mkdtemp(prefix='shine-', dir=shots)
    env = dict(os.environ, KJK_SETTINGS=SETTINGS, KJK_SAVES=SAVES, KJK_MUTE='1')
    if lights is not None:
        json.dump(lights, open(os.path.join(work, 'lights.json'), 'w'))
        env.update(KJK_FOUND_LIGHTS=os.path.join(work, 'lights.json'))
        if dim:
            env.update(KJK_SHADOW_DEBUG='7')
    x, y, z, yaw, pitch = at['view']
    cmds = ['wait world', 'wait 200f', 'anim 5', 'wait 20f',
            'camera %.2f %.2f %.2f %.1f %.1f 60' % (x, y, z, yaw, pitch), 'wait 14f', 'screenshot shot', 'quit']
    try:
        code = subprocess.run([KJK, 'play', '--stage', at['module'], '--headless', '--size', '960x540',
                               '--cmds', '; '.join(cmds)], cwd=work, env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=600).returncode
    except subprocess.TimeoutExpired:
        code = 'timed out'
    for found in glob.glob(os.path.join(work, 'kjk-artifacts', 'runs', '*', 'shot.png')):
        shutil.move(found, out)
    shutil.rmtree(work, ignore_errors=True)
    return 'exit %s, %s' % (code, 'picture' if os.path.exists(out) else 'NO PICTURE')


def main():
    retake = '--retake' in sys.argv[1:]
    only = set(sys.argv[1:]) - {'--retake'}
    kinds = {slug(k): k for k in json.load(open(os.path.join(HERE, 'light-kinds.json')))}
    chosen = json.load(open(os.path.join(HERE, 'shots.json')))
    bad_file = os.path.join(HERE, 'bad.txt')
    bad = set(open(bad_file).read().split()) if os.path.exists(bad_file) else set()
    os.makedirs(shots, exist_ok=True)

    def shoot(name):
        kind = kinds[name]
        at = place(kind, chosen[name])
        today, out = os.path.join(shots, name + '.png'), os.path.join(shots, name + '.shine.png')
        if retake:
            for old in (today, out):
                if os.path.exists(old):
                    os.remove(old)
            print(name, 'today:', picture(at, today), flush=True)
        elif os.path.exists(out):
            return name, 'kept'
        example = next((e for e in kind['examples'] if e['module'] == at['module'] and e['area'] == at['area']), None)
        plan = propose(kind)
        # A trial may name the fittings itself (`mates` in its override).
        mates = fittings(kind, example) or [at['position'] + [0] * 8]
        lights = lights_of(plan, mates) + plan.get('extra', [])
        if plan['key'] == 'glows' and plan.get('dim', True):
            # It lights nothing, so the area's hidden lamps light its room.
            lights += room_fill(kind['game'], at['module'], at['area'], at['view'][:3])
        return name, '%s, %d lights' % (picture(at, out, lights, plan.get('dim', True)), len(lights))

    wanted = [name for name in chosen
              if kinds[name].get('surface') and (name in only if only else name not in bad)]
    print(len(wanted), 'kinds', flush=True)
    with ThreadPoolExecutor(2) as pool:
        for name, result in pool.map(shoot, wanted):
            print(name, result, flush=True)


if __name__ == '__main__':
    main()
