use crate::list_handler::{self, get_config_dir};
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use std::fs::File;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::PathBuf;
use nix::unistd::Uid;
use std::env;
use std::process::Command;
use std::time::Duration;
use tokio::time;
static CSS: Asset = asset!("/assets/main.css");

#[component]
 fn form_handler(distro: list_handler::distro, show_form: Signal<bool>, mut status: Signal<String>) -> Element {
    let mut blockdev = use_signal(String::new);
    let mut selected = use_signal(|| "safe".to_string());

    let on_submit = move |_evt: Event<FormData>| {
        if blockdev != use_signal(|| "".to_string()) { 
            show_form.set(false);
            download_and_flash_handler(&distro, &blockdev.to_string(), selected.to_string(), status).unwrap();
        } else {
            status.set("Please enter a valid device!".to_string());
        }
    };
    rsx! {

        form { onsubmit: on_submit,
            label {"Target Block Device (e.g. /dev/sdb)"}
            input {
                value: "{blockdev}",
                oninput: move |evt| blockdev.set(evt.value()),
            }
            select {
                value: "{selected}",
                onchange: move |evt| selected.set(evt.value()),
                option {value: "fast", "Fast"}
                option {value: "safe", "Safe"}
            }
            button { r#type: "submit", "Confirm" }
        }
    }
}

 fn download_and_flash_handler(
    distro: &list_handler::distro,
    blockdev: &String,
    flashmode: String,
    mut status: Signal<String>,
) -> Result<(), Box<dyn std::error::Error>> 
{
    let distro_to_download = distro.clone();
    let blockdev_for_flash = blockdev.to_string();
    let mut iso_filename = get_config_dir().to_string_lossy().into_owned();
    iso_filename.push_str(&distro.filename);
    let flashmode_for_flash = flashmode.clone();

    
        spawn_forever(async move {

            status.set("Downloading iso image (will take a while)".to_string());
            match download_distro(&distro_to_download).await {
             Ok(_) => {
                status.set("Flashing to block device (ui might freeze and that's normal)".to_string());
                tokio::time::sleep(Duration::from_millis(200)).await;
                match flasher(&blockdev_for_flash, &iso_filename, &flashmode_for_flash) {
                    Ok(()) => {
                        status.set("Successfully flashed iso image!".to_string());
                        time::sleep(Duration::from_secs(2)).await;
                        status.set("".to_string());
                    }
                    Err(e) => status.set(format!("Flashing failed: {e}")),
                }
            }
        Err(e) => status.set(format!("Download failed: {e}")),
            }
        });

    Ok(())
}

async fn download_distro(distro: &list_handler::distro) -> Result<(), Box<dyn std::error::Error>> {
    let distro= distro.clone();
    info!{"requesting {}", distro.downloadlink};
    let response = reqwest::get(distro.downloadlink).await?.error_for_status()?;
    info!("got headers: {} len={:?}", response.status(), response.content_length());

    let mut file_path = PathBuf::from(get_config_dir());
    fs::create_dir_all(&file_path)?; // already creating if list isnt found but may be an edge case; flash_handler/ln86
    file_path.push(distro.filename);

    let contents = response.bytes().await?;
    info!("got body: {} bytes", contents.len());
    fs::write(&file_path, contents)?;
    info!("wrote {:?}", file_path);

    Ok(())
}

pub fn flasher(blockdev: &str, isoimg: &str, flashmode: &str) -> io::Result<()> {

    let args = vec![blockdev, isoimg, flashmode]; //the original arguments get shadowed later 

    if Uid::current().is_root(){
    let mut blockdev = File::options().write(true).open(&blockdev)?;
    let mut isoimg = File::open(&isoimg)?;
    match flashmode {
        "safe" => {
            info!("using safe mode");
            io::copy(&mut isoimg, &mut blockdev)?;
            blockdev.sync_all()?;
        }
        "fast" => {
            info!("using fast mode");
            let mut buffer = Vec::new();
            isoimg.read_to_end(&mut buffer)?;
            blockdev.write_all(&buffer)?;
            blockdev.sync_all()?;
        }
        _ => {
            error!("invalid flash mode");
            std::process::exit(1);
        }
    }
 } else {
    let exe_path = env::current_exe()?;
    let status = Command::new("pkexec")
    .arg(&exe_path)
    .arg("-f")
    .args(&args) 
    .status()?;
    
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("pkexec exited with {status}")
        ));
    }

 }
    Ok(())
}
#[component]
pub fn show_more(distro: list_handler::distro, is_showing: Signal<bool>) -> Element {
    let mut show_form = use_signal(|| false);
    let  status = use_signal(|| "".to_string());
    let back_button_handler = move |_: MouseEvent| {
        if show_form(){
            show_form.set(false);
        } else {
            is_showing.set(false);
        }
    };
    rsx! {
        document::Stylesheet { href: CSS }
        div { style: "position: absolute; width: 100vw; height: 100vh; overflow: hidden;",

            div { style: "
                    position: fixed;
                    top: 0;
                    left: 0;
                    width: 100%;
                    height: 100%;
                    pointer-events: none; 
                    z-index: 10;
                    background: radial-gradient(ellipse at center, rgba(0, 0, 0, 0) 40%, rgba(0, 0, 0, 0.85) 100%);
                " }

            div { style: "
                    display: flex;
                    flex-direction: column;
                    align-items: flex-start;
                    gap: 8px;
                    position: absolute;
                    top: 50%;
                    left: 50%;
                    transform: translate(-50%, -50%);
                    z-index: 50; 
                    padding: 2rem;
                    background: black;
                    border-radius: 8px;
                    box-shadow: 0 10px 25px rgba(0,0,0,0.5);
                ",
                button { onclick: back_button_handler, "back" }
                img {
                    class: "center",
                    style: "max-width:100px; max-height:100px; width: auto; height:auto; ",
                    src: "{distro.image}",
                }
                h1 { style: "text-align:center; margin: auto",
                "{distro.name}" }
                p { class: "center", "{distro.descriptionfull}" }
                h3 {class: "center", "{status}"}
                if show_form() {
                    form_handler { distro: distro.clone(), show_form: show_form,status: status, }
                } else {
                    button { onclick: move |_| show_form.set(true), style: "text-align: center; margin: auto", "download and flash" }
                }
            }
        }
    }
}
