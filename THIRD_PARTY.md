Fira Mono Medium (`assets/fonts/FiraMono-Medium.ttf`) is the complete font distributed with Bevy Feathers 0.20.1, used for in-game text.
Digitized data copyright (c) 2012–2015, The Mozilla Foundation and Telefonica S.A.
Licensed under the SIL Open Font License 1.1; see `assets/fonts/FiraMono-LICENSE`.

JuliaMono Medium 0.63 (`assets/fonts/JuliaMono-Medium.ttf`) supplies mathematical
Unicode glyphs missing from Fira Mono, including Lean identifiers and operators.
The unmodified font comes from `cormullion/juliamono` revision
`7b5b7af79de3a14ce0aa19745ef069d42c20f2cb` and is licensed under SIL OFL 1.1.
See `assets/fonts/JuliaMono-LICENSE`.

Noto Emoji (`assets/fonts/NotoEmoji.ttf`) supplies monochrome emoji glyphs for
the desktop and browser interfaces and code walls. The unmodified font and its SIL Open Font
License 1.1 come from [Google Fonts revision 8b0a1d0](https://github.com/google/fonts/tree/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/ofl/notoemoji).
See `assets/fonts/NotoEmoji-LICENSE`.

The browser syntax worker uses [Starry Night 3.11.0](https://github.com/wooorm/starry-night)
with separately loaded TextMate grammars and vscode-oniguruma 2.0.1. The web build
publishes their licenses and complete grammar notices under `syntax-licenses/`.
Unmodified grammar modules are available under `syntax-source/`; the complete
[pinned package source](https://registry.npmjs.org/@wooorm/starry-night/-/starry-night-3.11.0.tgz)
includes their original repository links and individual license terms.

Filename and interpreter detection metadata in `web/syntax-language-map.json`
comes from GitHub Linguist revision `14ac04abe3e3722e42c52489b377d9d4118fc8b6`.
The local projection keeps only names, scopes, extensions, filenames and interpreters;
see `licenses/linguist-MIT.txt`.

Caveat (`web/assets/caveat-latin-wght.woff2`) is licensed under the SIL Open Font License 1.1.
Copyright 2014 The Caveat Project Authors. See `web/assets/OFL-Caveat.txt`.

## GPU Markdown dependencies

The desktop and browser GPU Markdown renderer uses the following unmodified MPL-2.0 packages.
Their source archives are available at the links below, under the
[Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/).
The application is a larger work distributed under its own license.

- [cssparser 0.37.0 source](https://crates.io/api/v1/crates/cssparser/0.37.0/download)
- [cssparser-color 0.5.0 source](https://crates.io/api/v1/crates/cssparser-color/0.5.0/download)
- [cssparser-macros 0.7.1 source](https://crates.io/api/v1/crates/cssparser-macros/0.7.1/download)
- [dtoa-short 0.3.5 source](https://crates.io/api/v1/crates/dtoa-short/0.3.5/download)
- [lightningcss 1.0.0-alpha.72 source](https://crates.io/api/v1/crates/lightningcss/1.0.0-alpha.72/download)
- [lightningcss-derive 1.0.0-alpha.43 source](https://crates.io/api/v1/crates/lightningcss-derive/1.0.0-alpha.43/download)
- [parcel_selectors 0.28.3 source](https://crates.io/api/v1/crates/parcel_selectors/0.28.3/download)
- [static-self 0.1.2 source](https://crates.io/api/v1/crates/static-self/0.1.2/download)
- [static-self-derive 0.1.1 source](https://crates.io/api/v1/crates/static-self-derive/0.1.1/download)

Markdown parsing uses pulldown-cmark 0.13.6 under the MIT license.
Copyright 2015 Google Inc. See `licenses/pulldown-cmark-MIT.txt`.

GPU document layout uses bevy_markup 0.8.0 under MIT or Apache-2.0.
Copyright 2026 Nikita Chashchinskii. See `licenses/bevy-markup-MIT.txt`
and `licenses/bevy-markup-APACHE.txt`.

## Project artwork

The repository cover (`web/assets/start-screen.webp`) and facade texture
(`assets/city/facade-print.png`) are original artwork created for this project.
They are distributed under the project's MIT or Apache-2.0 license.

## Native terminal

VibePad uses Alacritty's terminal state machine and portable-pty directly.
No Zed terminal wrapper code is included.

- alacritty_terminal 0.26.0: Apache-2.0. See `licenses/alacritty-terminal-APACHE.txt`.
- portable-pty 0.9.0: MIT. See `licenses/portable-pty-MIT.txt`.

Touch flight controls adapt Feed the Poor's `web/src/touch.ts` at revision
`beabe03f5aef648717fa175693cb9a76c953d9d5`. The local adaptation adds altitude,
boost, water/paint actions, and VibePad navigation to the existing movement stick
and drag-to-look controls.

## Source editing

VibePad uses Sean Barrett's unchanged `stb_textedit.h` 1.16 from `nothings/stb`
revision `9600522de53973336fa033a289858a4354cbb04c`, under its MIT license option.
See `licenses/stb-textedit-MIT.txt` and `crates/explorer/vendor/stb/SOURCE.md`.
The local C adapter supplies bounded Unicode storage, wrapped row geometry and
key bindings; the shared Bevy widget reuses the city's syntax colors and fonts.

## Native agent audio

Voice uses webrtc-rs 0.14.0 under MIT or Apache-2.0, Tokio under MIT, and CPAL
under Apache-2.0. Packaged native builds include Cargo source notices alongside
the application licenses. The session and transcript adaptation follows
`bobishh/ecky` revision `c48e376f676663272ee3a06622cd81bacb3f24d7`;
the local transport uses native WebRTC and audio devices instead of Tauri's webview.

Native WebRTC audio uses opus 0.4.0 (MIT OR Apache-2.0) and opusic-sys 0.7.7
(BSD-3-Clause), with the unmodified bundled libopus 1.6.1 statically linked.
The bundled source comes from the published
[opusic-sys 0.7.7 archive](https://crates.io/api/v1/crates/opusic-sys/0.7.7/download).
See `licenses/libopus-BSD-3-Clause.txt` for libopus copyright, redistribution
conditions and patent-license links. Cargo source notices cover the Rust bindings
and the CMake build dependency (MIT OR Apache-2.0). The local adapter uses 48 kHz
mono audio, 20 ms Opus frames, a bounded RTP reorder window and packet-loss recovery.

The native WebRTC adapter uses a local webrtc-ice 0.14.0 adaptation for
periodic consent checks on the selected connection. Source revision, local
changes and both licenses are in `crates/explorer/vendor/webrtc-ice`.



## Image preview

Image preview uses image-rs 0.25.10 (MIT OR Apache-2.0). Additional GIF, TIFF and
QOI decoding uses color_quant 1.1.0, fax 0.2.7, gif 0.14.2, qoi 0.4.1,
tiff 0.11.3 and weezl 0.1.12, under MIT or MIT OR Apache-2.0 as declared by
each package. Distribution bundles include the complete source notices.
