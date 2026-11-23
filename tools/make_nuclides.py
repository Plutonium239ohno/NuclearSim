#!/usr/bin/env python3
"""Convert NUBASE2020 (nubase_4.mas20.txt) into data/nuclides.tsv.

Source: F.G. Kondev et al., Chin. Phys. C45, 030001 (2021),
https://www-nds.iaea.org/amdc/ame2020/nubase_4.mas20.txt

Output, one ground state per line:
    Z  N  mass_excess_keV  half_life_s  decay_modes
half_life_s is "stable", or seconds (0 = particle-unbound / unknown, i.e. instant).
decay_modes is NUBASE's list reduced to MODE=percent (percent may be '?').
"""
import re
import sys

UNITS = {
    'ys': 1e-24, 'zs': 1e-21, 'as': 1e-18, 'fs': 1e-15, 'ps': 1e-12, 'ns': 1e-9,
    'us': 1e-6, 'ms': 1e-3, 's': 1.0, 'm': 60.0, 'h': 3600.0, 'd': 86400.0,
    'y': 31556926.0, 'ky': 31556926e3, 'My': 31556926e6, 'Gy': 31556926e9,
    'Ty': 31556926e12, 'Py': 31556926e15, 'Ey': 31556926e18, 'Zy': 31556926e21,
    'Yy': 31556926e24,
}
HBAR_LN2 = 6.582119569e-22 * 0.6931471805599453  # MeV s


def half_life(t, unit):
    t = t.replace('#', '').strip()
    if t == 'stbl':
        return 'stable'
    if t in ('', 'p-unst'):
        return '0'
    t = t.lstrip('<>~')
    try:
        v = float(t)
    except ValueError:
        return '0'
    if unit in UNITS:
        return '%.6g' % (v * UNITS[unit])
    if unit in ('eV', 'keV', 'MeV'):  # resonance width -> half-life
        gamma = v * {'eV': 1e-6, 'keV': 1e-3, 'MeV': 1.0}[unit]
        return '%.6g' % (HBAR_LN2 / gamma)
    return '0'


def modes(br):
    out = []
    for tok in br.split(';'):
        tok = tok.strip()
        m = re.match(r'^([A-Za-z0-9+\-]+)\s*([=~<>]?)\s*([0-9.eE+\-]*)', tok)
        if not m:
            continue
        name, _, val = m.groups()
        if name == 'IS' or name == 'IT':
            continue
        try:
            float(val)
        except ValueError:
            val = '?'
        out.append('%s=%s' % (name, val))
    return ';'.join(out)


def main(src, dst):
    rows = []
    for line in open(src, encoding='latin-1'):
        if line.startswith('#') or len(line) < 20 or line[7] != '0':
            continue
        a = int(line[0:3])
        z = int(line[4:7])
        mass = line[18:31].replace('#', '').strip()
        if not mass:
            continue
        rows.append((z, a - z, float(mass), half_life(line[69:78], line[78:80].strip()), modes(line[119:])))
    with open(dst, 'w') as f:
        f.write('# Z\tN\tmass_excess_keV\thalf_life_s\tdecay_modes  (from NUBASE2020, Kondev et al. CPC 45 030001)\n')
        for z, n, m, t, d in sorted(rows):
            f.write('%d\t%d\t%.3f\t%s\t%s\n' % (z, n, m, t, d))
    print('%d nuclides -> %s' % (len(rows), dst))


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
