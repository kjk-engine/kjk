# Three or more columns of pictures side by side, four poses a sheet, for
# looking at a round before the owner sees it. It needs Pillow.
#   python bake-sheet.py <area folder> <label,label,...> <title,title,...>
# <area folder> is the path of the area's scratch folder. Each label is a
# folder of shots/ that bake-shoot.sh filled. The sheets are written to
# cmp-1.png, cmp-2.png and so on in the area's folder, over any older ones.
import sys, glob, os
from PIL import Image, ImageDraw
d = sys.argv[1]; cols = sys.argv[2].split(','); labels = sys.argv[3].split(',')
names = sorted(os.path.basename(f)[:-4] for f in glob.glob(f'{d}/shots/{cols[0]}/*.png'))
w, h = 640, 360
for part in range(0, len(names), 4):
    ns = names[part:part + 4]
    sheet = Image.new('RGB', (w * len(cols), h * len(ns)))
    for r, n in enumerate(ns):
        for c, col in enumerate(cols):
            im = Image.open(f'{d}/shots/{col}/{n}.png').convert('RGB').resize((w, h))
            ImageDraw.Draw(im).text((200, 6), labels[c], fill=(255, 255, 0))
            sheet.paste(im, (c * w, r * h))
    sheet.save(f'{d}/cmp-{part // 4 + 1}.png')
