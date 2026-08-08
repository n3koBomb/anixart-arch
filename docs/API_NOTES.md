# API compatibility notes

This project is a clean native reimplementation. The Android APK is used only as a behavioral/protocol reference. No decompiled Java/Kotlin code, proprietary Android resources, signing keys, or APK binaries are distributed here.

Reference APK used during development:

- package: `com.swiftsoft.anixartd`
- version: `9.0 BETA 21`
- versionCode: `26080522`
- SHA-256: `bd08b1033cea29f375499775076df6a1503ede5e620bb225543a3ab6877c15b5`
- observed API host: `https://api-s.anixsekai.com/`

## Episode hierarchy

```text
GET episode/{releaseId}
    → types[]

GET episode/{releaseId}/{episodeDubberId}
    → sources[]

GET episode/{releaseId}/{episodeDubberId}/{sourceId}
    → episodes[]
```

The decoder tolerates alternate snake_case/camelCase fields because the 9.0 branch is beta software.

## Playback interpretation added in 0.3

Episode objects expose a URL plus an `iframe` flag. They are not all raw video URLs.

```text
iframe=true or provider-style URL
    → native WebKitGTK WebView
    → local HTML wrapper
    → <iframe src="provider-url">

direct .m3u8/.mpd/.mp4/... URL
    → mpv
```

The iframe wrapper mirrors the important behavior observed in the reference APK's `assets/player_default.html`: the provider URL is embedded rather than opened as a top-level browser document.

The client deliberately keeps **Open URL (debug)** as a separate diagnostic action. A provider can reject top-level navigation while still supporting iframe playback, so a 404 from that debug action is not treated as proof that the episode link is invalid.

## Other observed/documented routes

- `config/toggles`
- `config/urls`
- `discover/interesting`
- `discover/recommendations/{page}`
- `search/releases/{page}`
- `release/{id}`
- `release/random`
- `episode/watch/{id}/{sourceId}/{episodePosition}`
- `favorite/add/{id}`
- `favorite/delete/{id}`
- `profile/info`

Because this API is not advertised as a stable public developer API, request/response decoding stays isolated behind `src/api.rs` and `src/model.rs`.

## Deliberate boundaries

The client does not implement DRM bypass, access-control circumvention, source-specific stream deobfuscation, or redistribution of the Anixart APK/official artwork.
