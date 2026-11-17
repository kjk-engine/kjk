"""build_page.py: the checklist page's files, from light-kinds.json and shots/.

Writes page/kinds.json (each kind with its slug, its group's key, its
picture, its proposal (propose.py) and its texture as a small data URI),
page/shots/<slug>.jpg, a picture 640 wide with a cutscene's black bars cut
off, and page/shots/<slug>-shine.jpg, the proposal shining (shine.py). A slug listed in
bad.txt gets no picture.
"""
import base64, io, json, os, re
from PIL import Image
from propose import propose
from round import fixes

HERE = os.path.dirname(os.path.abspath(__file__))
PAGE = os.path.join(HERE, 'page')
os.makedirs(os.path.join(PAGE, 'shots'), exist_ok=True)


def slug(kind):
    return kind['game'] + '-' + re.sub(r'[^a-z0-9]+', '_', (kind['group'] + ' ' + kind['name']).lower()).strip('_')


def without_bars(picture):
    """The picture without the black bars a cutscene draws above and below."""
    grey = picture.convert('L')
    width, height = grey.size
    rows = [max(grey.crop((0, y, width, y + 1)).getdata()) for y in range(height)]
    top = next((y for y, v in enumerate(rows) if v > 8), 0)
    bottom = next((y for y in range(height - 1, -1, -1) if rows[y] > 8), height - 1)
    if bottom - top < height // 3:
        return picture
    return picture.crop((0, top, width, bottom + 1))


# What the owner said of each kind of the current round and what changed
# (round.py). The page shows these alone until asked for the rest.
FIXES = fixes()

kinds = json.load(open(os.path.join(HERE, 'light-kinds.json')))
chosen = json.load(open(os.path.join(HERE, 'shots.json')))
bad_file = os.path.join(HERE, 'bad.txt')
bad = set(open(bad_file).read().split()) if os.path.exists(bad_file) else set()

for old in os.listdir(os.path.join(PAGE, 'shots')):
    os.remove(os.path.join(PAGE, 'shots', old))

pictured = {}
for kind in kinds:
    name = slug(kind)
    kind['slug'] = name
    kind['group_key'] = 'surface' if kind['group'].startswith('glowing surface') else 'lamp'
    kind['shot'] = kind['shot_place'] = kind['texture_data'] = kind['shine'] = None
    kind['proposal'] = propose(kind)
    if name in FIXES:
        kind['fix'] = FIXES[name]
    raw = os.path.join(HERE, 'shots', name + '.png')
    kept = os.path.join(HERE, 'kept', name + '.jpg')
    if name not in bad and name in chosen and (os.path.exists(raw) or os.path.exists(kept)):
        if os.path.exists(raw):
            picture = without_bars(Image.open(raw).convert('RGB'))
            picture = picture.resize((640, round(640 * picture.height / picture.width)), Image.LANCZOS)
            os.makedirs(os.path.dirname(kept), exist_ok=True)
            picture.save(kept, quality=82)
        thumb = Image.open(kept)
        # A cutscene's subtitle keeps part of the lower bar. The picture
        # between the bars is 209 rows, and the light is at its middle.
        if 209 < thumb.height < 300:
            thumb = thumb.crop((0, 0, thumb.width, 209))
        thumb.save(os.path.join(PAGE, 'shots', name + '.jpg'), quality=82)
        place = chosen[name]
        kind['shot'] = 'shots/' + name + '.jpg'
        kind['shot_place'] = place['module'] + (', room ' + place['room'] if place.get('room') else '')
        pictured[kind['game']] = pictured.get(kind['game'], 0) + kind['count']
        # The proposal shining in the darkened room, cut the same way.
        raw = os.path.join(HERE, 'shots', name + '.shine.png')
        kept = os.path.join(HERE, 'kept', name + '.shine.jpg')
        if os.path.exists(raw):
            picture = without_bars(Image.open(raw).convert('RGB'))
            picture = picture.resize((640, round(640 * picture.height / picture.width)), Image.LANCZOS)
            picture.save(kept, quality=82)
        if os.path.exists(kept):
            thumb = Image.open(kept)
            if 209 < thumb.height < 300:
                thumb = thumb.crop((0, 0, thumb.width, 209))
            thumb.save(os.path.join(PAGE, 'shots', name + '-shine.jpg'), quality=82)
            kind['shine'] = 'shots/' + name + '-shine.jpg'
    if kind.get('picture'):
        texture = Image.open(os.path.join(HERE, 'textures', kind['picture'])).convert('RGB')
        texture.thumbnail((64, 64))
        body = io.BytesIO()
        texture.save(body, 'JPEG', quality=80)
        kind['texture_data'] = 'data:image/jpeg;base64,' + base64.b64encode(body.getvalue()).decode()

for kind in kinds:
    del kind['examples']   # The page does not use them, and their lists are long.
json.dump(kinds, open(os.path.join(PAGE, 'kinds.json'), 'w'), separators=(',', ':'))
for game in ('k1', 'tsl'):
    of_game = [k for k in kinds if k['game'] == game]
    total = sum(k['count'] for k in of_game)
    print(game, len(of_game), 'kinds,', sum(1 for k in of_game if k['shot']), 'with a picture, which cover',
          '%.1f%%' % (100 * pictured.get(game, 0) / total), 'of', total, 'lights')
print('kinds.json', os.path.getsize(os.path.join(PAGE, 'kinds.json')) // 1024, 'KB')
