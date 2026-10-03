use std::env;
use dioxus::prelude::*;
use std::io;
use crate::list_handler::distro_list;

pub mod flash_handler;
pub mod list_handler;
static CSS: &str = include_str!("../assets/main.css");

#[component]
fn app() -> Element {
    rsx! {
        document::Style { "{CSS}" }
        div { distro_list {} }
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 { 
        flash_handler::flasher(&args[2], &args[3], &args[4])? ;
        } else {
        dioxus::launch(app);
    }
    Ok(())
}
