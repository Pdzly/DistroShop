# DistroShop

A small Linux desktop app for browsing distro images and flashing them to a USB stick.

This project is still early-stage, but the basic flow is there: it loads a distro catalog, lets you view details, downloads an ISO, and writes it to a target device.

## What it does

- Shows a list of Linux distros from a remote JSON catalog
- Caches the list locally in `~/.config/distroshop/distros.json`
- Lets you refresh the catalog manually
- Displays each distro's image, summary, and full description
- Downloads the selected ISO
- Flashes the ISO to a chosen block device
- Offers a `safe` and `fast` flash mode (streaming the file little by little or loading the entire iso into memory and then writing to disk)

## Current status

Supported on MacOS, Windows, and Linux.
MacOS users have to compile from source because i can't do it myself on Linux

## Requirements

- Rust and Cargo
- A target USB device 
- A graphical desktop environment (if on linux)

## Running the app

I recommend using the prebuilt binary, but you can also run it  with cargo:

```bash
cargo run
```

If you want the Dioxus dev workflow instead of the plain desktop app:

```bash
dx serve
```

## Flashing safely

Before flashing, double-check the target device path.

Useful commands:

```bash
lsblk
sudo fdisk -l
```

The app writes directly to the selected block device. Make sure you are targeting the USB stick, not your system drive.

## Project layout

```text
src/
├── main.rs          # app entry point and launch logic
├── list_handler.rs  # distro catalog loading, caching, and UI
└── flashing/        # download + flashing logic
    ├── mod.rs
    ├── flash_handler.rs
    ├── flasher_unix.rs
    └── flasher_win.rs
assets/
├── distros.json     # distro catalog used by the app
├── logos/           # bundled official artwork for catalog distros
└── main.css         # styling
```

## Bundled distro logos

Catalog logos are bundled under `assets/logos/` so known distros render offline and retain their official identity even when a cached catalog contains an older image URL. The assets are copied verbatim from their primary project sources; Tiny Core's PNG is the unmodified official `images/logo.png` (also shipped as `/usr/local/share/pixmaps/logo.png` in `Xprogs.tcz`).

| Distro | Official source |
| --- | --- |
| Arch Linux | [archlinux-common-style favicon](https://gitlab.archlinux.org/archlinux/archlinux-common-style/-/blob/master/img/favicon.svg) |
| Tiny Core Linux | [Tiny Core logo](http://tinycorelinux.net/images/logo.png) |
| Linux Mint | [brand-logo](https://github.com/linuxmint/brand-logo/blob/master/ring.svg) |
| Ubuntu | [Ubuntu brand assets](https://design.ubuntu.com/brand) |
| Debian | [Debian open-use logo](https://www.debian.org/logos/) |
| Fedora Workstation | [Fedora Project logos](https://gitlab.com/fedora/design/team/logos/fedora-project-logos/-/blob/main/brand-book-assets/logo-svgs/fedora_workstation.svg) |
| Manjaro | [Manjaro branding logo](https://gitlab.manjaro.org/artwork/branding/logo/-/blob/master/logo.svg) |
| openSUSE Tumbleweed | [openSUSE distribution logos](https://github.com/openSUSE/distribution-logos/blob/master/Tumbleweed/square-hicolor.svg) |
| Kali Linux | [Kali graphic resources](https://gitlab.com/kalilinux/documentation/graphic-resources/-/blob/main/kali-icon/kali-Logomark_and_Wordmark.svg) |
| Alpine Linux | [Alpine Linux logo](https://alpinelinux.org/alpinelinux-logo.svg) |
| Gentoo | [Gentoo logo](https://www.gentoo.org/inside-gentoo/artwork/gentoo-logo.html) |
| NixOS | [NixOS branding](https://nixos.org/branding/) |

### Licenses and trademarks

The Debian open-use logo is Copyright © 1999 Software in the Public Interest, Inc. and is available under LGPL-3.0-or-later or CC BY-SA 3.0; the restricted-use Debian logo is not included. Gentoo's vector logo is Copyright Gentoo Foundation and Lennart Andre Rolland and is CC BY-SA 2.5, subject to its [name and logo guidelines](https://www.gentoo.org/inside-gentoo/foundation/name-logo-guidelines.html). Fedora® and its logo are trademarks of Red Hat, Inc.; use follows the [Fedora brand guidelines](https://docs.fedoraproject.org/en-US/project/brand/). The remaining names and logos are subject to their respective projects' published branding, license, and trademark terms.

The GPL-3.0 license for DistroShop applies to this project only. It does not grant rights to, or imply endorsement by, any distro's name, logo, or trademark.

## License

GPL-3.0
