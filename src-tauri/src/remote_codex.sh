# Run on an SSH host by Agent Gauge (read-only). Prints, for each Codex
# session touched in the last 24 hours, only metadata:
#   H <tab> hostname          (once, before any sessions)
#   F <tab> modified-epoch <tab> now-epoch
#   M <tab> "key":"value"      (id, cwd, originator, parent_thread_id, agent_nickname)
#   T <tab> "key":number       (last turn's input_tokens, model_context_window)
# Never prints conversation text.
printf 'H\t%s\n' "$(hostname)"
d="${CODEX_HOME:-$HOME/.codex}/sessions"
[ -d "$d" ] || exit 0
now=$(date +%s)
find "$d" -type f -name '*.jsonl' -mmin -1440 2>/dev/null | while IFS= read -r f; do
  m=$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f")
  printf 'F\t%s\t%s\n' "$m" "$now"
  head -c 65536 "$f" | head -n 1 \
    | grep -o -E '"(id|cwd|originator|parent_thread_id|agent_nickname)":"[^"]*"' \
    | head -n 8 | sed 's/^/M\t/'
  tail -c 65536 "$f" | grep '"token_count"' | tail -n 1 \
    | grep -o -E '"(input_tokens|model_context_window)":[0-9]+' \
    | tail -n 2 | sed 's/^/T\t/'
done
