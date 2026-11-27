# MusicIndex producer support: the reference slot text of publisher ADR 0041 §The State File Contract.
# It writes state.json in the format musicindex-liquidsoap-state/2 (ADR 0041 §The State File Contract).
# liquidsoap-now-playing reads that file. A new required field needs a new format version. An optional field does not (ADR 0041 §The State File Contract).
# It also writes the picture file of ADR 0041 §The Slot Text Writes The Picture File into the same folder. The producer reads it.
# Put this text in the AzuraCast Pre-Broadcast slot, after azuracast.handle_jingle_mode.
# Replace INSTANCE on the musicindex_dir line with the instance name of ADR 0041 §The Template Unit.
settings.request.metadata_decoders.recode.exclude := list.append(settings.request.metadata_decoders.recode.exclude(), ["podcast value routes", "podcast chapters"])

# The MusicIndex frame of publisher ADR 0038 in the HLS timed ID3.
settings.encoder.metadata.export := list.append(settings.encoder.metadata.export(), ["txxx"])

# The state file for the producer. One writer only: file.write uses one temporary name.
musicindex_dir = "/var/azuracast/musicindex/INSTANCE"
# One reference holds the sequence number, the start time, the metadata,
# the rid of the last track and the jingle flag.
musicindex_state = ref((0, 0., [], "", false))
# True when the last track mark came with no metadata.
musicindex_pending = ref(false)

def musicindex_set(started, m) =
  let (seq, _, _, last_rid, _) = musicindex_state()
  rid = m["rid"]
  # Jingle mode gives a jingle the metadata of the last song, with the same rid.
  jingle = rid != "" and rid == last_rid
  # A track with no metadata keeps the rid of the last track for this test.
  next_rid = if rid == "" then last_rid else rid end
  musicindex_state := (seq + 1, started, m, next_rid, jingle)
end

# on_track gets only the metadata in the frame of the track mark, else an empty
# list (liquidsoap source.ml, execute_on_track). After a jingle the metadata of
# the next track can come in a later frame. Then the first metadata of the track
# gives a new seq, with the start time of the track mark.
# The two handlers are synchronous, so that they operate in the frame order.
# They only set references. The file write is in musicindex_write.
def musicindex_on_track(m) =
  musicindex_pending := list.length(m) == 0
  musicindex_set(time(), m)
end
source.methods(radio).on_track(synchronous=true, musicindex_on_track)

def musicindex_on_metadata(m) =
  if musicindex_pending() and list.length(m) > 0 then
    musicindex_pending := false
    let (_, started, _, _, _) = musicindex_state()
    musicindex_set(started, m)
  end
end
source.methods(radio).on_metadata(synchronous=true, musicindex_on_metadata)

# The picture file of ADR 0041 §The Slot Text Writes The Picture File. The slot text selects the front cover of the
# file, else the first picture, and writes the bytes with no change. The
# producer reads the picture file. It never reads the media file.
musicindex_picture_max = 67108864
# The seq of the last picture read, and the name of its picture file.
musicindex_picture = ref((-1, ""))
# The picture files to remove after the next write of state.json.
musicindex_picture_old = ref([])

# A picture file of an earlier start of Liquidsoap is not current.
try
  list.iter(
    fun (f) -> file.remove(f),
    file.ls(absolute=true, pattern="picture-*", musicindex_dir)
  )
catch err do
  log(level=2, label="musicindex", "The removal of old picture files failed: #{err}")
end

# Gives the picture blocks of a FLAC metadata_block_picture value.
# file.metadata.flac_native joins two blocks with ", " (ADR 0041 §The Slot Text Writes The Picture File).
# Each block holds its lengths, so the function reads them to find the next block.
def musicindex_flac_blocks(s) =
  total = string.bytes.length(s)
  def int_at(i) =
    string.binary.to_int(little_endian=false, string.sub(encoding="ascii", s, start=i, length=4))
  end
  blocks = ref([])
  i = ref(0)
  while i() + 32 <= total do
    mime_len = int_at(i() + 4)
    desc_len = int_at(i() + 8 + mime_len)
    data_len = int_at(i() + 28 + mime_len + desc_len)
    len = 32 + mime_len + desc_len + data_len
    if i() + len > total then
      # A block that is not complete stops the read.
      i := total
    else
      blocks := list.append(blocks(), [string.sub(encoding="ascii", s, start=i(), length=len)])
      i := i() + len + 2
    end
  end
  blocks()
