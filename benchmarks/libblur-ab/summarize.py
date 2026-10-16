"""Summarize run.sh CSV. run.sh runs the backends back to back per round, so
each round gives a paired ratio novtb/backend (>1.00 = rayon faster). Reported
per case: medians, the median paired ratio, and how many rounds rayon won.
Pass --md for a Markdown table."""
import csv, statistics, sys
from collections import defaultdict

md = "--md" in sys.argv
rows = list(csv.DictReader(open([a for a in sys.argv[1:] if a != "--md"][0])))
times = defaultdict(lambda: defaultdict(list))
rss = defaultdict(lambda: defaultdict(list))
busy = []
digests = defaultdict(set)
order = []
for r in rows:
    key = (r["scenario"], r["case"], r["threads"])
    if key not in order:
        order.append(key)
    times[key][r["backend"]].append(float(r["median_us"]))
    rss[key][r["backend"]].append(int(r["peak_rss_mib"]))
    busy.append(int(r["busy_before_pct"]))
    digests[key].add(r["digest"])

head = ["case", "threads", "novtb ms", "rayon-static ms", "rayon-steal ms", "novtb/static", "novtb/steal", "steal won"]
if md:
    print("| " + " | ".join(head) + " |")
    print("|" + "|".join(["---"] + ["---:"] * (len(head) - 1)) + "|")
else:
    print(f"{head[0]:30} {'thr':>3} {'novtb':>9} {'static':>9} {'steal':>9} {'n/stat':>7} {'n/steal':>7} {'won':>5}")
agg = defaultdict(list)
for key in order:
    t = times[key]
    n = t["novtb"]
    cells = [key[1], key[2]] + [f"{statistics.median(t[b]) / 1e3:.2f}" for b in ("novtb", "rayon-static", "rayon-steal")]
    for b in ("rayon-static", "rayon-steal"):
        paired = [a / x for a, x in zip(n, t[b])]
        agg[(key[0], b)].append(statistics.median(paired))
        cells.append(f"{statistics.median(paired):.2f}")
    cells.append(f"{sum(a > x for a, x in zip(n, t['rayon-steal']))}/{len(n)}")
    if len(digests[key]) != 1:
        cells[0] += " DIGEST MISMATCH"
    if md:
        print("| " + " | ".join(cells) + " |")
    else:
        print(f"{cells[0]:30} {cells[1]:>3} {cells[2]:>9} {cells[3]:>9} {cells[4]:>9} {cells[5]:>7} {cells[6]:>7} {cells[7]:>5}")
print()
for (scen, b), v in agg.items():
    print(f"{scen}: geomean novtb/{b} = {statistics.geometric_mean(v):.3f} (min {min(v):.2f}, max {max(v):.2f}, {len(v)} cases)")
for key in order:
    if key[0] == "nested_alloc":
        print(f"{key[1]} peak RSS MiB (median): " + ", ".join(
            f"{b} {statistics.median(rss[key][b]):.0f}" for b in ("novtb", "rayon-static", "rayon-steal")))
print(f"background CPU before slices: median {statistics.median(busy)}%, max {max(busy)}%")
