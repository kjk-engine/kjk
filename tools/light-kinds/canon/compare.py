# Compares the canon round c1 of some modules with their earlier set, PNG
# by PNG. The earlier set is round r2 where it exists, else round r1, in a
# folder laid out as bake.sh lays out target/light-kinds/game (one folder
# bake-<module> a module). Rounds r1 and r2 were baked in the worktree
# lighting-step6 and are not in the repository. It needs Pillow.
#   python -I compare.py <canon-bakes folder> <old game folder> <game> <module> ...
# Prints for each module the lightmaps in both, the mean of the per-PNG mean
# absolute difference (of 255, RGB), the largest per-PNG mean and its PNG,
# and PNGs found in one set only.
import glob, os, sys
from PIL import Image, ImageChops, ImageStat

canon, old, game = sys.argv[1:4]
for module in sys.argv[4:]:
    folder = {'152har': 'bake-har', 'end_m01aa': 'bake-gpu'}.get(module, 'bake-' + module)
    label = 'r2' if os.path.isdir(os.path.join(old, folder, 'bake', 'r2')) else 'r1'
    olds = glob.glob(os.path.join(old, folder, 'bake', label, '*', 'b'))
    news = glob.glob(os.path.join(canon, game, module, 'bake', 'c1', '*', 'b'))
    if not olds or not news:
        print(module, 'missing a set', label)
        continue
    a = {os.path.basename(p).lower(): p for p in glob.glob(os.path.join(olds[0], '*.png'))}
    b = {os.path.basename(p).lower(): p for p in glob.glob(os.path.join(news[0], '*.png'))}
    means, worst = [], (0.0, '')
    for name in sorted(set(a) & set(b)):
        x, y = Image.open(a[name]).convert('RGB'), Image.open(b[name]).convert('RGB')
        if x.size != y.size:
            print('  ', module, name, 'sizes differ')
            continue
        m = sum(ImageStat.Stat(ImageChops.difference(x, y)).mean) / 3
        means.append(m)
        worst = max(worst, (m, name))
    print('%s (%s against %s): %d PNGs, mean %.2f, largest %.2f in %s, only old %d, only new %d' % (
        module, 'c1', label, len(means), sum(means) / max(1, len(means)), worst[0], worst[1],
        len(set(a) - set(b)), len(set(b) - set(a))))
