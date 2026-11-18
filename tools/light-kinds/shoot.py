"""shoot.py [most]: a picture in the game of each common kind of light.

Reads light-kinds.json beside it, takes each game's `most` commonest kinds
(70 by default), picks for each a place to look at it so that few modules
are loaded (the next place for a kind listed in retry.txt), and runs the game once a module. The pictures land in
shots/<game>-<kind>.png, and shots.json says which place each shows.
"""
import glob, json, os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
KJK = os.path.join(ROOT, 'target', 'release', 'kjk.exe' if os.name == 'nt' else 'kjk')
SETTINGS = os.path.join(ROOT, 'target', 'detail-bake', 'shots', 'enhanced.json')
most = int(sys.argv[1]) if len(sys.argv) > 1 else 70
only = sys.argv[2] if len(sys.argv) > 2 else None

kinds = json.load(open(os.path.join(HERE, 'light-kinds.json')))
shots = os.path.join(HERE, 'shots')
os.makedirs(shots, exist_ok=True)


def slug(kind):
    return kind['game'] + '-' + re.sub(r'[^a-z0-9]+', '_', (kind['group'] + ' ' + kind['name']).lower()).strip('_')


# retry.txt lists the kinds whose first place gave a bad picture.
retry_file = os.path.join(HERE, 'retry.txt')
retry = set(open(retry_file).read().split()) if os.path.exists(retry_file) else set()

plan = {}  # (game, module) -> [(slug, example)]
chosen = {}
for game in ('k1', 'tsl'):
    of_game = [k for k in kinds if k['game'] == game and k['examples']][:most]
    for kind in of_game:
        examples = kind['examples']
        if slug(kind) in retry:
            # The first place gave a bad picture: take the next, in another
            # module where there is one.
            later = examples[1:]
            examples = [e for e in later if e['module'] != examples[0]['module']] or later or examples
        used = [e for e in examples if (game, e['module']) in plan]
        example = (used or examples)[0]
        plan.setdefault((game, example['module']), []).append((slug(kind), example))
        chosen[slug(kind)] = example

json.dump(chosen, open(os.path.join(HERE, 'shots.json'), 'w'), indent=1)
print(len(chosen), 'kinds in', len(plan), 'modules', flush=True)

for (game, module), wanted in sorted(plan.items()):
    if only and module != only:
        continue
    if all(os.path.exists(os.path.join(shots, name + '.png')) for name, _ in wanted):
        continue
    cmds = ['wait world', 'wait 200f', 'anim 5', 'wait 20f']
    for name, example in wanted:
        x, y, z, yaw, pitch = example['view']
        cmds += ['camera %.2f %.2f %.2f %.1f %.1f 60' % (x, y, z, yaw, pitch), 'wait 14f', 'screenshot ' + name]
    cmds.append('quit')
    env = dict(os.environ, KJK_SETTINGS=SETTINGS, KJK_MUTE='1')
    log = open(os.path.join(shots, 'run-%s-%s.log' % (game, module)), 'w')
    try:
        code = subprocess.run(
            [KJK, 'play', '--stage', module, '--headless', '--size', '960x540', '--cmds', '; '.join(cmds)],
            cwd=shots, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=600).returncode
    except subprocess.TimeoutExpired:
        code = 'timed out'
    # The game writes each picture into its run's folder; bring them here.
    for name, _ in wanted:
        for found in glob.glob(os.path.join(shots, 'kjk-artifacts', 'runs', '*', name + '.png')):
            shutil.move(found, os.path.join(shots, name + '.png'))
    shutil.rmtree(os.path.join(shots, 'kjk-artifacts'), ignore_errors=True)
    made = sum(os.path.exists(os.path.join(shots, name + '.png')) for name, _ in wanted)
    print(game, module, 'exit', code, 'pictures', made, 'of', len(wanted), flush=True)
