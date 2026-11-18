//! Gives the git revision and the build time to the compiler of the publisher
//! (musicindex ADR 0083 R-VERSION-8). A release build can set
//! `MUSICINDEX_LIVE_PUBLISHER_GIT_REVISION` and
//! `MUSICINDEX_LIVE_PUBLISHER_BUILT_AT`.

fn main() {
    musicindex_build_info::build::emit("MUSICINDEX_LIVE_PUBLISHER");
}
