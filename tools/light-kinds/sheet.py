"""sheet.py [pattern]: contact sheets of shots/*.png, 12 a sheet, into sheets/."""
import glob, os, sys
from PIL import Image, ImageDraw
here = os.path.dirname(os.path.abspath(__file__))
files = sorted(glob.glob(os.path.join(here, 'shots', (sys.argv[1] if len(sys.argv) > 1 else '*') + '.png')))
os.makedirs(os.path.join(here, 'sheets'), exist_ok=True)
for at in range(0, len(files), 12):
    sheet = Image.new('RGB', (4 * 480, 3 * 290), 'black')
    for i, f in enumerate(files[at:at + 12]):
        im = Image.open(f).convert('RGB').resize((480, 270))
        sheet.paste(im, ((i % 4) * 480, (i // 4) * 290 + 20))
        ImageDraw.Draw(sheet).text(((i % 4) * 480 + 4, (i // 4) * 290 + 4), os.path.basename(f)[:-4][:75], fill='white')
    sheet.save(os.path.join(here, 'sheets', 'sheet-%02d.jpg' % (at // 12)), quality=80)
print(len(files), 'pictures')
