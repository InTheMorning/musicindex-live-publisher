#!/usr/bin/env bash

TXTFILE=/tmp/mixxx-now-playing.txt
DBFILE="$HOME/.mixxx/mixxxdb.sqlite"

LAST_HIST_ID=""

while pgrep -i mixxx > /dev/null; do
  # Returns: <history_pt_id>|<artist>|<title>
  ROW=$(
    sqlite3 "$DBFILE" "
      SELECT pt.id || '|' || IFNULL(l.artist,'') || '|' || IFNULL(l.title,'')
      FROM PlaylistTracks pt
      JOIN Playlists p ON p.id = pt.playlist_id
      JOIN library l ON l.id = pt.track_id
      WHERE p.hidden = 2
      ORDER BY pt.pl_datetime_added DESC, pt.id DESC
      LIMIT 1;
    "
  )

  HIST_ID="${ROW%%|*}"
  REST="${ROW#*|}"
  ARTIST="${REST%%|*}"
  TITLE="${REST#*|}"

  # Only write when History gets a new entry (i.e. track actually changes)
  if [[ -n "$HIST_ID" && "$HIST_ID" != "$LAST_HIST_ID" ]]; then
    LAST_HIST_ID="$HIST_ID"

    # Preserve your formatting quirks
    LINE="$(printf '%s|%s\n' "$ARTIST" "$TITLE" | sed 's/-//g' | sed 's/|/ - /g')"
    printf '%s' "$LINE" > "$TXTFILE"
  fi

  sleep 0.5
done