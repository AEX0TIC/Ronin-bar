# Ronin Bar

A smart dock for the top of the Windows 10/11 screen: your pinned apps and open windows with their real icons, working system controls, anime wallpapers and five themes. One small program written in Rust: no GlazeWM, Zebar, Windhawk, Oh My Posh or any other app to install.

## What it does

The dock sits at the top of the screen. Windows keeps that space free, so maximised windows stay below it. It steps aside while a fullscreen game or video is in front.

**Left: the dock**

| | |
|---|---|
| `ronin` | Opens the control panel |
| pinned apps | Click to open. Right-click to manage them. Pick them from your Start menu in **Apps** (up to 16) |
| open windows | Click to bring a window to the front, click the one in front to minimise it, middle-click to close it, right-click to pin its program. The line under an icon marks the window in front; dimmed icons are minimised |

**Right: controls**

| | |
|---|---|
| `cpu` `ram` | Processor and memory use, updated every second |
| `net` | Whether the PC is online |
| `bat` | Battery level (`+` while charging); hidden on PCs without a battery |
| `vol` | Volume: scroll to change, click to mute |
| `wall` | Next wallpaper; right-click for all wallpapers |
| date, clock | Day, date and time |
| `power` | Lock, sleep, restart, shut down (asks before sleep, restart and shut down) |

**Apps**: search everything in your Start menu, pin, unpin, reorder with ← →, or open an app straight from the list. Only shortcuts and programs can be pinned, and they are opened the way Explorer opens them.

**30 wallpapers**: 10 samurai, 10 anime cars and 10 anime scenes, downloaded from [Wallhaven](https://wallhaven.cc) on first start. Only the anime category and safe-for-work images are used. Previous / next / random, or change automatically every 15 min to 3 h.

**5 themes**: Sakura, Samurai, Neon Tokyo, Matcha (light) and Midnight. Each colours the bar and sets Windows' dark/light mode and accent colour.

## Install

Download `Ronin-Bar-<version>.exe` from [Releases](https://github.com/AEX0TIC/Ronin-bar/releases) and run it. It asks once, then installs itself for your Windows account only (no admin needed):

```text
%LOCALAPPDATA%\Programs\Ronin Bar\
├─ ronin-bar.exe
└─ data\   settings, the wallpaper list, wallpapers\ and thumbs\
```

It also adds a Start-menu shortcut, an entry in **Settings → Apps**, and (if you leave it on) starts with Windows.

## Uninstall

**Settings → Apps → Ronin Bar → Uninstall**, or the control panel's **Settings → Uninstall**. It puts back the wallpaper, dark/light mode and accent colour you had before, removes the shortcut, the Settings → Apps entry and the startup entry, and deletes the folder.

## Safety

- Only Windows' own functions are used; no scripts and no helper programs. The core logic has no `unsafe` code.
- Downloads use HTTPS only, from `wallhaven.cc`, with every URL and id checked; each file must really be a JPEG or PNG under 25 MB.
- Nothing runs as administrator, and nothing outside your account is changed.

The wallpapers belong to their artists. Ronin Bar downloads them to your PC for personal use and does not redistribute them; hover a wallpaper to see its page on wallhaven.cc.

## Build

Requires Rust 1.87+ on Windows.

```powershell
cargo test --workspace
cargo build --release -p ronin-bar   # target\release\ronin-bar.exe
```

`crates/core` (themes, settings, dock pins, wallpaper catalogue and checks) is tested on any OS; `crates/app` is the Windows program.
