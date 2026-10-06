use crate::flashing::flash_handler;
use dioxus::prelude::*;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const DISTROLISTGITHUB: &str =
    "https://raw.githubusercontent.com/TechCore3/DistroShop/refs/heads/main/assets/distros.json";
static CSS: &str = include_str!("../assets/main.css");
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to create HTTP client")
});

#[derive(Deserialize, Clone, PartialEq)]
#[allow(non_camel_case_types)] // cant be bothered to change case at this point in development lol
pub struct distro {
    pub id: u8, //might expand later who knows
    pub name: String,
    pub description: String,
    pub descriptionfull: String,
    pub image: String,
    pub downloadlink: String,
    pub filename: String,
    #[serde(skip)]
    pub logo_source: Option<Arc<str>>,
}

pub fn get_config_dir() -> PathBuf {
    let mut home_dir = dirs::home_dir().expect("unable to find home dir!");
    home_dir.push(".config/distroshop/"); // %LOCALAPPDATA% be damned
    home_dir
}

fn get_local_distro_list() -> PathBuf {
    let mut path = get_config_dir();
    path.push("distros.json");
    path
}

fn read_from_disk() -> Result<Vec<distro>, Box<dyn std::error::Error>> {
    let file_path = get_local_distro_list();
    info!("reading from cached JSON file");
    let file_content = fs::read_to_string(file_path)?;
    let items: Vec<distro> = serde_json::from_str(&file_content)?;

    Ok(items)
}

async fn download_and_save_list() -> Result<Vec<distro>, Box<dyn std::error::Error>> {
    let response = HTTP_CLIENT
        .get(DISTROLISTGITHUB)
        .send()
        .await?
        .error_for_status()?;
    let json_text = response.text().await?;
    let distros: Vec<distro> = serde_json::from_str(&json_text)?;

    fs::create_dir_all(get_config_dir())?;
    fs::write(get_local_distro_list(), json_text)?;

    Ok(distros)
}

fn keep_visible_logo_sources(mut updated: Vec<distro>, current: &[distro]) -> Vec<distro> {
    for distro in &mut updated {
        if let Some(previous) = current.iter().find(|previous| previous.id == distro.id) {
            distro.logo_source = previous.logo_source.clone();
        }
    }
    updated
}

async fn hydrate_logos(
    mut items: Vec<distro>,
    cache_dir: &Path,
    refresh: bool,
) -> (Vec<distro>, Vec<String>) {
    let mut tasks = tokio::task::JoinSet::new();

    for (index, distro) in items.iter().enumerate() {
        let client = HTTP_CLIENT.clone();
        let cache_dir = cache_dir.to_path_buf();
        let id = distro.id;
        let url = distro.image.clone();

        tasks.spawn(async move {
            let cached = crate::logo_source::load_logo(&client, &cache_dir, id, &url, refresh).await;
            (index, cached)
        });
    }

    let mut warnings = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((index, cached)) => {
                items[index].logo_source = cached.source;
                if let Some(warning) = cached.warning {
                    error!("logo refresh failed for {}: {warning}", items[index].name);
                    warnings.push(items[index].name.clone());
                }
            }
            Err(error) => {
                error!("logo refresh task failed: {error}");
                warnings.push("an image task".to_string());
            }
        }
    }

    (items, warnings)
}

fn image_status(catalog_error: Option<String>, warnings: Vec<String>) -> String {
    let mut messages = Vec::new();
    if let Some(error) = catalog_error {
        messages.push(error);
    }
    if !warnings.is_empty() {
        messages.push(format!(
            "Logo refresh failed for {}. Cached images were retained where available; retry Refresh list.",
            warnings.join(", ")
        ));
    }
    messages.join(" ")
}

#[component]
pub fn distro_list() -> Element {
    let mut items_signal = use_signal(Vec::<distro>::new);
    let mut status_signal = use_signal(|| String::from("Initializing..."));
    let mut is_loading = use_signal(|| true);
    let mut active_distro_id = use_signal(|| None::<u8>);
    let mut is_showing_more = use_signal(|| false);

    use_effect(move || {
        spawn(async move {
            let items = match read_from_disk() {
                Ok(items) => {
                    items_signal.set(items.clone());
                    items
                }
                Err(error) => {
                    info!("no usable cached list: {error}");
                    status_signal.set("Downloading distro catalog...".to_string());
                    match download_and_save_list().await {
                        Ok(items) => {
                            items_signal.set(items.clone());
                            items
                        }
                        Err(download_error) => {
                            error!("initial catalog download failed: {download_error}");
                            status_signal.set(format!("Catalog download failed: {download_error}"));
                            is_loading.set(false);
                            return;
                        }
                    }
                }
            };

            status_signal.set("Loading distro images...".to_string());
            let (items, warnings) =
                hydrate_logos(items, &get_config_dir().join("images"), false).await;
            items_signal.set(items);
            status_signal.set(image_status(None, warnings));
            is_loading.set(false);
        });
    });

    let handle_manual_sync = move |_| {
        to_owned![items_signal, status_signal, is_loading];
        let current_items = items_signal();
        spawn(async move {
            is_loading.set(true);
            status_signal.set("Refreshing distro catalog and images...".to_string());

            let (items, catalog_error) = match download_and_save_list().await {
                Ok(items) => {
                    let items = keep_visible_logo_sources(items, &current_items);
                    items_signal.set(items.clone());
                    info!("successfully updated local list");
                    (items, None)
                }
                Err(error) => {
                    error!("manual catalog refresh failed: {error}");
                    (
                        current_items,
                        Some(format!(
                            "Catalog refresh failed: {error}. Refreshing cached distro images."
                        )),
                    )
                }
            };

            let (items, warnings) =
                hydrate_logos(items, &get_config_dir().join("images"), true).await;
            items_signal.set(items);
            status_signal.set(image_status(catalog_error, warnings));
            is_loading.set(false);
        });
    };
    let distros = items_signal.read();
    rsx! {
        document::Style { "{CSS}" }
        div { class: "app-shell",
            div { class: "toolbar",
                div { class: "toolbar-copy",
                    h1 { class: "app-title", "DistroShop" }
                }
                div { class: "toolbar-actions",
                    button { class: "primary-button", onclick: handle_manual_sync, disabled: is_loading(),
                        if is_loading() {
                            "Syncing..."
                        } else {
                            "Refresh list"
                        }
                    }
                }
            }
            p { class: "status-line", "{status_signal}" }
            
            div {
                id: "distrolist",
                for distro in distros.iter().cloned().collect::<Vec<_>>() {
                    
                    li { class: "distro-card",
                        div { class: "distro-image-frame",
                            if let Some(source) = &distro.logo_source {
                                img {
                                    class: "distro-image",
                                    src: "{source}",
                                    alt: "{distro.name} logo",
                                }
                            }
                        }
                        div { class: "distro-details",
                            h3 { class: "distro-title", "{distro.name}" }
                            p { class: "distro-description", "{distro.description}" }
                        }
                        div { class: "distro-actions",
                            button {
                                class: "distro-button",
                                onclick: move |_| {
                                    active_distro_id.set(Some(distro.id));
                                    is_showing_more.set(true);
                                },
                                "Show more"
                            }
                        }
                    }
                    if active_distro_id() == Some(distro.id) && is_showing_more() {
                        flash_handler::show_more { distro: distro.clone(), is_showing: is_showing_more }
                    }
                }
            }
        }
    }
}
