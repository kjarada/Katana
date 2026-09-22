# tools/compare_benchmarks.py <baseline.json> <candidate.json> [threshold-percent]
#
# Compares two Google Benchmark JSON runs and reports what actually moved.
#
# It exists because "is the new build faster?" is a question about NOISE as
# much as about speed: run the same binary twice and individual benchmarks
# move by a few per cent. Anything inside the threshold is reported as noise
# rather than as a result, and the summary is the MEDIAN of the per-benchmark
# ratios, which one wild outlier cannot move.
import io
import json
import statistics
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


def main():
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
