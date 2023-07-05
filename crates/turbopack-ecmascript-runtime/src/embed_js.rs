use turbo_tasks::Vc;
use turbo_tasks_fs::{embed_directory, FileContent, FileSystem, FileSystemPath};
use turbopack_core::{code_builder::Code, context::AssetContext};
use turbopack_ecmascript::StaticEcmascriptCode;

#[turbo_tasks::function]
pub fn embed_fs() -> Vc<Box<dyn FileSystem>> {
    embed_directory!("turbopack", "$CARGO_MANIFEST_DIR/js/src")
}

#[turbo_tasks::function]
pub fn embed_file(path: String) -> Vc<FileContent> {
    embed_fs().root().join(path).read()
}

#[turbo_tasks::function]
pub fn embed_file_path(path: String) -> Vc<FileSystemPath> {
    embed_fs().root().join(path)
}

#[turbo_tasks::function]
pub fn embed_static_code(asset_context: Vc<AssetContext>, path: &str) -> Vc<Code> {
    StaticEcmascriptCode::new(asset_context, embed_file_path(path)).code()
}
