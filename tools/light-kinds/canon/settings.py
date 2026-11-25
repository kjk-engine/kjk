# Prints, as shell assignments, the settings a module is baked with in the
# canon round. A module with an entry in data/lights/bake.toml takes what
# tools/light-kinds/bake.sh would use. A module with no entry there takes
# what the owner walked in rounds r1 and r2: the sun, the flags and the
# levels of that bake. walked.txt says which bake each module showed then
# (b1 to b5, or a label of an approved bake), and walked/<module>.txt holds
# its levels. auto-suns.txt, auto-suns2.txt and b3.txt to b5.txt hold the
# suns and flags of the batch rounds b1 to b5.
#   settings.py <canon folder> <root> <game> <module> <settings from>
# <canon folder> is the folder of this file and <root> the repository's.
# <settings from> is the module whose settings are taken, which is the
# module itself unless it shares another module's area.
import os, shlex, sys, tomllib

tools, root, game, module, source = sys.argv[1:6]
data = tomllib.load(open(os.path.join(root, 'data', 'lights', 'bake.toml'), 'rb'))
entries = data.get('modules', {})
out = {}

def say(key, value):
    out[key] = value

walked = {}
for line in open(os.path.join(tools, 'walked.txt')):
    parts = line.split()
    if len(parts) >= 3:
        walked[parts[1]] = parts[2]

if source in entries or walked.get(source, 'no') == 'no':
    entry = entries.get(source, data['default'])
    say('ORIGIN', 'bake.toml' if source in entries else 'default')
    if 'levels' in entry:
        say('MODE', 'levels')
        say('LEVELS', os.path.join(root, 'data', 'lights', entry['levels']))
        say('OF', entry.get('of', 'original'))
        say('SCALE', str(entry.get('scale', 1.0)))
    else:
        say('MODE', 'share')
        say('SHARE', str(entry['share']))
        say('CAP', str(entry.get('ceiling', 'none')))
        say('OF', entry.get('of', 'original'))
    own = entries.get(source, {})
    sun = own.get('sun')
    sunargs = ''
    if sun:
        sunargs = ' '.join(str(n) for key in ('toward', 'colour', 'sky', 'toward2', 'colour2') for n in sun.get(key, [])) + ' ' + str(sun.get('width', ''))
    say('SUN', sunargs.strip() if sun else '')
    say('SKYLIGHTS', ' '.join(own.get('sun', {}).get('skylights', [])))
    flags = [] if own.get('even', True) else ['--no-even']
    if own.get('fill') == 'stored':
        flags.append('--fill-stored')
    if own.get('every_room'):
        flags.append('--every-room')
    if own.get('backdrop') == 'stored':
        flags.append('--stored-backdrop')
    for key in ('least', 'most', 'floor'):
        if key in own:
            flags += ['--' + key, str(own[key])]
    say('FLAGS', ' '.join(flags))
    say('DARK_UNLIT', '1' if own.get('dark_unlit') else '')
else:
    w = walked[source]
    say('ORIGIN', 'walked ' + w)
    say('MODE', 'walked')
    say('LEVELS', os.path.join(tools, 'walked', source + '.txt'))
    say('OF', 'lit')
    sun, flags = '', ''

    def last(path):
        found = ''
        for line in open(path):
            if line.startswith(source + ' '):
                found = line.rstrip('\n').split(' ', 1)[1]
        return found

    batch = tools
    if w == 'b1':
        sun = last(os.path.join(batch, 'auto-suns.txt'))
        flags = '--no-even' if sun else ''
    elif w == 'b2':
        sun = last(os.path.join(batch, 'auto-suns2.txt'))
        flags = '--no-even'
    elif w in ('b3', 'b4', 'b5'):
        sun = last(os.path.join(batch, 'auto-suns2.txt'))
        row = last(os.path.join(batch, w + '.txt'))
        flags = row.split(' ', 1)[1] if ' ' in row else ''
    say('SUN', sun)
    say('SKYLIGHTS', '')
    say('FLAGS', flags)
    say('DARK_UNLIT', '')

if 'LEVELS' in out:
    out['LEVELS'] = out['LEVELS'].replace('\\', '/')
for key, value in out.items():
    print('%s=%s' % (key, shlex.quote(value)))
