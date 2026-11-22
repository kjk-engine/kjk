"""Makes one picture of a module's shots for the owner to compare.

    python -I model-sheet.py <shots folder> <mask label> <out.png> <pose,pose,...> <label>...

Each row is a pose and each column a label, as model-shoot.sh named them.
Each picture is captioned with its pose, its label and the model's
brightness over its floor's (model-measure.py). It needs Pillow.
"""
import importlib.util
import os
import sys

from PIL import Image, ImageDraw

spec = importlib.util.spec_from_file_location(
    "model_measure", os.path.join(os.path.dirname(os.path.abspath(__file__)), "model-measure.py"))
measure = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measure)

folder, mask, out, poses = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4].split(',')
labels = sys.argv[5:]
table = measure.table(folder, mask, labels)
W, H = 480, 270
sheet = Image.new('RGB', (W * len(labels), (H + 20) * len(poses)), 'white')
draw = ImageDraw.Draw(sheet)
for row, pose in enumerate(poses):
    for column, label in enumerate(labels):
        path = os.path.join(folder, label, pose + '.png')
        if os.path.exists(path):
            sheet.paste(Image.open(path).convert('RGB').resize((W, H)), (column * W, row * (H + 20) + 20))
        ratio = table.get(pose) and table[pose].get(label)
        caption = '%s  %s  %s' % (pose, label, '%.2f' % ratio[2] if ratio else '')
        draw.text((column * W + 4, row * (H + 20) + 4), caption, fill='black')
sheet.save(out)
