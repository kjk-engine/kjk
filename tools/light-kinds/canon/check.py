# Checks of a canon round, after canon.sh has run for both games.
#   python -I check.py <canon-bakes folder> [<label>]
# <label> is the round's name, c1 when left out. The modules are the lists
# modules-<game>.txt beside this file.
# Writes check.txt into the canon-bakes folder: for each game the modules
# baked, failed and missing, PNGs and MB of the `b` folders, each set's
# fingerprint format and placeables.txt, the modules with --stored-backdrop,
# PNGs byte-identical to an original-copy (a stored lightmap that the baker
# copies), and the rooms with no lamp light and no fill. compare.py compares
# a round with an earlier one.
import glob, hashlib, os, re, sys

canon = sys.argv[1]
label = sys.argv[2] if len(sys.argv) > 2 else 'c1'
lists = os.path.dirname(os.path.abspath(__file__))
out = open(os.path.join(canon, 'check.txt'), 'w', newline='\n')

def say(*parts):
    print(*parts)
    print(*parts, file=out)

def sets(game, module):
    return sorted(glob.glob(os.path.join(canon, game, module, 'bake', label, '*', 'b')))

for game in ('k1', 'tsl'):
    modules = [line.split() for line in open(os.path.join(lists, 'modules-%s.txt' % game)) if line.strip()]
    ok, bad, pngs, size = [], [], 0, 0
    backdrop, noplace, black = [], [], []
    for module, source in modules:
        found = sets(game, module)
        if not found:
            bad.append(module)
            continue
        good = True
        for b in found:
            p = glob.glob(os.path.join(b, '*.png'))
            fp = os.path.join(b, 'fingerprint.txt')
            fmt = re.search(r'^format (\d+)', open(fp).read(), re.M) if os.path.exists(fp) else None
            if not p or not fmt or fmt[1] != '2':
                good = False
            pngs += len(p)
            size += sum(os.path.getsize(f) for f in glob.glob(os.path.join(b, '*')))
            pl = os.path.join(b, 'placeables.txt')
            if not os.path.exists(pl):
                noplace.append(module)
            elif not re.search(r'^format 2$', open(pl).read(), re.M):
                good = False
        (ok if good else bad).append(module)
        c1out = os.path.join(canon, game, module, label + '.out')
        if os.path.exists(c1out) and '--stored-backdrop' in open(c1out).read():
            backdrop.append(module)
        log = os.path.join(canon, game, module, 'bake-%s.log' % label)
        filled = set()
        for b in found:
            f = os.path.join(b, 'fill.txt')
            if os.path.exists(f):
                filled |= {line.split()[0].lower() for line in open(f) if line.strip() and not line.startswith('#')}
        if os.path.exists(log):
            for line in open(log):
                # A room of the table "each room": b's target, b before its
                # scale, b, its scale. A room whose lamps give it (nearly)
                # nothing and that takes no stored fill at load is black.
                m = re.match(r'\s+(\S+): \d+, live [0-9.]+; a .*; b ([0-9.]+) ([0-9.]+) ([0-9.]+) x([0-9.]+)', line)
                if not m:
                    continue
                room, target, final = m[1].lower(), float(m[2]), float(m[4])
                if any(name.startswith(room) for name in filled):
                    continue
                if target > 0.01 and final < 0.1 * target:
                    black.append('%s:%s(%.3f of %.3f)' % (module, room, final, target))
    say('%s: %d modules listed, %d with a good set, failed or bad: %s' % (game, len(modules), len(ok), ' '.join(bad) or 'none'))
    say('  PNGs in b: %d, size of b: %.1f MB' % (pngs, size / 1e6))
    say('  sets without placeables.txt: %s' % (' '.join(noplace) or 'none'))
    say('  --stored-backdrop: %s' % (' '.join(backdrop) or 'none'))
    say('  rooms the log calls unlit: %s' % (' '.join(black) or 'none'))
out.close()
