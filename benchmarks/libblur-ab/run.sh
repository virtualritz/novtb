#!/usr/bin/env bash
# Builds the harness once per threading backend, then for every case runs the
# backends in alternation (A B C A B C ...) in short slices. Interleaving per
# case, not per full sweep, spreads bursts of outside load over all backends.
# Before every slice it waits until the machine is idle (no compiler running,
# outside CPU use under IDLE_PCT percent) and appends that reading to the row.
#   ROUNDS=5 BUDGET_MS=1000 FILTER=gauss ./run.sh > results.csv
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/harness"
build() { # <name> <novtb path>
	cargo build --release --quiet --target-dir "$here/target/$1" \
		--config "patch.crates-io.novtb.path=\"$2\""
}
build novtb "$here/../.."
build rayon "$here/shim"
novtb="$here/target/novtb/release/libblur-ab"
rayon="$here/target/rayon/release/libblur-ab"
export BUDGET_MS="${BUDGET_MS:-1000}"
# Percent of all CPUs busy over half a second, from /proc/stat.
busy_pct() {
	local a b
	read -ra a < <(head -1 /proc/stat)
	sleep 0.5
	read -ra b < <(head -1 /proc/stat)
	local idle=$(( (b[4] + b[5]) - (a[4] + a[5]) )) total=0 i
	for i in 1 2 3 4 5 6 7 8; do total=$(( total + b[i] - a[i] )); done
	echo $(( 100 * (total - idle) / total ))
}
wait_idle() {
	local pct
	while :; do
		pct=$(busy_pct)
		if ! pgrep -x 'rustc|cargo|cc|cc1|cc1plus|ld|lld|sccache-dist' >/dev/null &&
			(( pct < ${IDLE_PCT:-12} )); then
			echo "$pct"
			return
		fi
		echo "waiting for idle: ${pct}% busy" >&2
		sleep 5
	done
}
slice() { # <backend> <binary> [env...]
	local pct
	pct=$(wait_idle)
	env CASE="$case" BACKEND="$1" "${@:3}" "$2" | sed "s/\$/,$pct/"
}
echo "backend,scenario,case,threads,median_us,p10_us,p90_us,samples,digest,peak_rss_mib,busy_before_pct"
LIST=1 "$novtb" | while read -r case; do
	for _ in $(seq "${ROUNDS:-5}"); do
		slice novtb "$novtb"
		slice rayon-static "$rayon" NOVTB_SHIM=static
		slice rayon-steal "$rayon" NOVTB_SHIM=steal
	done
done
