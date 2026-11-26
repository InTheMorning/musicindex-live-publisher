# Display Keys Task 001: The Crate `display-keys` And The Publisher Keys

Status: Ready - 2026-11-26. ADR 0042 and ADR 0043 govern this task.

Each criterion is mechanical. No visual check applies to this task.

## Goal

Make the workspace crate `display-keys` with the day rule of ADR 0042 and the funding rules of ADR 0043. Then the
publisher reads `pub_date` and `funding` from `display.json` and sends `pubDate` and `funding` to the relay.

## Context

- ADR 0042 §The Producer and §The Publisher: the day rule and the key `pubDate`.
- ADR 0043 §The Key and §The Publisher: the shape of `funding`, the exact rules of the relay, the label cut, the limit
  of 3 entries and the body rule of 8,192 bytes.
- ADR 0041 §One Display Writer And One Picture Component: the publisher does not depend on `now-playing-drop`.
  Thus the rules go into a new crate that the two sides use.
- The publisher reads `display.json` in `src/display.rs` (`read_display_state` at line 310, `parse_track` at line
  364). It makes the wire body in `DisplayState::body_with_pairing` (line 203). `src/relay.rs`
  `RelayClient::publish_display` (line 376) sends the body. Tests are in `tests/display.rs`.

## The Crate `display-keys`

Add the folder `display-keys` as a workspace member and as a member of `default-members` in the root `Cargo.toml`.
It has no dependency other than `serde` (with `derive`). License: `AGPL-3.0-only`, the same as the root package.
Edition 2024.

Public items:

- `pub fn pub_day(value: &str) -> Option<&str>`: the first 10 characters of `value` when they are `YYYY-MM-DD`,
  ASCII digits and hyphens, and a day that exists in the Gregorian calendar (leap years included). Else `None`. A
  value of fewer than 10 characters gives `None`. Do not add `chrono` or `time`.
- `pub struct FundingLink { pub url: String, pub label: Option<String> }`, with `Serialize` and `Deserialize`.
  Serialization omits `label` when it is `None`.
- `pub const MAX_FUNDING_LINKS: usize = 3`, `pub const MAX_FUNDING_URL_CHARS: usize = 512`,
  `pub const MAX_FUNDING_LABEL_CHARS: usize = 128`.
- `pub fn funding_url_ok(url: &str) -> bool`: the rules of ADR 0043 §The Key for `url`. 1 to 512 characters
  (`chars().count()`). The scheme before `://` is `http` or `https` with no case compare. After `://`, one character
  or more comes before the first `/`, `?` or `#`. No character for which `char::is_whitespace` or `char::is_control`
  is true.
- `pub fn funding_label(raw: &str) -> Option<String>`: `str::trim`, then the first 128 characters, then `str::trim`
  again (end only is sufficient). An empty result gives `None`.
- `pub fn funding_links<I>(entries: I) -> Vec<FundingLink>` where each entry gives a URL and an optional raw label
  (select a simple input type, for example `IntoIterator<Item = (&str, Option<&str>)>`). It keeps the sequence of the
  entries. It removes each entry where `funding_url_ok` gives `false`. It removes an entry when its URL is equal, character
  by character, to the URL of an earlier entry that stays. It applies `funding_label` to each label. It keeps at most
  3 entries.

Unit tests in the crate cover each rule. Use these values as a minimum:

- Days: `2026-11-12` and `2024-02-29` are correct. `2026-02-29`, `2026-02-30`, `2026-13-01`, `2026-00-10`,
  `2026-11-31`, `12 Nov 2026` and an empty string give `None`. `2026-11-12T08:00:00` gives `2026-11-12`.
- URLs: `HTTPS://example.com` is correct. `https:///support`, `ftp://x`, a URL with a space and a URL with a tab are
  refused. A URL of 512 characters is correct, and a URL of 513 characters is refused.
- Labels: a label of 200 characters gives 128 characters. A label with a non-breaking space or an ideographic space at
  the end loses that space.
- Lists: a second equal URL is removed. Five correct entries give three.

## The Publisher

