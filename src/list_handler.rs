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
static RELEASE_DISTROS: LazyLock<Vec<distro>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/distros.json"))
        .expect("bundled distro metadata must be valid")
});
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("DistroShop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
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
    #[serde(default)]
    pub image_fallbacks: Vec<String>,
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

fn release_distro_metadata() -> &'static [distro] {
    RELEASE_DISTROS.as_slice()
}

fn add_logo_candidate(candidates: &mut Vec<String>, candidate: &str) {
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == candidate) {
        candidates.push(candidate.to_owned());
    }
}

fn logo_candidates_from_metadata(item: &distro, release_metadata: &[distro]) -> Vec<String> {
    let release_item = release_metadata.iter().find(|release_item| {
        release_item.id == item.id && release_item.name.eq_ignore_ascii_case(&item.name)
    });
    let release_capacity = release_item
        .map(|release_item| 1 + release_item.image_fallbacks.len())
        .unwrap_or_default();
    let mut candidates = Vec::with_capacity(1 + item.image_fallbacks.len() + release_capacity);
    add_logo_candidate(&mut candidates, &item.image);
    for fallback in &item.image_fallbacks {
        add_logo_candidate(&mut candidates, fallback);
    }

    if let Some(release_item) = release_item {
        add_logo_candidate(&mut candidates, &release_item.image);
        for fallback in &release_item.image_fallbacks {
            add_logo_candidate(&mut candidates, fallback);
        }
    }

    candidates
}

fn logo_candidates(item: &distro) -> Vec<String> {
    logo_candidates_from_metadata(item, release_distro_metadata())
}

fn keep_visible_logo_sources(mut updated: Vec<distro>, current: &[distro]) -> Vec<distro> {
    for distro in &mut updated {
        if let Some(previous) = current.iter().find(|previous| {
            previous.id == distro.id && previous.name.eq_ignore_ascii_case(&distro.name)
        }) {
            distro.logo_source = previous.logo_source.clone();
        }
    }
    updated
}

async fn hydrate_logos(
    mut items: Vec<distro>,
    cache_dir: &Path,
    refresh: bool,
    mut items_signal: Signal<Vec<distro>>,
) -> (Vec<distro>, Vec<String>, Vec<String>) {
    let mut tasks = tokio::task::JoinSet::new();
    let mut warnings = Vec::new();

    for (index, distro) in items.iter().enumerate() {
        let mut candidates = logo_candidates(distro).into_iter();
        let Some(url) = candidates.next() else {
            error!("no logo URL is configured for {}", distro.name);
            warnings.push(distro.name.clone());
            continue;
        };
        let fallbacks = candidates.collect::<Vec<_>>();
        let client = HTTP_CLIENT.clone();
        let cache_dir = cache_dir.to_path_buf();
        let id = distro.id;

        tasks.spawn(async move {
            let cached =
                crate::logo_source::load_logo(&client, &cache_dir, id, &url, &fallbacks, refresh)
                    .await;
            (index, cached)
        });
    }

    let mut alternate_sources = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((index, cached)) => {
                let used_alternate = cached.used_alternate;
                if used_alternate {
                    alternate_sources.push(items[index].name.clone());
                }
                if let Some(warning) = cached.warning {
                    error!("logo refresh failed for {}: {warning}", items[index].name);
                    warnings.push(items[index].name.clone());
                }
                let logo_source = cached.source;
                items[index].logo_source = logo_source.clone();
                items_signal.write()[index].logo_source = logo_source;
            }
            Err(error) => {
                error!("logo refresh task failed: {error}");
                warnings.push("an image task".to_string());
            }
        }
    }

    (items, warnings, alternate_sources)
}

