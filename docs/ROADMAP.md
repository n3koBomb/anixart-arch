# Roadmap

## 0.1 — native transport/UI proof of concept — done

- GTK4/libadwaita application shell
- Wayland/X11 through GTK/GDK
- API configuration/search/discover
- external mpv proof of concept
- Arch packaging metadata

## 0.2 — episode navigation — done

- voiceover/subtitle model
- source/player model
- episode model
- release → voiceover → source → episode UI
- raw API diagnostics
- stale async-response protection

## 0.3 — correct provider playback routing — field testing

- identify direct media vs provider/iframe URLs
- direct streams → mpv
- iframe/provider pages → native WebKitGTK 6.0 player
- local iframe wrapper matching the reference app's playback model
- raw top-level URL action retained only for diagnostics
- GStreamer codec packages in Arch dependencies

## 0.4 — richer release UX

- poster loading + cache
- release overview instead of raw JSON
- paging/infinite search
- genres, status and episode counters
- robust mirror selection through config endpoints
- structured API errors

## 0.5 — account layer

- independently verified login contract
- Secret Service/libsecret token storage
- profile, favorites, history and lists

## 0.6 — richer player

- source-specific direct-stream resolvers where legitimately available
- playback position/history synchronization
- fullscreen/media keys
- subtitles/audio-track UX
- optional embedded libmpv/GStreamer surface for direct streams

## 1.0 — public AUR release

- stable Git repository and tags
- committed `Cargo.lock`
- reproducible source tarballs
- `.SRCINFO`
- clean-chroot build
- Plasma Wayland, GNOME Wayland and X11 integration tests
