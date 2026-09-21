# Bundled social-card font

`Roboto-Regular-subset.ttf` and `Roboto-Bold-subset.ttf` are Latin subsets of
Roboto (Regular and Bold), used only to rasterize text into generated social
images (A5, ADR 0032). The font is compiled into the `signal` binary with
`include_bytes!`, so social-image rendering never reads a system font and
never depends on what happens to be installed on the build machine.

## Provenance

- Upstream: <https://github.com/googlefonts/roboto-2> (`src/hinted/`), at the
  `main` branch revision downloaded 2026-09-21.
- Original files: `Roboto-Regular.ttf` (515,100 bytes),
  `Roboto-Bold.ttf` (514,260 bytes).
- Licence: Apache License, Version 2.0 — see `LICENSE-Roboto.txt` (verbatim
  upstream `LICENSE`). Apache-2.0 matches this repository's licence; the
  font is redistributed unmodified except for subsetting.

## Subsetting

Produced with fontTools 4.65.0:

```sh
UNI='U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-203A,U+2044,U+2052,U+20AC,U+2122,U+2190-2193,U+2212,U+2215,U+2260,U+2264,U+2265,U+25CF,U+2605,U+2606'
pyftsubset Roboto-Regular.ttf --unicodes="$UNI" --output-file=Roboto-Regular-subset.ttf
pyftsubset Roboto-Bold.ttf   --unicodes="$UNI" --output-file=Roboto-Bold-subset.ttf
```

The subset covers Basic Latin, Latin-1 Supplement, typographic punctuation
(curly quotes, en/em dashes, ellipsis, bullets), and a few common symbols.
Glyphs outside the subset render as the font's missing-glyph box; A5 does not
autoselect fonts per script (a documented limitation, not silent truncation).

Layout features (kerning/ligatures) are dropped: the social-card layout
advances glyph by glyph with `ab_glyph`'s per-glyph `h_advance`, which never
consults kerning, so keeping GPOS would be dead weight.
