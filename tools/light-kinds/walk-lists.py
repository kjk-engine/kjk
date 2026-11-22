# Writes the lists a walk through every baked module needs.
#   python walk-lists.py <worktree root>
# It reads target/light-kinds/batch/modules.txt, a line a module with its
# game, and looks in each module's scratch folder under
# target/light-kinds/game for its newest bake. It writes, into
# target/light-kinds/walk:
#   bakes.txt        every module's folder of baked lightmaps, for
#                    KJK_DETAIL_BAKES
#   modules-k1.txt   the KOTOR 1 modules that have a bake, for
#                    KJK_WALK_MODULES
#   modules-tsl.txt  the same for KOTOR 2
#   chosen.txt       which bake each module shows
#   walked.txt       which bake each module showed before round r1, the
#                    one the owner walked
# A module's approved bake comes first, then the rounds from the newest.
import glob, os, sys

root = sys.argv[1]
game = os.path.join(root, 'target', 'light-kinds', 'game')
out = os.path.join(root, 'target', 'light-kinds', 'walk')
os.makedirs(out, exist_ok=True)
folders = {'152har': 'bake-har', 'end_m01aa': 'bake-gpu'}
approved = {'152har': 'g12', 'end_m01aa': 'g12', '101per': 'g12', '103per': 'g12',
            'danm13': 'h5', 'tat_m17aa': 'a8', 'tat_m20aa': 'e5'}
rounds = ['f1', 't2', 't1', 'b5', 'b4', 'b3', 'b2', 'b1']
bakes, chosen, walked, lists = [], [], [], {'k1': [], 'tsl': []}
for line in open(os.path.join(root, 'target', 'light-kinds', 'batch', 'modules.txt')):
    if not line.strip():
        continue
    which, module = line.split()
    folder = os.path.join(game, folders.get(module, 'bake-' + module), 'bake')
    # Round r1 (22 November 2026) baked every module again after the
    # owner's notes, the approved areas too, so it comes first.
    labels = ['r1'] + ([approved[module]] if module in approved else []) + rounds
    for label in labels:
        found = glob.glob(os.path.join(folder, label, '*', 'b'))
        if found and glob.glob(os.path.join(found[0], '*.png')):
            bakes.append(os.path.abspath(found[0]))
            chosen.append('%s %s %s' % (which, module, label))
            lists[which].append(module)
            break
    else:
        chosen.append('%s %s no bake' % (which, module))
    for label in labels[1:]:
        found = glob.glob(os.path.join(folder, label, '*', 'b'))
        if found and glob.glob(os.path.join(found[0], '*.png')):
            walked.append('%s %s %s' % (which, module, label))
            break
    else:
        walked.append('%s %s no bake' % (which, module))
for name, lines in (('bakes.txt', bakes), ('chosen.txt', chosen), ('walked.txt', walked),
                    ('modules-k1.txt', lists['k1']), ('modules-tsl.txt', lists['tsl'])):
    with open(os.path.join(out, name), 'w', newline='\n') as file:
        file.write('\n'.join(lines) + '\n')
print('%d bakes, %d KOTOR 1 modules, %d KOTOR 2 modules' % (len(bakes), len(lists['k1']), len(lists['tsl'])))
