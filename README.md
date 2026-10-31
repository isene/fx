# fx

<img src="img/fx.svg" align="right" width="150">

**The views and effects of the desktop, each with a tick box.**

![Rust](https://img.shields.io/badge/language-Rust-orange) ![Unlicense](https://img.shields.io/badge/license-Unlicense-green) ![Platform](https://img.shields.io/badge/platform-Linux-blue) ![Stay Amazing](https://img.shields.io/badge/Stay-Amazing-important)

One key opens a short list: the views, such as LCARS, and the effects, such as the transporter beam-in. Space switches the one under the cursor, and it takes effect at once. No keys to remember for each. Part of the [Fe₂O₃ Rust terminal suite](https://github.com/isene/fe2o3).

## Using it

Bind it to a key in your window manager:

```
bind Mod4+Shift+f exec fx
```

From a key binding it opens its own [glass](https://github.com/isene/glass) window. The same key closes it again. It wears [launch](https://github.com/isene/launch)'s colours from `~/.launchrc`, so a view that dresses launch dresses fx too.

| Key | Does |
|---|---|
| arrows, `Tab` | Move |
| `Space`, `Enter` | Switch the one under the cursor |
| `Esc` | Close |

## What it lists

- **Views**, from the [views](https://github.com/isene/fe2o3#views) tool. A view dresses strip, herald, launch and tile at once, so only one is on at a time. A new file in `views/` shows up here by itself.
- **Beam-in**, when the desktop runs on [frame](https://github.com/isene/frame). A new window arrives in sparkles, like a transporter. fx sets `beam = 600` (or `0`) in `~/.framerc` and tells frame to read it again.
- **Living wallpaper**, when frame runs and [astro](https://github.com/isene/astro) is installed. The sky over you now, the Moon in its phase and the Yr forecast, drawn again every hour. fx points frame's `background` at astro's picture and starts `astro --wall`; off points it back at `~/.framebg`.

## Speed

fx asks the views tool once at start, about 10 ms. Between keys it does nothing at all.

## Install

```bash
cargo install --path .
```

## License

Public domain. Do what you like with it.
