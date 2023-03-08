use anyhow::Result;
use swc_core::{
    common::GLOBALS,
    ecma::{
        codegen::{text_writer::JsWriter, Emitter},
        visit::{VisitMutWith, VisitMutWithPath},
    },
};
use turbo_tasks::TryJoinIterExt;
use turbopack_core::{
    asset::Asset,
    chunk::{ChunkItem, ChunkItemVc, ChunkingContextVc, ModuleId, ModuleIdVc},
    ident::AssetIdentVc,
    reference::AssetReferencesVc,
    resolve::{origin::ResolveOrigin, ModulePart, ModulePartVc},
};

use super::{asset::EcmascriptModulePartAssetVc, part_of_module};
use crate::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemContentVc,
        EcmascriptChunkItemOptions, EcmascriptChunkItemVc,
    },
    code_gen::{CodeGenerateable, CodeGenerateableVc},
    parse::ParseResult,
    path_visitor::ApplyVisitors,
    references::AnalyzeEcmascriptModuleResult,
    transform::remove_shebang,
    EcmascriptModuleAssetVc, ParseResultSourceMap,
};

#[turbo_tasks::value]
pub struct EcmascriptModulePartChunkItem {
    pub(super) full_module: EcmascriptModuleAssetVc,

    pub(super) module: EcmascriptModulePartAssetVc,
    pub(super) context: ChunkingContextVc,

    pub(super) part: ModulePartVc,
}

impl EcmascriptModulePartChunkItemVc {
    pub(super) fn new(data: EcmascriptModulePartChunkItem) -> Self {
        data.cell()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for EcmascriptModulePartChunkItem {
    #[turbo_tasks::function]
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let context = self.context;

        let AnalyzeEcmascriptModuleResult {
            references,
            code_generation,
            ..
        } = &*self.module.analyze().await?;

        let mut code_gens = Vec::new();
        for r in references.await?.iter() {
            if let Some(code_gen) = CodeGenerateableVc::resolve_from(r).await? {
                code_gens.push(code_gen.code_generation(context));
            }
        }
        for c in code_generation.await?.iter() {
            let c = c.resolve().await?;
            code_gens.push(c.code_generation(context));
        }
        // need to keep that around to allow references into that
        let code_gens = code_gens.into_iter().try_join().await?;
        let code_gens = code_gens.iter().map(|cg| &**cg).collect::<Vec<_>>();
        // TOOD use interval tree with references into "code_gens"
        let mut visitors = Vec::new();
        let mut root_visitors = Vec::new();
        for code_gen in code_gens {
            for (path, visitor) in code_gen.visitors.iter() {
                if path.is_empty() {
                    root_visitors.push(&**visitor);
                } else {
                    visitors.push((path, &**visitor));
                }
            }
        }

        let parsed = part_of_module(self.split_data, Some(self.chunk_id)).await?;

        if let ParseResult::Ok {
            program,
            source_map,
            globals,
            eval_context,
            ..
        } = &*parsed
        {
            let mut program = program.clone();

            GLOBALS.set(globals, || {
                if !visitors.is_empty() {
                    program.visit_mut_with_path(
                        &mut ApplyVisitors::new(visitors),
                        &mut Default::default(),
                    );
                }
                for visitor in root_visitors {
                    program.visit_mut_with(&mut visitor.create());
                }
                program.visit_mut_with(&mut swc_core::ecma::transforms::base::hygiene::hygiene());
                program.visit_mut_with(&mut swc_core::ecma::transforms::base::fixer::fixer(None));

                // we need to remove any shebang before bundling as it's only valid as the first
                // line in a js file (not in a chunk item wrapped in the runtime)
                remove_shebang(&mut program);
            });

            let mut bytes: Vec<u8> = vec![];
            // TODO: Insert this as a sourceless segment so that sourcemaps aren't affected.
            // = format!("/* {} */\n", self.module.path().to_string().await?).into_bytes();

            let mut srcmap = vec![];

            let mut emitter = Emitter {
                cfg: swc_core::ecma::codegen::Config {
                    ..Default::default()
                },
                cm: source_map.clone(),
                comments: None,
                wr: JsWriter::new(source_map.clone(), "\n", &mut bytes, Some(&mut srcmap)),
            };

            emitter.emit_program(&program)?;

            let srcmap = ParseResultSourceMap::new(source_map.clone(), srcmap).cell();

            Ok(EcmascriptChunkItemContent {
                inner_code: bytes.into(),
                source_map: Some(srcmap),
                options: if eval_context.is_esm() {
                    EcmascriptChunkItemOptions {
                        ..Default::default()
                    }
                } else {
                    EcmascriptChunkItemOptions {
                        // These things are not available in ESM
                        module: true,
                        exports: true,
                        this: true,
                        ..Default::default()
                    }
                },
                ..Default::default()
            }
            .into())
        } else {
            Ok(EcmascriptChunkItemContent {
                inner_code: format!("__turbopack_wip__({{ wip: true }});",).into(),
                ..Default::default()
            }
            .cell())
        }
    }

    #[turbo_tasks::function]
    fn chunking_context(&self) -> ChunkingContextVc {
        self.context
    }

    #[turbo_tasks::function]
    async fn id(&self) -> Result<ModuleIdVc> {
        let module = self.full_module.origin_path().await?;
        let part = self.part.await?;

        match &*part {
            ModulePart::ModuleEvaluation => {
                Ok(ModuleId::String(format!("{} (ecmascript evaluation)", module.path)).into())
            }
            ModulePart::Export(name) => {
                let name = name.await?;
                Ok(
                    ModuleId::String(format!("{} (ecmascript export {})", module.path, name))
                        .into(),
                )
            }
        }
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for EcmascriptModulePartChunkItem {
    #[turbo_tasks::function]
    async fn references(&self) -> AssetReferencesVc {
        self.module.references()
    }

    #[turbo_tasks::function]
    async fn asset_ident(&self) -> Result<AssetIdentVc> {
        Ok(self.module.ident())
    }
}
