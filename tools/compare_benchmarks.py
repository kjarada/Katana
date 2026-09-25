# tools/compare_benchmarks.py <baseline.json> <candidate.json> [threshold-percent]
# tools/compare_benchmarks.py --alternate ROUNDS FILTER NAME=EXE [NAME=EXE ...]
#
# Compares Google Benchmark runs and reports what actually moved.
#
# It exists because "is the new build faster?" is a question about NOISE as
# much as about speed: run the same binary twice and individual benchmarks
# move by a few per cent. Anything inside the threshold is reported as noise
# rather than as a result, and the summary is the MEDIAN of the per-benchmark
# ratios, which one wild outlier cannot move.
#
# The first form compares two saved runs. It is only as good as the two runs:
# on this machine, with the application open or a build going, a whole run can
# drift by up to 2x, and docs/performance.md records a regression that two
# back-to-back runs "found" and alternating runs did not reproduce. The
# --alternate form is for that: copy each build's katana_benchmarks.exe aside,
# and it runs them round by round, reversing the order every round, so whatever
# the machine is doing lands on all of them alike, then prints the minimum and
# the median of every repetition of every benchmark for each binary.
import io
import json
import os
import statistics
import subprocess
import sys


def load(path):
    with io.open(path, encoding='utf-8') as handle:
        data = json.load(handle)
    out = {}
    for entry in data.get('benchmarks', []):
        # Prefer the MEDIAN of repeated runs where there is one: a mean is
        # pulled by a single descheduled iteration, a median is not.
        if entry.get('run_type') == 'aggregate':
            if entry.get('aggregate_name') != 'median':
                continue
            name = entry['name'].rsplit('_median', 1)[0]
        else:
            name = entry['name']
            if name in out:
                continue  # an aggregate already answered for this one
        # cpu_time is what the code costs; real_time includes scheduling.
        out[name] = (entry['cpu_time'], entry.get('time_unit', 'ns'))
    return out


def alternate(rounds, pattern, binaries):
    env = dict(os.environ)
    # The benchmark links the MSYS2 runtime; without it on PATH it cannot start.
    env['PATH'] = r'C:\msys64\ucrt64\bin' + os.pathsep + env.get('PATH', '')
    times = {}  # (binary name, benchmark) -> [ms, one per repetition]
    for round_index in range(rounds):
        order = binaries if round_index % 2 == 0 else list(reversed(binaries))
        for name, exe in order:
            out = subprocess.run([exe, '--benchmark_filter=' + pattern,
                                  '--benchmark_repetitions=3', '--benchmark_min_time=0.2s',
                                  '--benchmark_format=json'],
                                 capture_output=True, text=True, env=env, check=True).stdout
            for entry in json.loads(out)['benchmarks']:
                if entry.get('run_type') != 'iteration':
                    continue
                scale = {'ms': 1.0, 'us': 1e-3, 'ns': 1e-6}[entry['time_unit']]
                times.setdefault((name, entry['run_name']), []).append(entry['real_time'] * scale)
    names = [name for name, _ in binaries]
    benchmarks = sorted({key[1] for key in times})
    print('%-40s' % 'min / median' + ''.join('%24s' % name for name in names))
    for benchmark in benchmarks:
        row = '%-40s' % benchmark[:40]
        # One unit a row, the largest that keeps its fastest sample at 1 or
        # more: in ms to two places, a sub-microsecond case read 0.00 / 0.00.
        fastest = min(min(samples) for samples in
                      (times.get((name, benchmark), []) for name in names) if samples)
        unit, scale = next((u, f) for u, f in (('ms', 1.0), ('us', 1e3), ('ns', 1e6))
                           if fastest * f >= 1.0 or u == 'ns')
        for name in names:
            samples = times.get((name, benchmark), [])
            row += '%24s' % ('%.2f / %.2f %s' % (min(samples) * scale,
                                                 statistics.median(samples) * scale, unit)
                             if samples else '-')
        print(row)
    print('samples per cell: %d (%d rounds x 3 repetitions)' % (rounds * 3, rounds))


def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--alternate':
        if len(sys.argv) < 6:
            sys.exit('usage: compare_benchmarks.py --alternate ROUNDS FILTER NAME=EXE NAME=EXE...')
        alternate(int(sys.argv[2]), sys.argv[3], [a.split('=', 1) for a in sys.argv[4:]])
        return
    if len(sys.argv) < 3:
        sys.exit('usage: compare_benchmarks.py <baseline.json> <candidate.json> [threshold%]')
    threshold = float(sys.argv[3]) if len(sys.argv) > 3 else 5.0
    baseline = load(sys.argv[1])
    candidate = load(sys.argv[2])

    shared = [n for n in baseline if n in candidate]
    missing = [n for n in baseline if n not in candidate]
    added = [n for n in candidate if n not in baseline]

    rows = []
    for name in shared:
        before, unit = baseline[name]
        after, _ = candidate[name]
        if before <= 0.0:
            continue
        change = (after - before) / before * 100.0
        rows.append((change, name, before, after, unit))
    rows.sort()

    faster = [r for r in rows if r[0] <= -threshold]
    slower = [r for r in rows if r[0] >= threshold]

    print('%d benchmarks compared (%d only in baseline, %d only in candidate)'
          % (len(rows), len(missing), len(added)))
    print('threshold for a real change: %.1f%%' % threshold)
    print()
    ratios = [r[0] for r in rows]
    print('median change   %+.2f%%' % statistics.median(ratios))
    print('mean change     %+.2f%%' % statistics.fmean(ratios))
    print('spread          %.2f%% (stdev)' % (statistics.pstdev(ratios) if len(ratios) > 1 else 0.0))
    print('faster than %.0f%%: %d      slower than %.0f%%: %d      within noise: %d'
          % (threshold, len(faster), threshold, len(slower), len(rows) - len(faster) - len(slower)))

    def show(title, group):
        if not group:
            return
        print()
        print(title)
        print('  %-46s %12s %12s %9s' % ('benchmark', 'baseline', 'candidate', 'change'))
        for change, name, before, after, unit in group:
            print('  %-46s %10.3f%s %10.3f%s %+8.1f%%'
                  % (name[:46], before, unit, after, unit, change))

    show('FASTER:', faster[:20])
    show('SLOWER:', list(reversed(slower))[:20])


if __name__ == '__main__':
    main()
