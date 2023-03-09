use anyhow::Result;
use indoc::formatdoc;
use turbo_tasks::{primitives::StringVc, Value, ValueToString, ValueToStringVc};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack::ecmascript::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemContentVc,
        EcmascriptChunkItemVc, EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc,
        EcmascriptChunkVc, EcmascriptExports, EcmascriptExportsVc,
    },
    utils::stringify_js,
};
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    chunk::{
        availablility_info::AvailablilityInfo, Chunk, ChunkGroupVc, ChunkItem, ChunkItemVc,
        ChunkVc, ChunkableAsset, ChunkableAssetReference, ChunkableAssetReferenceVc,
        ChunkableAssetVc, ChunkingContext, ChunkingContextVc, ChunkingType, ChunkingTypeOptionVc,
    },
    ident::AssetIdentVc,
    reference::{AssetReference, AssetReferenceVc, AssetReferencesVc},
    resolve::{ResolveResult, ResolveResultVc},
};
use turbopack_ecmascript::utils::stringify_js_pretty;

#[turbo_tasks::function]
fn modifier() -> StringVc {
    StringVc::cell("client chunks".to_string())
}

#[turbo_tasks::value(shared)]
pub struct WithClientChunksAsset {
    pub asset: EcmascriptChunkPlaceableVc,
    pub server_root: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl Asset for WithClientChunksAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> AssetIdentVc {
        self.asset.ident().with_modifier(modifier())
    }

    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        unimplemented!()
    }

    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        AssetReferencesVc::cell(vec![WithClientChunksAssetReference {
            asset: self.asset.into(),
        }
        .cell()
        .into()])
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for WithClientChunksAsset {
    #[turbo_tasks::function]
    fn as_chunk(
        self_vc: WithClientChunksAssetVc,
        context: ChunkingContextVc,
        availablility_info: Value<AvailablilityInfo>,
    ) -> ChunkVc {
        EcmascriptChunkVc::new(
            context.with_layer("rsc"),
            self_vc.as_ecmascript_chunk_placeable(),
            availablility_info,
        )
        .into()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for WithClientChunksAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self_vc: WithClientChunksAssetVc,
        context: ChunkingContextVc,
    ) -> EcmascriptChunkItemVc {
        WithClientChunksChunkItem {
            context: context.with_layer("rsc"),
            inner: self_vc,
        }
        .cell()
        .into()
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> EcmascriptExportsVc {
        // TODO This should be EsmExports
        EcmascriptExports::Value.cell()
    }
}

#[turbo_tasks::value]
struct WithClientChunksChunkItem {
    context: ChunkingContextVc,
    inner: WithClientChunksAssetVc,
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for WithClientChunksChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> ChunkingContextVc {
        self.context
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let inner = self.inner.await?;
        let group = ChunkGroupVc::from_asset(
            inner.asset.into(),
            self.context,
            Value::new(AvailablilityInfo::Root {
                current_availability_root: inner.asset.into(),
            }),
        );
        let chunks = group.chunks().await?;
        let server_root = inner.server_root.await?;

        let mut asset_paths = vec![];
        for chunk in chunks.iter() {
            for reference in chunk.references().await?.iter() {
                let assets = &*reference.resolve_reference().primary_assets().await?;
                for asset in assets.iter() {
                    asset_paths.push(asset.ident().path().await?);
                }
            }

            asset_paths.push(chunk.path().await?);
        }

        let mut client_chunks = Vec::new();
        for asset_path in asset_paths {
            if let Some(path) = server_root.get_path_to(&asset_path) {
                client_chunks.push(path.to_string());
            }
        }

        let module_id = stringify_js(&*inner.asset.as_chunk_item(self.context).id().await?);
        Ok(EcmascriptChunkItemContent {
            inner_code: formatdoc!(
                // We store the chunks in a binding, otherwise a new array would be created every
                // time the export binding is read.
                r#"
                    __turbopack_esm__({{
                        default: () => __turbopack_import__({}),
                        chunks: () => chunks,
                    }});
                    const chunks = {};
                "#,
                module_id,
                stringify_js_pretty(&client_chunks),
            )
            .into(),
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for WithClientChunksChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> AssetIdentVc {
        self.inner.ident()
    }

    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        self.inner.references()
    }
}

#[turbo_tasks::value]
struct WithClientChunksAssetReference {
    asset: AssetVc,
}

#[turbo_tasks::value_impl]
impl ValueToString for WithClientChunksAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "local asset {}",
            self.asset.ident().to_string().await?
        )))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for WithClientChunksAssetReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> ResolveResultVc {
        ResolveResult::asset(self.asset).cell()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAssetReference for WithClientChunksAssetReference {
    #[turbo_tasks::function]
    fn chunking_type(&self) -> ChunkingTypeOptionVc {
        ChunkingTypeOptionVc::cell(Some(ChunkingType::IsolatedParallel))
    }
}
