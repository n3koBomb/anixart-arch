# Changelog

## 0.4.1 - 2026-08-08

- Replace separate release-details and voiceover/source/episode windows with one `GtkStack` navigation flow: Home → Release details → Playback selection.
- Keep the actual video renderer as a dedicated playback window; only application navigation is consolidated.
- Fix Discover/Interesting card identity. Beta 21's `Interesting` model is a navigation/banner wrapper (`id`, `title`, `image`, `action`, ...), not a release. 0.4.1 resolves the actual release target from the wrapper `action` and also accepts explicit `release_id` / `releaseId` or nested `release` schemas. This prevents wrapper IDs such as `8013030` from being sent to `/release/{id}` and `/episode/{id}`.
- Add regression tests for wrapped Discover results and explicit release IDs.
- Constrain poster-card layout so FlowBox cells no longer stretch artwork across half the desktop.
- Use a bounded `GtkPaned` release layout with fixed artwork on the left and scrollable metadata on the right.
- Prefer `image_original`/release artwork before wrapper/banner images.
- Surface voiceover/source/episode errors directly on the playback-selection page instead of only in the hidden Home status line.
- Automatically select the only available player source to reduce one unnecessary click.

## 0.4.0 - 2026-08-08

- Introduce the phone-inspired desktop shell with search, filter/category row, poster grid, bottom navigation, release details and voiceover artwork.
- Move developer actions out of the main header into the Settings popover.
- Keep Filter and non-Home navigation destinations structural while their data sources are implemented incrementally.

## 0.3.4 - 2026-08-08

- Default `GSK_RENDERER=gl` on Wayland when the user has not explicitly selected a renderer.
- Preserve explicit `GSK_RENDERER` overrides, including `GSK_RENDERER=vulkan anixart` for future A/B testing.
- Keep `GraphicsOffload` enabled as an opportunistic direct-offload hint while using GL as the efficient fractional-scaling fallback.
- Document the observed 175% fractional-scaling failure path: Vulkan rejects the WebKit XR24 DMABUF modifier and falls back through costly GL downloads/conversions.
- Extend `anixart --media-check` with the renderer policy and explicit GL/Vulkan test commands.
- Replace stale hard-coded UI version text with `CARGO_PKG_VERSION`.

## 0.3.3 - 2026-08-08

- Explicitly force WebKitGTK hardware acceleration for provider WebViews.
- Wrap provider WebViews in GTK `GraphicsOffload` for Wayland/DMABUF presentation.
- Enable black-background offload-friendly letterboxing.
- Require `gst-plugin-va` in Arch packaging for VA-API GStreamer decoders.
- Add Intel/Mesa VA driver and `vainfo` optional dependencies.
- Expand `anixart --media-check` with session, renderer, VA-API, and hardware decoder diagnostics.
- Add opt-in `--webkit-no-dmabuf` and `--webkit-no-compositing` A/B diagnostic switches.
- Keep the existing WebKit provider compatibility and ring/LTO packaging fixes.


## 0.3.2 - 2026-08-08

- Fix compilation with gtk4 0.11 + webkit6 0.6 by explicitly selecting `WebViewExt::settings(&webview)`.
- Resolves Rust E0034/E0282 caused by the name collision between WebKit `WebViewExt::settings()` and GTK `WidgetExt::settings()`.
- Keeps all 0.3.1 WebKit media/MSE/provider compatibility settings unchanged.
- Synchronize the bundled Arch packaging templates with the actual source version and media dependencies.

## 0.3.1

- Enable WebKitGTK MediaSource, Media Capabilities, HTML5 media, WebAudio, fullscreen and inline/autoplay playback for provider players.
- Load the embedded wrapper with a provider HTTP(S) base URI instead of `about:blank`, matching Android `loadDataWithBaseURL` semantics more closely.
- Use an Android-WebView-compatible user agent for provider compatibility.
- Keep the iframe wrapper intentionally close to the APK's bundled `assets/player_default.html`.
- Enable WebKit console logging for beta diagnostics.
- Add `anixart --media-check` for GStreamer/mpv capability diagnostics.
- Keep direct streams routed to mpv.


## 0.3.0 - 2026-08-08

- Added native WebKitGTK 6.0 player window for iframe/provider episode URLs.
- Reproduced the reference app's key playback behavior: provider URL is embedded in an iframe instead of opened as a top-level web page.
- Added automatic playback routing: recognizable direct media streams use mpv; provider pages use WebKitGTK.
- Changed the episode primary action dynamically between **Play in mpv** and **Play in app**.
- Changed episode double-click to use the same smart player router.
- Renamed top-level browser action to **Open URL (debug)** and explicitly warn that iframe-only providers can intentionally return a 404 there.
- Added WebKitGTK/GStreamer runtime dependencies to Arch packaging.
- Updated gtk-rs/libadwaita-rs bindings to the current compatible 0.11/0.9 generation required by webkit6 0.6.x.
- Retained the CachyOS/Arch `ring` LTO compatibility fix.

## 0.2.0 - 2026-08-08

- Replaced first-level media-URL scanning with the real three-stage episode API flow.
- Added typed voiceover/subtitle parsing from `types`.
- Added player/source selection and concrete episode parsing.
- Added Watch and Raw API tabs.
- Added stale-response guards.

## 0.1.0 - 2026-08-07

- Initial clean native Linux proof of concept.
