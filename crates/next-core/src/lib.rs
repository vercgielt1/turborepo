#![feature(async_closure)]
#![feature(min_specialization)]

mod app_render;
mod app_source;
mod embed_js;
pub mod env;
mod fallback;
pub mod manifest;
mod next_build;
pub mod next_client;
mod next_client_component;
pub mod next_config;
mod next_font_google;
pub mod next_image;
mod next_import_map;
pub mod next_server;
pub mod next_shared;
mod page_loader;
mod page_source;
pub mod react_refresh;
mod runtime;
mod util;
mod web_entry_source;

pub use app_source::create_app_source;
pub use page_source::create_page_source;
pub use turbopack_node::source_map;
pub use web_entry_source::create_web_entry_source;

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbo_tasks_fetch::register();
    turbopack_dev_server::register();
    turbopack_node::register();
    turbopack::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
