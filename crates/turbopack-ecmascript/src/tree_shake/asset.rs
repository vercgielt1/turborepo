use anyhow::Result;
use turbo_tasks::{primitives::StringVc, Value};
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    chunk::{
        availability_info::AvailabilityInfo, ChunkVc, ChunkableAsset, ChunkableAssetVc,
        ChunkingContextVc,
    },
    ident::AssetIdentVc,
    reference::{AssetReferencesVc, SingleAssetReferenceVc},
    resolve::{ModulePart, ModulePartVc},
};

use super::{
    chunk_item::{EcmascriptModulePartChunkItem, EcmascriptModulePartChunkItemVc},
    get_part_id, split_module,
};
use crate::{
    chunk::{
        EcmascriptChunkItemVc, EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc,
        EcmascriptChunkVc, EcmascriptExportsVc,
    },
    references::analyze_ecmascript_module,
    AnalyzeEcmascriptModuleResultVc, EcmascriptModuleAssetVc,
};

/// A reference to part of an ES module.
///
/// This type is used for an advanced tree shkaing.
#[turbo_tasks::value]
pub struct EcmascriptModulePartAsset {
    full_module: EcmascriptModuleAssetVc,
    part: ModulePartVc,
}

impl EcmascriptModulePartAssetVc {
    /// Create a new instance of [EcmascriptModulePartAssetVc], whcih consists
    /// of a pointer to the full module and the [ModulePart] pointing the part
    /// of the module.
    pub fn new(module: EcmascriptModuleAssetVc, part: ModulePartVc) -> Result<Self> {
        Ok(EcmascriptModulePartAsset {
            full_module: module,
            part,
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl Asset for EcmascriptModulePartAsset {
    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        // This is not reachable because EcmascriptModulePartAsset implements
        // ChunkableAsset and ChunkableAsset::as_chunk is called instead.
        unreachable!("EcmascriptModulePartAsset::content cannot be called directly")
    }

    #[turbo_tasks::function]
    async fn references(&self) -> Result<AssetReferencesVc> {
        let split_data = split_module(self.full_module).await?;
        let part_id = match get_part_id(&split_data, self.part).await {
            Ok(v) => v,
            Err(_) => return Ok(self.full_module.references()),
        };

        let deps = match split_data.deps.get(&part_id) {
            Some(v) => v,
            None => return Ok(self.full_module.references()),
        };

        let mut assets = deps
            .iter()
            .map(|&part_id| {
                Ok(SingleAssetReferenceVc::new(
                    EcmascriptModulePartAssetVc::new(
                        self.full_module,
                        ModulePartVc::new(ModulePart::Internal(part_id)),
                    )?
                    .as_asset(),
                    StringVc::cell("ecmascript module part".to_string()),
                )
                .as_asset_reference())
            })
            .collect::<Result<Vec<_>>>()?;

        let external = self.full_module.references().await?;

        assets.extend(external.iter().cloned());

        Ok(AssetReferencesVc::cell(assets))
    }

    #[turbo_tasks::function]
    async fn ident(&self) -> Result<AssetIdentVc> {
        let inner = self.full_module.ident();

        Ok(inner.with_part(self.part))
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for EcmascriptModulePartAsset {
    #[turbo_tasks::function]
    async fn as_chunk_item(
        self_vc: EcmascriptModulePartAssetVc,
        context: ChunkingContextVc,
    ) -> Result<EcmascriptChunkItemVc> {
        let s = self_vc.await?;

        Ok(
            EcmascriptModulePartChunkItemVc::new(EcmascriptModulePartChunkItem {
                module: self_vc,
                context,
                full_module: s.full_module,
                part: s.part,
            })
            .into(),
        )
    }

    #[turbo_tasks::function]
    async fn get_exports(self_vc: EcmascriptModuleAssetVc) -> Result<EcmascriptExportsVc> {
        Ok(self_vc.analyze().await?.exports)
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for EcmascriptModulePartAsset {
    #[turbo_tasks::function]
    async fn as_chunk(
        self_vc: EcmascriptModulePartAssetVc,
        context: ChunkingContextVc,
        availability_info: Value<AvailabilityInfo>,
    ) -> ChunkVc {
        EcmascriptChunkVc::new(
            context,
            self_vc.as_ecmascript_chunk_placeable(),
            availability_info,
        )
        .into()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptModulePartAssetVc {
    #[turbo_tasks::function]
    pub(super) async fn analyze(self) -> Result<AnalyzeEcmascriptModuleResultVc> {
        let part = self.await?;
        let this = part.full_module.await?;
        Ok(analyze_ecmascript_module(
            this.source,
            part.full_module.as_resolve_origin(),
            Value::new(this.ty),
            this.transforms,
            Value::new(this.options),
            this.compile_time_info,
            Some(part.part),
        ))
    }
}