end

# Gives the pictures of a file as a list of (picture type, data), in file order.
# MP3 gives one APIC frame: Liquidsoap keeps one value for each metadata key
# (ADR 0041 §The Slot Text Writes The Picture File). FLAC gives its picture blocks. Each other format gives [].
def musicindex_pictures(fname) =
  pictures = ref([])
  def add(picture_type, data) =
    pictures := list.append(pictures(), [(picture_type, data)])
  end
  list.iter(
    fun (kv) ->
      if fst(kv) == "apic" then
        p = string.apic.parse(snd(kv))
        add(p.picture_type, (p : string))
      end,
    file.metadata.id3v2(fname)
  )
  list.iter(
    fun (kv) ->
      if fst(kv) == "metadata_block_picture" then
        list.iter(
          fun (block) -> begin
            p = file.metadata.flac.cover.decode(block)
            if null.defined(p) then
              p = null.get(p)
              add(p.picture_type, (p : string))
            end
          end,
          musicindex_flac_blocks(snd(kv))
        )
      end,
    file.metadata.flac_native(fname)
  )
  pictures()
end

# Writes the picture file of the track with this seq. Gives the name of the
# picture file, or "" when the track has no picture.
def musicindex_write_picture(seq, fname) =
  if fname == "" then
    ""
  else
    pictures = musicindex_pictures(fname)
    # The front cover has the picture type 3.
    front = ref("")
    list.iter(fun (p) -> if front() == "" and fst(p) == 3 then front := snd(p) end, pictures)
    data =
      if front() != "" then
        front()
      elsif list.length(pictures) > 0 then
        snd(list.hd(pictures))
      else
        ""
      end
    if data == "" or string.bytes.length(data) > musicindex_picture_max then
      ""
    else
      name = "picture-#{seq}"
      file.write(atomic=true, temp_dir=musicindex_dir, data=data, "#{musicindex_dir}/#{name}")
      name
    end
  end
end

# Reads the picture one time for each seq, in this thread and not in the
# streaming thread. A jingle keeps the picture of the last track. A track mark
# with no metadata also keeps it, because a jingle can give its metadata later.
def musicindex_update_picture(seq, m, jingle) =
  let (picture_seq, picture_name) = musicindex_picture()
  if not jingle and list.length(m) > 0 and seq != picture_seq then
    name =
      try
        musicindex_write_picture(seq, m["filename"])
      catch err do
        log(level=2, label="musicindex", "The read of the picture failed: #{err}")
        ""
      end
    musicindex_picture := (seq, name)
    if picture_name != "" and picture_name != name then
      musicindex_picture_old := list.append(musicindex_picture_old(), [picture_name])
    end
  end
end

def musicindex_write() =
  let (picture_seq, _, picture_m, _, picture_jingle) = musicindex_state()
  musicindex_update_picture(picture_seq, picture_m, picture_jingle)
  let (seq, started, m, _, jingle) = musicindex_state()
  # A new track during the picture read: the next write reads its picture.
  # This write must not give the metadata of the last track with a new elapsed.
  if seq == picture_seq then
    let (_, picture_name) = musicindex_picture()
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
      pub_date = m["date"],
      wpay = m["wpay"],
      track_guid = m["podcast item guid"],
      feed_guid = m["podcast feed guid"],
      value_routes = m["podcast value routes"],
      image = m["podcast image"],
      description = m["podcast description"],
      contributors = m["podcast contributors"],
      chapters = m["podcast chapters"],
      picture = picture_name
    })
    try
      file.write(atomic=true, temp_dir=musicindex_dir, data=data, "#{musicindex_dir}/state.json")
      # The state now names the new picture file, so the old file can go.
      old = musicindex_picture_old()
      musicindex_picture_old := []
      list.iter(
        fun (name) ->
          try
            file.remove("#{musicindex_dir}/#{name}")
          catch err do
            log(level=2, label="musicindex", "The removal of #{name} failed: #{err}")
          end,
        old
      )
    catch err do
      log(level=2, label="musicindex", "The write of state.json failed: #{err}")
    end
  end
