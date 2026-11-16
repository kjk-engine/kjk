"""shine.py [slug ...]: each pictured light fitting shining in a darkened room.

For every kind that has a picture and a proposal that lights something, it
runs the game once with the room's baked light at half
(KJK_SHADOW_DEBUG=7) and a light of the proposed colour and reach at every
fitting of the kind in the area (KJK_FOUND_LIGHTS). The camera is the first
picture's. The pictures land in shots/<slug>.shine.png. Two games run at a
time.
"""
import glob, json, os, shutil, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
from propose import propose, slug

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
KJK = os.path.join(ROOT, 'target', 'release', 'kjk.exe')
SETTINGS = os.path.join(ROOT, 'target', 'detail-bake', 'shots', 'enhanced.json')
OFF = 0.25      # How far off its surface a fitting's light stands (metres).
only = set(sys.argv[1:])

kinds = {slug(k): k for k in json.load(open(os.path.join(HERE, 'light-kinds.json')))}
chosen = json.load(open(os.path.join(HERE, 'shots.json')))
bad_file = os.path.join(HERE, 'bad.txt')
bad = set(open(bad_file).read().split()) if os.path.exists(bad_file) else set()
shots = os.path.join(HERE, 'shots')
os.makedirs(shots, exist_ok=True)


def shoot(name):
    kind, place = kinds[name], chosen[name]
    plan = propose(kind)
    out = os.path.join(shots, name + '.shine.png')
    if os.path.exists(out):
        return name, 'kept'
    example = next((e for e in kind['examples'] if e['module'] == place['module'] and e['area'] == place['area']), None)
    mates = example['mates'] if example else [place['position'] + [0] * 8]
    lights = []
    if plan['colour'] and plan['gather']:
        # Many tiny pieces of one object: one soft light above their middle.
        groups = []
        for mate in mates:
            near = next((g for g in groups if sum((a - b) ** 2 for a, b in zip(g[0][:3], mate[:3])) < 9.0), None)
            if near:
                near.append(mate)
            else:
                groups.append([mate])
        for group in groups:
            middle = [sum(m[i] for m in group) / len(group) for i in range(3)]
            lights.append({'at': [middle[0], middle[1], middle[2] + 0.6],
                           'colour': [v * plan['strength'] for v in plan['colour']], 'radius': plan['reach'],
                           'rooms': True})
    elif plan['colour']:
        for x, y, z, fx, fy, fz, width, lx, ly, lz, half in mates:
            light = {'at': [x + fx * OFF, y + fy * OFF, z + fz * OFF],
                     'colour': [v * plan['strength'] for v in plan['colour']], 'radius': plan['reach'], 'rooms': True}
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
    work = tempfile.mkdtemp(prefix='shine-', dir=shots)
    json.dump(lights, open(os.path.join(work, 'lights.json'), 'w'))
    x, y, z, yaw, pitch = place['view']
    cmds = ['wait world', 'wait 200f', 'anim 5', 'wait 20f',
            'camera %.2f %.2f %.2f %.1f %.1f 60' % (x, y, z, yaw, pitch), 'wait 14f', 'screenshot shine', 'quit']
    env = dict(os.environ, KJK_SETTINGS=SETTINGS, KJK_MUTE='1', KJK_SHADOW_DEBUG='7',
               KJK_FOUND_LIGHTS=os.path.join(work, 'lights.json'))
    try:
        code = subprocess.run([KJK, 'play', '--stage', place['module'], '--headless', '--size', '960x540',
                               '--cmds', '; '.join(cmds)], cwd=work, env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=600).returncode
    except subprocess.TimeoutExpired:
        code = 'timed out'
    for found in glob.glob(os.path.join(work, 'kjk-artifacts', 'runs', '*', 'shine.png')):
        shutil.move(found, out)
    shutil.rmtree(work, ignore_errors=True)
    return name, 'exit %s, %d lights, %s' % (code, len(lights), 'picture' if os.path.exists(out) else 'NO PICTURE')


wanted = [name for name in chosen if name not in bad and kinds[name].get('surface') and (not only or name in only)]
print(len(wanted), 'kinds', flush=True)
with ThreadPoolExecutor(2) as pool:
    for name, result in pool.map(shoot, wanted):
        print(name, result, flush=True)
