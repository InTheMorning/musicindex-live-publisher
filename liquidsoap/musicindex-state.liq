# MusicIndex producer support: the reference slot text of publisher ADR 0022.
# It writes state.json in the format musicindex-liquidsoap-state/2 (ADR 0025).
# liquidsoap-now-playing reads that file. A new required field needs a new format version. An optional field does not (ADR 0026).
# Put this text in the AzuraCast Pre-Broadcast slot, after azuracast.handle_jingle_mode.
# Replace INSTANCE on the musicindex_dir line with the instance name of ADR 0022 §The Template Unit.
settings.request.metadata_decoders.recode.exclude := list.append(settings.request.metadata_decoders.recode.exclude(), ["podcast value routes", "podcast chapters"])

# The MusicIndex frame of publisher ADR 0009 in the HLS timed ID3. Test only.
settings.encoder.metadata.export := list.append(settings.encoder.metadata.export(), ["txxx"])
# A track GUID is unique only in its feed. Write the frame only when both GUIDs are present.
def musicindex_id3(m) =
  item = m["podcast item guid"]
  feed = m["podcast feed guid"]
  if item == "" or feed == "" then
    []
  else
    frame = json.stringify(compact=true, {
      schema = "musicindex.hls/1",
      track = {
        artist = m["artist"],
        title = m["title"],
        artwork = null,
        feedGuid = feed,
        itemGuid = item
      },
      value = null
    })
    [("TXXX", "musicindex\000#{frame}")]
  end
end
radio = metadata.map(musicindex_id3, radio)

# The state file for the producer. One writer only: file.write uses one temporary name.
musicindex_dir = "/var/azuracast/musicindex/INSTANCE"
# One reference holds the sequence number, the start time, the metadata,
# the rid of the last track and the jingle flag.
musicindex_state = ref((0, 0., [], "", false))

def musicindex_on_track(m) =
  let (seq, _, _, last_rid, _) = musicindex_state()
  rid = m["rid"]
  # Jingle mode gives a jingle the metadata of the last song, with the same rid.
  jingle = rid != "" and rid == last_rid
  musicindex_state := (seq + 1, time(), m, rid, jingle)
end
source.methods(radio).on_track(synchronous=false, musicindex_on_track)

def musicindex_write() =
  let (seq, started, m, _, jingle) = musicindex_state()
  data = json.stringify({
    format = "musicindex-liquidsoap-state/2",
    seq = seq,
    written = time(),
    live = azuracast.to_live(),
    jingle = jingle,
    track_started = started,
    elapsed = source.methods(radio).elapsed(),
    cue_in = float_of_string(default=0., m["liq_cue_in"]),
    title = m["title"],
    artist = m["artist"],
    song = m["song"],
    album = m["album"],
    track_guid = m["podcast item guid"],
    feed_guid = m["podcast feed guid"],
    value_routes = m["podcast value routes"],
    image = m["podcast image"],
    description = m["podcast description"],
    contributors = m["podcast contributors"],
    chapters = m["podcast chapters"]
  })
  try
    file.write(atomic=true, temp_dir=musicindex_dir, data=data, "#{musicindex_dir}/state.json")
  catch err do
    log(level=2, label="musicindex", "The write of state.json failed: #{err}")
  end
end
thread.run(every=1., musicindex_write)
