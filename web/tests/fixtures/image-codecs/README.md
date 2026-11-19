# Raster preview fixtures

These project-owned fixtures contain a 160 × 100 solid magenta image
(RGB 210, 40, 170). They were encoded with image-rs 0.25.10 using
`DynamicImage::write_to`, except for DDS's explicit DXT1 header and blocks.
The ICO fixture uses an RGBA PNG payload; HDR uses floating-point RGB.

The GIF has two full frames, magenta then green, with an infinite repeat and
100 ms delays. VibePad must display only its first frame. DDS uses DXT1 blocks
with RGB565 color `(26 << 11) | (10 << 5) | 21`, index zero for every pixel.

These fixtures exercise the shared Rust decoder and actual GPU pixels for
formats that browser-native image elements cannot consistently decode.
