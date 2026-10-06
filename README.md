# DistroShop

A small Linux desktop app for browsing distro images and flashing them to a USB stick.

This project is still early-stage, but the basic flow is there: it loads a distro catalog, lets you view details, downloads an ISO, and writes it to a target device.

## What it does

- Shows a list of Linux distros from a remote JSON catalog
- Caches the list locally in `~/.config/distroshop/distros.json`
- Lets you refresh the catalog manually
- Displays each distro's image, summary, and full description
- Uses responsive distro cards with consistent inner padding and bottom-aligned actions; cards grow to fit wrapped text
- Keeps the title, refresh button, and status visible while only the distro-card area scrolls, including in small windows
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
├── logo_source.rs   # persistent distro image cache and HTTP revalidation
└── flashing/        # download + flashing logic
    ├── mod.rs
    ├── flash_handler.rs
    ├── flasher_unix.rs
    └── flasher_win.rs
assets/
├── distros.json     # distro catalog used by the app
├── logo.png         # bundled application icon (not a distro logo)
└── main.css         # styling
```

## Distro image cache

Distro logos are downloaded, never bundled as image bytes. The loaded catalog's `image` URL is always preferred. If it fails or does not return an image, DistroShop tries its optional `image_fallbacks` URLs, then the URLs in the release catalog for the same numeric ID and case-insensitive distro name. Duplicate URLs are tried only once per loading pass, apart from bounded retries for transient failures.

- On first startup, missing images are downloaded and cached under `~/.config/distroshop/images/`. Each logo appears as soon as it is ready; a slow source does not hold up the other cards.
- Later startups reuse valid cached images without making image requests, including images downloaded from alternate sources. The requested primary URL must still match and the actual source must remain an allowed candidate. Missing or corrupt entries, or entries whose preferred URL changed, are fetched again.
- **Refresh list** downloads the latest catalog and checks the preferred image sources again before trying alternatives. ETag and Last-Modified validators are sent only to the URL that supplied the cached image. Existing images stay visible throughout refresh.
- Requests have connection and overall timeouts. Transient failures get at most one retry per URL; permanent errors and non-image responses move directly to the next source.
- If every source fails, the last cached image is retained and a warning is shown. A never-cached image stays empty when none of its sources is reachable. Successful alternatives are reported separately from failures.
- Cards and details dialogs share the same cached image. Each numeric distro ID has a cache entry containing the requested primary URL, actual source URL, image data, and HTTP validators. Older cache entries remain readable.

The application reads `~/.config/distroshop/distros.json`, or fetches the upstream catalog when none is usable. If neither is available, it shows release catalog metadata with a warning. Refresh replaces the cached catalog when successful and still refreshes displayed images if the catalog request fails.

`assets/distros.json` is embedded as URL/catalog metadata, so its alternate sources also work with older upstream catalogs. It does not override their preferred image URLs. The only embedded image remains `assets/logo.png`, the application's own window icon; no distro logo files are included.

### Licenses and trademarks

Downloaded images retain their respective projects' copyright, licensing, and trademark terms.

The GPL-3.0 license for DistroShop applies to this project only. It does not grant rights to, or imply endorsement by, any distro's name, logo, or trademark.

## License

GPL-3.0
