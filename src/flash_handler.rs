use crate::list_handler::{self, get_config_dir};
use dioxus::prelude::*;
use std::fs::File;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::PathBuf;
use nix::unistd::Uid;
use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

static CSS: Asset = asset!("/assets/main.css");

#[component]
 fn form_handler(distro: list_handler::distro) -> Element {
    let mut blockdev = use_signal(String::new);
    let on_submit = move |_evt: Event<FormData>| {
        download_and_flash_handler(&distro, &blockdev.to_string());
    };
    rsx! {
        form { onsubmit: on_submit,
            label {"Target Block Device (e.g. /dev/sdb)"}
            input {
                value: "{blockdev}",
                oninput: move |evt| blockdev.set(evt.value()),
            }
            button { r#type: "submit", "Confirm" }
        }
    }
}

async fn download_and_flash_handler(distro: &list_handler::distro, blockdev: &String) {
    let distro = distro.clone();
    if let Ok(()) = download_distro(&distro).await {
        todo!{}
    }

}

async fn download_distro(distro: &list_handler::distro) -> Result<(), Box<dyn std::error::Error>> {
    let distro= distro.clone();
    let response = reqwest::get(distro.downloadlink).await?;
    let mut file_path = PathBuf::from(get_config_dir());
    fs::create_dir_all(&file_path)?; // already creating if list isnt found but may be an edge case; flash_handler/ln86
    file_path.push(distro.filename);

    let contents = response.bytes().await?;
    fs::write(file_path, contents)?;

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
    let exe_path = env::current_exe().expect("failed to read /proc/self/exe");
    let err = Command::new("pkexec")
    .arg(&exe_path)
    .arg("-f")
    .args(&args) 
    .exec();
    eprintln!("failed to exec pkexec: {err}");
    std::process::exit(1); 
 }
    Ok(()) //unreachable but needed for previous '?' operators
}
#[component]
pub fn show_more(distro: list_handler::distro, is_showing: Signal<bool>) -> Element {
    let mut show_form = use_signal(|| false);

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
                button { onclick: move |_| is_showing.set(false), "back" }
                img {
                    class: "center",
                    style: "max-width:100px; max-height:100px; width: auto; height:auto; ",
                    src: "{distro.image}",
                }
                h1 { "{distro.name}" }
                p { class: "center", "{distro.descriptionfull}" }
                if show_form() {
                    form_handler { distro: distro.clone() }
                }
                button { onclick: move |_| show_form.set(true), "download and flash" }
            }
        }
    }
}
