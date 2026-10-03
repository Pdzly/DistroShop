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
- Offers a `safe` and `fast` flash mode

## Current status

This is not a polished release and things may change quickly.

It works best for a simple local desktop workflow on Linux (Windows support is planned and will release before HL3). Make sure to use `lsblk` to check block device, because writing to a block device deletes previous data on it.

## Requirements

- Rust and Cargo
- Linux desktop environment
- A target USB device such as `/dev/sdb`
- `pkexec` 

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
├── flash_handler.rs  # download + flashing logic
assets/
├── distros.json     # distro catalog used by the app
├── main.css        # styling
```

## License

GPL-3.0
