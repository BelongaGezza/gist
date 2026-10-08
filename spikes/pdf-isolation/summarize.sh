#!/bin/bash
# Usage: summarize.sh bench.tsv  -> per (file, mode): median wall_ms, host_rss MiB, helper_rss MiB, json bytes, input bytes, outcome
# Median of N runs; also prints min and max wall so outliers stay visible.
awk -F'\t' '
function fld(s, k,   a, n, i, kv) { n = split(s, a, "\t"); for (i = 1; i <= n; i++) { if (index(a[i], k "=") == 1) return substr(a[i], length(k) + 2) } return "" }
{
  key = $2 "|" $1
  w = $4; sub("wall_ms=", "", w)
  h = $5; sub("host_rss=", "", h)
  p = $6; sub("helper_rss=", "", p)
  j = $7; sub("json_bytes=", "", j)
  b = $8; sub("in_bytes=", "", b)
  o = $9; sub("outcome=", "", o)
  n[key]++; W[key, n[key]] = w + 0; H[key] = h + 0; P[key] = p + 0; J[key] = j; B[key] = b; O[key] = o
  if (!(key in seen)) { seen[key] = 1; order[++cnt] = key }
}
END {
  printf "%-28s %-9s %9s %9s %9s %9s %9s %10s %9s  %s\n", "file", "mode", "med_ms", "min_ms", "max_ms", "host_MiB", "help_MiB", "json_B", "in_B", "outcome"
  for (c = 1; c <= cnt; c++) {
    k = order[c]; m = n[k]
    for (i = 1; i <= m; i++) t[i] = W[k, i]
    for (i = 2; i <= m; i++) { v = t[i]; j2 = i - 1; while (j2 > 0 && t[j2] > v) { t[j2 + 1] = t[j2]; j2-- } t[j2 + 1] = v }
    med = (m % 2) ? t[(m + 1) / 2] : (t[m / 2] + t[m / 2 + 1]) / 2
    split(k, kk, "|")
    printf "%-28s %-9s %9.1f %9.1f %9.1f %9.1f %9.1f %10s %9s  %s\n", kk[1], kk[2], med, t[1], t[m], H[k] / 1048576, P[k] / 1048576, J[k], B[k], O[k]
  }
}' "$1"
