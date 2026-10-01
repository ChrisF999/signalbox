#!/usr/bin/env python3
# Writes wtt-synthetic.bbox.html: a made-up Working Timetable in the form
# `pdftotext -bbox` gives for LU WTTs (words with their boxes), for the tests
# of ts2_import::wtt. Fictional trains 301-303, times and running times
# (Bank platform 7 westbound 3 1/4 min, 8 westbound 3 3/4, 7 eastbound 4 1/2,
# 8 eastbound 3 3/4); no figure in it comes from TfL's timetable. Rerun after editing:
#   python3 crates/ts2-import/tests/data/wtt-synthetic.py > crates/ts2-import/tests/data/wtt-synthetic.bbox.html
out = []
def word(x0, y0, w, h, t):
    t = t.replace('&', '&amp;')
    out.append(f'    <word xMin="{x0:.6f}" yMin="{y0:.6f}" xMax="{x0+w:.6f}" yMax="{y0+h:.6f}">{t}</word>')
def text(x, y, s, h=6.07, cw=3.6):
    for part in s.split():
        w = cw * len(part)
        word(x, y, w, h, part); x += w + 1.8
def centred(c, y, s, h=4.55):
    width = sum(3.0 * len(p) for p in s.split()) + 1.8 * (len(s.split()) - 1)
    text(c - width / 2, y, s, h, 3.0)
def time(c, y, hh, mm, frac=None, stacked=None, wash=False):
    if wash:
        word(c - 9.87, y, 18.16, 5.99, f'{hh}z{mm}'); x1 = c + 8.29
    else:
        word(c - 9.87, y, 7.18, 5.99, hh); word(c + 1.11, y, 7.18, 5.99, mm); x1 = c + 8.29
    if frac: word(x1, y + 0.17, 1.82, 5.89, frac)
    if stacked:
        n, d = stacked
        word(x1, y + 0.12, 1.63, 3.04, n); word(x1 + 0.2, y + 3.02, 1.63, 3.04, d)
def page(direction, rows, cols):
    out.append('  <page width="595.220000" height="842.000000">')
    text(42.83, 54.31, 'MONDAYS TO FRIDAYS', 8.4)
    text(466.45 if direction == 'WESTBOUND' else 42.83, 54.31 if direction == 'WESTBOUND' else 62.0, direction, 8.4)
    labels = {'train': 'Train No.', 'trip': 'Trip No.', 'crew': 'Crew Running No.', 'notes': 'Notes', 'pf': 'Platform No.',
              'bank': 'BANK', 'arr': 'arr.', 'dep': 'dep.', 'siding': 'Waterloo Siding', 'depot': 'Waterloo Depot',
              'toform': 'To form', 'by': 'By Crew Running No.'}
    for key, y in rows:
        x = 96.14 if key in ('arr', 'dep') else 49.45 if key == 'pf' else 38.2
        text(x, y, labels[key])
        for dots in (118.08,):
            text(dots, y, '.')
        if key == 'arr':
            text(38.2, y + 3.12, 'WATERLOO')
    y = dict(rows)
    for c, col in cols:
        for key, v in col.items():
            if key == 'extra':
                for dy, s in v:
                    centred(c, y['notes'] + dy, s)
            elif isinstance(v, tuple):
                time(c, y[key] + 0.08, *v[:2], **(v[2] if len(v) > 2 else {}))
            else:
                centred(c, y[key] + 0.08, v)
    out.append('  </page>')

out.append('<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><html xmlns="http://www.w3.org/1999/xhtml">')
out.append('<head>\n<title>Synthetic working timetable (test data)</title>\n</head>\n<body>\n<doc>')
# Page 1: a contents page (no train service).
out.append('  <page width="595.220000" height="842.000000">')
text(200, 100, 'SYNTHETIC LINE WORKING TIMETABLE')
text(200, 120, 'Train Service MONDAYS TO FRIDAYS')
out.append('  </page>')
W = [('train', 73.21), ('trip', 85.71), ('crew', 98.20), ('notes', 116.95), ('pf', 129.54), ('bank', 135.70),
     ('arr', 141.95), ('dep', 148.20), ('siding', 154.53), ('depot', 160.70), ('toform', 173.20), ('by', 179.45)]
