# Says whether two bakes of one area are the same, lightmap by lightmap.
#   python bake-same.py <one bake's set> <the other bake's set>
# Each argument is a folder of lightmaps, for example
# target/light-kinds/game/bake-103per/bake/c5/103per/b. It prints how many
# lightmaps are the same to the last pixel and the largest difference of any
# pixel, from 0 to 255. It needs Pillow.
import glob, os, sys
from PIL import Image, ImageChops

one, other = sys.argv[1], sys.argv[2]
count = same = worst = 0
for path in sorted(glob.glob(os.path.join(one, '*.png'))):
    twin = os.path.join(other, os.path.basename(path))
    if not os.path.exists(twin):
        print('missing', twin)
        continue
    a, b = Image.open(path).convert('RGB'), Image.open(twin).convert('RGB')
    most = 255 if a.size != b.size else max(high for _, high in ImageChops.difference(a, b).getextrema())
    count, same, worst = count + 1, same + (most == 0), max(worst, most)
print('%d lightmaps, %d the same, largest pixel difference %d' % (count, same, worst))