fn image_status(
    catalog_error: Option<String>,
    warnings: Vec<String>,
    alternate_sources: Vec<String>,
) -> String {
    let mut messages = Vec::new();
    if let Some(error) = catalog_error {
        messages.push(error);
    }
    if !alternate_sources.is_empty() {
        messages.push(format!(
            "Using an alternate logo source for {}.",
            alternate_sources.join(", ")
        ));
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
            let (items, catalog_error) = match read_from_disk() {
                Ok(items) => {
                    items_signal.set(items.clone());
                    (items, None)
                }
                Err(error) => {
                    info!("no usable cached list: {error}");
                    status_signal.set("Downloading distro catalog...".to_string());
                    match download_and_save_list().await {
                        Ok(items) => {
                            items_signal.set(items.clone());
                            (items, None)
                        }
                        Err(download_error) => {
                            error!(
                                "initial catalog download failed after unusable cached catalog: {download_error}"
                            );
                            let items = release_distro_metadata().to_vec();
                            items_signal.set(items.clone());
                            (
                                items,
                                Some(
                                    "Catalog unavailable; showing release catalog metadata. Retry Refresh list."
                                        .to_string(),
                                ),
                            )
                        }
                    }
                }
            };

            status_signal.set("Loading distro images...".to_string());
            let (items, warnings, alternate_sources) = hydrate_logos(
                items,
                &get_config_dir().join("images"),
                false,
                items_signal,
            )
            .await;
            items_signal.set(items);
            status_signal.set(image_status(catalog_error, warnings, alternate_sources));
            is_loading.set(false);
        });
    });

    let handle_manual_sync = move |_| {
        if is_loading() {
            return;
        }

        let current_items = items_signal();
        is_loading.set(true);
        to_owned![items_signal, status_signal, is_loading];
        spawn(async move {
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
                        Some(
                            "Catalog refresh failed; refreshing displayed distro images.".to_string(),
                        ),
                    )
                }
            };

            let (items, warnings, alternate_sources) = hydrate_logos(
                items,
                &get_config_dir().join("images"),
                true,
                items_signal,
            )
            .await;
            items_signal.set(items);
            status_signal.set(image_status(catalog_error, warnings, alternate_sources));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_distro(
        id: u8,
        name: &str,
        image: &str,
        image_fallbacks: &[&str],
    ) -> distro {
        distro {
            id,
            name: name.to_owned(),
            description: String::new(),
            descriptionfull: String::new(),
            image: image.to_owned(),
            image_fallbacks: image_fallbacks.iter().map(|url| (*url).to_owned()).collect(),
            downloadlink: String::new(),
            filename: String::new(),
            logo_source: None,
        }
    }

    #[test]
    fn catalog_urls_precede_matching_release_metadata() {
        let catalog_item = test_distro(
            3,
            "Ubuntu",
            "https://custom.example/ubuntu.svg",
            &[
                "https://custom.example/ubuntu-backup.svg",
                "https://release.example/ubuntu.svg",
            ],
        );
        let release_item = test_distro(
            3,
            "ubuntu",
            "https://release.example/ubuntu.svg",
            &["https://release.example/ubuntu-backup.svg"],
        );

        assert_eq!(
            logo_candidates_from_metadata(&catalog_item, &[release_item]),
            vec![
                "https://custom.example/ubuntu.svg",
                "https://custom.example/ubuntu-backup.svg",
                "https://release.example/ubuntu.svg",
                "https://release.example/ubuntu-backup.svg",
            ]
        );
    }

    #[test]
    fn same_id_with_different_name_does_not_use_release_logo_metadata() {
        let catalog_item = test_distro(7, "custom distribution", "https://custom.example/logo.svg", &[]);
        let release_item = test_distro(7, "openSUSE Tumbleweed", "https://release.example/logo.svg", &[]);

        assert_eq!(
            logo_candidates_from_metadata(&catalog_item, &[release_item]),
            vec!["https://custom.example/logo.svg"]
        );
    }

    #[test]
    fn new_distro_keeps_its_custom_logo_urls() {
        let catalog_item = test_distro(
            99,
            "new distribution",
            "https://custom.example/new.svg",
            &["https://custom.example/new-backup.svg"],
        );
        let unrelated_release_item =
            test_distro(100, "new distribution", "https://release.example/logo.svg", &[]);

        assert_eq!(
            logo_candidates_from_metadata(&catalog_item, &[unrelated_release_item]),
            vec![
                "https://custom.example/new.svg",
                "https://custom.example/new-backup.svg",
            ]
        );
    }

    #[test]
    fn catalog_without_image_fallbacks_remains_compatible() {
        let parsed: distro = serde_json::from_str(
            r#"{
                "id": 42,
                "name": "legacy distro",
                "description": "legacy",
                "descriptionfull": "legacy catalog entry",
                "image": "https://legacy.example/logo.svg",
                "downloadlink": "https://legacy.example/download.iso",
                "filename": "legacy.iso"
            }"#,
        )
        .expect("legacy catalog remains valid");

        assert!(parsed.image_fallbacks.is_empty());
    }
}
