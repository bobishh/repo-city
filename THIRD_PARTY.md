Fira Mono Medium (`assets/fonts/FiraMono-Medium.ttf`) is the complete font distributed with Bevy Feathers 0.20.1, used for in-game text.
Digitized data copyright (c) 2012–2015, The Mozilla Foundation and Telefonica S.A.
Licensed under the SIL Open Font License 1.1; see `assets/fonts/FiraMono-LICENSE`.

Noto Emoji (`assets/fonts/NotoEmoji.ttf`) supplies monochrome emoji glyphs for
the desktop and browser interfaces and code walls. The unmodified font and its SIL Open Font
License 1.1 come from [Google Fonts revision 8b0a1d0](https://github.com/google/fonts/tree/8b0a1d0f5983c89bc2b93f1b5fb55f9e252744b5/ofl/notoemoji).
See `assets/fonts/NotoEmoji-LICENSE`.

The browser syntax worker bundles Shiki and its TextMate grammars, plus the
JavaScript regex engine and its dependencies. The web build publishes their
license notices under `syntax-licenses/`.

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
