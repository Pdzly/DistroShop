# DistroShop

A small Linux desktop app for browsing distro images and flashing them to a USB stick.

This project is still early-stage, but the basic flow is there: it loads a distro catalog, lets you view details, downloads an ISO, and writes it to a target device.

## What it does

- Shows a list of Linux distros from a remote JSON catalog
- Caches the list locally in `~/.config/distroshop/distros.json`
- Lets you refresh the catalog manually
- Displays each distro's image, summary, and full description
- Uses responsive distro cards with consistent inner padding and bottom-aligned actions; cards grow to fit wrapped text
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

Distro logos are downloaded from the `image` URL in the loaded `distros.json`; they are not bundled in the executable or selected by distro name.

- On first startup, missing images are downloaded and cached under `~/.config/distroshop/images/`.
- Later startups reuse valid cached images without making image requests. Missing or corrupt entries, or entries whose catalog URL changed, are downloaded again.
- **Refresh list** downloads the latest catalog and checks its image URLs for changes. ETag and Last-Modified validators avoid downloading unchanged images when the server supports them; otherwise the returned image content is compared with the cache.
- Failed updates keep the last cached image and show a warning. If an image has never been cached, its frame stays empty until the URL becomes reachable.
- The cards and details dialog use the same cached image. Each numeric distro ID has a cache entry containing the source URL, image data, and HTTP validators.

The application reads the cached catalog at `~/.config/distroshop/distros.json`, or fetches the upstream catalog when none is usable. Refresh also replaces that catalog, so changing the repository's `assets/distros.json` only affects remote refreshes after the updated catalog is published upstream.

Only `assets/logo.png`, the application's own window icon, remains embedded at build time.

### Licenses and trademarks

Downloaded images retain their respective projects' copyright, licensing, and trademark terms.

The GPL-3.0 license for DistroShop applies to this project only. It does not grant rights to, or imply endorsement by, any distro's name, logo, or trademark.

## License

GPL-3.0