page('WESTBOUND', W, [
    (142.0, {'train': '303', 'trip': '1', 'crew': '9', 'notes': 'Ety', 'extra': [(-6.16, 'Start')],
             'arr': 'Pfm 26', 'dep': ('05', '50'), 'depot': ('05', '52'), 'toform': 'Stop'}),
    (185.2, {'train': '302', 'trip': '1', 'crew': '2', 'notes': 'TThX', 'extra': [(-6.16, 'Start')], 'pf': '8',
             'bank': ('06', '05'), 'arr': ('06', '08', {'frac': '34'}), 'dep': ('06', '10'), 'siding': ('06', '11'), 'toform': ('06', '12')}),
    (206.8, {'train': '302', 'trip': '2', 'crew': '2', 'notes': 'TThO', 'extra': [(-6.16, 'Start')], 'pf': '7',
             'bank': ('06', '05'), 'arr': ('06', '08', {'frac': '14'}), 'dep': ('06', '10'), 'siding': ('06', '11'), 'toform': ('06', '12')}),
    (250.0, {'train': '301', 'trip': '2', 'crew': '1', 'pf': '7', 'bank': ('06', '09'), 'arr': ('06', '12', {'frac': '14'}),
             'dep': ('06', '13', {'frac': '12'}), 'siding': ('06', '14', {'frac': '12'}), 'toform': ('06', '16'), 'by': '2'}),
    (336.5, {'train': '302', 'trip': '5', 'crew': '2', 'notes': 'WO', 'pf': '8', 'bank': ('06', '34', {'frac': '12'}),
             'arr': ('06', '38', {'stacked': ('1', '4')}), 'toform': 'Stop'}),
    (293.3, {'train': '301', 'trip': '4', 'crew': '1', 'pf': '7', 'bank': ('06', '24'), 'arr': ('06', '27', {'frac': '14'}),
             'dep': ('06', '28', {'frac': '12'}), 'depot': ('06', '30', {'frac': '12', 'wash': True}),
             'extra': [(49.0, 'Shed Rd')], 'toform': 'Stop'}),
])
E = [('train', 73.21), ('trip', 85.71), ('crew', 98.20), ('notes', 116.95), ('depot', 123.20), ('siding', 129.45),
     ('arr', 135.70), ('dep', 141.95), ('bank', 148.20), ('pf', 154.45), ('toform', 166.95), ('by', 173.20)]
page('EASTBOUND', E, [
    (142.0, {'train': '301', 'trip': '1', 'crew': '1', 'extra': [(-6.16, 'Start')], 'depot': ('06', '00'),
             'arr': ('06', '01', {'frac': '12'}), 'dep': ('06', '03'), 'bank': ('06', '07', {'frac': '12'}), 'pf': '7', 'toform': ('06', '09')}),
    (185.2, {'train': '302', 'trip': '3', 'crew': '2', 'siding': ('06', '12'), 'arr': ('06', '12', {'frac': '34'}),
             'dep': ('06', '13', {'frac': '12'}), 'bank': ('06', '17', {'frac': '14'}), 'pf': '8', 'toform': ('06', '34', {'stacked': ('1', '2')})}),
    (228.4, {'train': '301', 'trip': '3', 'crew': '1', 'siding': ('06', '16'), 'arr': ('06', '16', {'frac': '34'}),
             'dep': ('06', '18'), 'bank': ('06', '22', {'frac': '12'}), 'pf': '7', 'toform': ('06', '24')}),
])
# A Saturday page: never read.
out.append('  <page width="595.220000" height="842.000000">')
text(42.83, 54.31, 'SATURDAYS WESTBOUND', 8.4)
text(38.2, 73.21, 'Train No.'); text(140, 73.29, '309', 4.55)
out.append('  </page>')
out.append('</doc>\n</body>\n</html>')
print('\n'.join(out))
