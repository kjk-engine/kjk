"""round3.py: the eleven kinds of the owner's second check, pictured again.

Takes each kind's place from the new survey (the same area as before where
the kind is still there), writes it into shots.json, drops the old
pictures and takes today's picture again. shine.py then takes the other.
"""
import glob, json, os, shutil, subprocess, tempfile
from concurrent.futures import ThreadPoolExecutor
from propose import slug

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
KJK = os.path.join(ROOT, 'target', 'release', 'kjk.exe')
SETTINGS = os.path.join(ROOT, 'target', 'detail-bake', 'shots', 'enhanced.json')
NAMES = open(os.path.join(HERE, 'round3.txt')).read().split()
# Kinds whose old place was a cutscene or showed the fitting badly: the
# survey's first place instead, or the place of this number.
FRESH = {'lhr_lit01': 0, 'lma_light06': 0}

kinds = {k['name']: k for k in json.load(open(os.path.join(HERE, 'light-kinds.json'))) if k['game'] == 'k1'}
chosen = json.load(open(os.path.join(HERE, 'shots.json')))
shots = os.path.join(HERE, 'shots')
os.makedirs(shots, exist_ok=True)
jobs = []
for name in NAMES:
    kind = kinds[name]
    key, old = slug(kind), chosen[slug(kind)]
    same = [e for e in kind['examples'] if e['module'] == old['module'] and e['area'] == old['area']]
    example = kind['examples'][FRESH[name]] if name in FRESH else (same or kind['examples'])[0]
    chosen[key] = {k: v for k, v in example.items() if k != 'mates'}
    for path in (os.path.join(HERE, 'kept', key + '.jpg'), os.path.join(HERE, 'kept', key + '.shine.jpg'),
                 os.path.join(shots, key + '.png'), os.path.join(shots, key + '.shine.png')):
        if os.path.exists(path):
            os.remove(path)
    jobs.append((key, example))
json.dump(chosen, open(os.path.join(HERE, 'shots.json'), 'w'), indent=1)


def shoot(job):
    key, example = job
    work = tempfile.mkdtemp(prefix='today-', dir=shots)
    x, y, z, yaw, pitch = example['view']
    cmds = ['wait world', 'wait 200f', 'anim 5', 'wait 20f',
            'camera %.2f %.2f %.2f %.1f %.1f 60' % (x, y, z, yaw, pitch), 'wait 14f', 'screenshot today', 'quit']
    env = dict(os.environ, KJK_SETTINGS=SETTINGS, KJK_MUTE='1')
    try:
        code = subprocess.run([KJK, 'play', '--stage', example['module'], '--headless', '--size', '960x540',
                               '--cmds', '; '.join(cmds)], cwd=work, env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, timeout=600).returncode
    except subprocess.TimeoutExpired:
        code = 'timed out'
    for found in glob.glob(os.path.join(work, 'kjk-artifacts', 'runs', '*', 'today.png')):
        shutil.move(found, os.path.join(shots, key + '.png'))
    shutil.rmtree(work, ignore_errors=True)
    return key, code, example['module'], example['room']


with ThreadPoolExecutor(2) as pool:
    for result in pool.map(shoot, jobs):
        print(*result, flush=True)