end
thread.run(every=1., musicindex_write)

# The segment frame of ADR 0038 §The Segment Frame. The insert is the last operator of the slot text,
# so the handlers above and the AzuraCast handlers never see an inserted group. It never makes a new track.
musicindex_inner = radio
radio = insert_metadata(radio)
musicindex_frame = ref((-1, [], false))

# BEGIN musicindex_boundaries
# The segment starts of ADR 0045 §The Segment Rule. The producer uses the same rule, and the cases in
# liquidsoap-now-playing/tests/segment-boundaries.json check the two (scripts/check-slot-boundaries.sh).
# It returns (true when one or more entries have valid times, the segment starts above 0).
def musicindex_boundaries(text) =
  entries =
    try
      let json.parse (e : [{start_ms: int?, end_ms: int?}?]) = text
      e
    catch _ do
      []
    end
  # An entry that is not an object, or a time that is not an integer from 0 to 2^62-1, gives null.
  def span(e) =
    x = null.get(default={start_ms=null(), end_ms=null()}, e)
    s = null.get(default=-1, x.start_ms)
    t = null.get(default=-1, x.end_ms)
    if s >= 0 and t > s then [(s, t)] else [] end
  end
  # Of two entries with the same start, the first in the list wins.
  def keep(kept, x) =
    if list.exists(fun (y) -> fst(y) == fst(x), kept) then kept else list.append(kept, [x]) end
  end
  spans = list.sort(fun (a, b) -> fst(a) - fst(b), list.fold(keep, [], list.flatten(list.map(span, entries))))
  # A chapter ends at the next start. A gap starts at an end before the next start, and after the last chapter.
  def gap(i, x) =
    next = fst(list.nth(default=(-1, -1), spans, i + 1))
    if next < 0 or snd(x) < next then [snd(x)] else [] end
  end
  ends = list.flatten(list.mapi(gap, spans))
  (list.length(spans) > 0, list.filter(fun (b) -> b > 0, list.append(list.map(fst, spans), ends)))
end
# END musicindex_boundaries

# Write the frame at each segment start of ADR 0045 §The Slot Text (ADR 0038 §The Segment Frame).
def musicindex_on_frame() =
  let (seq, started, m, _, jingle) = musicindex_state()
  let (frame_seq, _, _) = musicindex_frame()
  if seq != frame_seq and list.length(m) > 0 then
    let (chapters, bounds) = musicindex_boundaries(m["podcast chapters"])
    musicindex_frame := (seq, if jingle then [0] else list.append([0], bounds) end, chapters)
  end
  let (frame_seq, left, chapters) = musicindex_frame()
  if frame_seq == seq and list.length(left) > 0 then
    pos = int_of_float((source.methods(musicindex_inner).elapsed() + float_of_string(default=0., m["liq_cue_in"])) * 1000.)
    passed = list.filter(fun (b) -> b <= pos, left)
    if list.length(passed) > 0 then
      musicindex_frame := (seq, list.filter(fun (b) -> b > pos, left), chapters)
      b = list.fold(fun (a, x) -> if x > a then x else a end, 0, passed)
      # The producer reads track_started from the text in state.json, with about 12 significant digits.
      # The slot text uses the same text, so that the two make the same playId (station log step 51).
      ms = int_of_float(float_of_string(json.stringify(started)) * 1000.)
      id = if chapters then "ls-#{ms}-#{seq}-#{b}" else "ls-#{ms}-#{seq}" end
      play_id = if jingle then null else id end
      frame = json.stringify(compact=true, {schema = "musicindex.hls/2", playId = play_id})
      keep = list.filter(fun (kv) -> snd(kv) != "", [("title", m["title"]), ("artist", m["artist"]), ("album", m["album"]), ("song", m["song"])])
      radio.insert_metadata(new_track=false, list.append(keep, [("TXXX", "musicindex\000#{frame}")]))
    end
  end
end
radio.on_frame(before=true, synchronous=true, musicindex_on_frame)