- Root `Cargo.toml`: add `display-keys = { path = "display-keys" }`. Do not add `now-playing-drop`.
- `DisplayTrack` in `src/display.rs` gets `pub_date: Option<String>` and `funding: Vec<FundingLink>`.
- `parse_track`:
  - `pub_date`: a string that is not empty gives `Some`. `null`, a missing key or a different type gives `None`.
    The publisher does not apply the day rule (ADR 0042 §The Publisher).
  - `funding`: when it is an array, read each element that is an object with a string `url` and an optional string
    `label`. Ignore each other element. Pass the result through `display_keys::funding_links`. `null`, a missing key or
    a different type gives an empty list. An unknown key does not make the track not valid.
- `body_with_pairing`: add `pubDate` when `pub_date` is `Some`. Add `funding` when the list is not empty, as a JSON
  array of objects with `url` and, when present, `label`. Never send `null` or `[]`.
- **The body rule** (ADR 0043 §The Publisher): when the serialized JSON body is more than 8,192 bytes and the track has
  `funding`, the publisher sends the body without `funding`. It writes one `warn` line for each `play_id` (a track with
  `play_id` `None` writes one line for each change of state). Put this rule where the body is made, so that each send
  path uses it. A body above 8,192 bytes with no `funding` goes as before.
- The change of the display state, as the publisher compares it, includes `pub_date` and `funding`.

## Base components

| Component | Status | Location | Reason |
| --- | --- | --- | --- |
| Publisher display reader | extend | /home/citizen/build/musicindex-live-publisher/src/display.rs | `parse_track` reads the optional keys of version 3. |
| Publisher wire body | extend | /home/citizen/build/musicindex-live-publisher/src/display.rs | `body_with_pairing` adds `album` and `value` in the same way. |
| Relay display send | reuse | /home/citizen/build/musicindex-live-publisher/src/relay.rs | `publish_display` sends the body with no change. |
| URL check for artwork | reuse | /home/citizen/build/musicindex-live-publisher/now-playing-drop/src/display.rs | `image_url` stays for the artwork. Its rules differ (2,048 characters, no host rule), so it does not serve the funding rules. |
| Display key rules | new | /home/citizen/build/musicindex-live-publisher/display-keys | Searched now-playing-drop/src/display.rs (`image_url`), src/display.rs, musicindex-lib fetch/src/resolve.rs (`good_image_url`) and fetch/src/reader.rs (`feed_host`). None has the host, white space and control rules, and the publisher cannot use now-playing-drop (ADR 0041). |

## Files To Inspect

- `docs/adr/0042-an-episode-sends-its-publication-date.md`
- `docs/adr/0043-the-display-state-carries-the-funding-links.md`
- `src/display.rs`, `src/relay.rs`, `tests/display.rs`
- `Cargo.toml`

## Files Likely To Change

- `Cargo.toml`, `Cargo.lock` (the new path crate only)
- `display-keys/Cargo.toml`, `display-keys/src/lib.rs` (new)
- `src/display.rs`, `src/relay.rs` if the body rule needs it
- `tests/display.rs`

## Do Not Touch

- `now-playing-drop`, `liquidsoap-now-playing`, `mixxx-now-playing`, `station-import`, `liquidsoap/*.liq`.
  Tasks 002 and 003 change the producers.
- `docs/adr/`, `AGENTS.md`, `docs/README.md`.
- The live value payload (`src/livevalue.rs`).

## Acceptance Criteria

Mechanical. Each item is a test:

- The crate tests of §The Crate `display-keys` pass.
- `tests/display.rs`: a `display.json` track with `pub_date` `"2026-11-12"` gives `pubDate` `"2026-11-12"` on the
  wire. `null`, `""` and a missing key give no key `pubDate`.
- `tests/display.rs`: a track with two valid links (one with a label, one without) gives `funding` with two objects.
  The object with no label has no key `label`.
- `tests/display.rs`: `null`, `[]`, a list with only invalid entries and a missing key give no key `funding`. A list of
  five valid entries gives three. An entry with `https:///support` is not sent, and the other entries are sent.
- `tests/display.rs`: a track whose body with `funding` is more than 8,192 bytes (use a long `title`) is sent without
  `funding`, and with each other key.
- The existing display tests pass with no change of their expected bodies.
- The full gate passes. `Cargo.lock` changes only for the new path crate.

## Test Commands

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Escalation Triggers

- A rule of ADR 0042 or ADR 0043 cannot be done as written. Stop and report.
- The body rule needs a change in the retry or pairing logic of `src/relay.rs`. Stop and report.

A delegate does not commit. The publisher session reads the diff, runs the gate and makes the commit. Write prose and
code comments in Simplified Technical English (`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).
