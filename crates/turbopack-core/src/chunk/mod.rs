pub mod availability_info;
pub mod available_chunk_items;
pub mod chunk_group;
pub mod chunking;
pub(crate) mod chunking_context;
pub(crate) mod containment_tree;
pub(crate) mod data;
pub(crate) mod evaluate;
pub mod optimize;
pub(crate) mod passthrough_asset;

use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    future::Future,
    hash::Hash,
};

use anyhow::Result;
use auto_hash_map::AutoSet;
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use tracing::{info_span, Span};
use turbo_tasks::{
    debug::ValueDebugFormat,
    graph::{AdjacencyMap, GraphTraversal, GraphTraversalResult, Visit, VisitControlFlow},
    trace::TraceRawVcs,
    ReadRef, TryFlatJoinIterExt, Upcast, Value, ValueToString, Vc,
};
use turbo_tasks_fs::FileSystemPath;
use turbo_tasks_hash::DeterministicHash;

use self::availability_info::AvailabilityInfo;
pub use self::{
    chunking_context::{ChunkingContext, ChunkingContextExt},
    data::{ChunkData, ChunkDataOption, ChunksData},
    evaluate::{EvaluatableAsset, EvaluatableAssetExt, EvaluatableAssets},
    passthrough_asset::PassthroughModule,
};
use crate::{
    asset::Asset,
    ident::AssetIdent,
    module::Module,
    output::OutputAssets,
    reference::{ModuleReference, ModuleReferences},
};

/// A module id, which can be a number or string
#[turbo_tasks::value(shared)]
#[derive(Debug, Clone, Hash, Ord, PartialOrd, DeterministicHash)]
#[serde(untagged)]
pub enum ModuleId {
    Number(u32),
    String(String),
}

impl Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleId::Number(i) => write!(f, "{}", i),
            ModuleId::String(s) => write!(f, "{}", s),
        }
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for ModuleId {
    #[turbo_tasks::function]
    fn to_string(&self) -> Vc<String> {
        Vc::cell(self.to_string())
    }
}

impl ModuleId {
    pub fn parse(id: &str) -> Result<ModuleId> {
        Ok(match id.parse::<u32>() {
            Ok(i) => ModuleId::Number(i),
            Err(_) => ModuleId::String(id.to_string()),
        })
    }
}

/// A list of module ids.
#[turbo_tasks::value(transparent, shared)]
pub struct ModuleIds(Vec<Vc<ModuleId>>);

/// A [Module] that can be converted into a [Chunk].
#[turbo_tasks::value_trait]
pub trait ChunkableModule: Module + Asset {
    fn as_chunk_item(
        self: Vc<Self>,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
    ) -> Vc<Box<dyn ChunkItem>>;
}

#[turbo_tasks::value(transparent)]
pub struct Chunks(Vec<Vc<Box<dyn Chunk>>>);

#[turbo_tasks::value_impl]
impl Chunks {
    /// Creates a new empty [Vc<Chunks>].
    #[turbo_tasks::function]
    pub fn empty() -> Vc<Self> {
        Vc::cell(vec![])
    }
}

/// A chunk is one type of asset.
/// It usually contains multiple chunk items.
#[turbo_tasks::value_trait]
pub trait Chunk: Asset {
    fn ident(self: Vc<Self>) -> Vc<AssetIdent>;
    fn chunking_context(self: Vc<Self>) -> Vc<Box<dyn ChunkingContext>>;
    // TODO Once output assets have their own trait, this path() method will move
    // into that trait and ident() will be removed from that. Assets on the
    // output-level only have a path and no complex ident.
    /// The path of the chunk.
    fn path(self: Vc<Self>) -> Vc<FileSystemPath> {
        self.ident().path()
    }

    /// Other [OutputAsset]s referenced from this [Chunk].
    fn references(self: Vc<Self>) -> Vc<OutputAssets> {
        OutputAssets::empty()
    }
}

/// Aggregated information about a chunk content that can be used by the runtime
/// code to optimize chunk loading.
#[turbo_tasks::value(shared)]
#[derive(Default)]
pub struct OutputChunkRuntimeInfo {
    pub included_ids: Option<Vc<ModuleIds>>,
    pub excluded_ids: Option<Vc<ModuleIds>>,
    /// List of paths of chunks containing individual modules that are part of
    /// this chunk. This is useful for selectively loading modules from a chunk
    /// without loading the whole chunk.
    pub module_chunks: Option<Vc<OutputAssets>>,
    pub placeholder_for_future_extensions: (),
}

#[turbo_tasks::value_trait]
pub trait OutputChunk: Asset {
    fn runtime_info(self: Vc<Self>) -> Vc<OutputChunkRuntimeInfo>;
}

/// Specifies how a chunk interacts with other chunks when building a chunk
/// group
#[derive(
    Copy, Default, Clone, Hash, TraceRawVcs, Serialize, Deserialize, Eq, PartialEq, ValueDebugFormat,
)]
pub enum ChunkingType {
    /// Asset is placed in the same chunk group and is loaded in parallel. It
    /// doesn't become an async module when the referenced module is async.
    #[default]
    Parallel,
    /// Asset is placed in the same chunk group and is loaded in parallel. It
    /// becomes an async module when the referenced module is async.
    ParallelInheritAsync,
    /// An async loader is placed into the referencing chunk and loads the
    /// separate chunk group in which the asset is placed.
    Async,
}

#[turbo_tasks::value(transparent)]
pub struct ChunkingTypeOption(Option<ChunkingType>);

/// A [ModuleReference] implementing this trait and returning true for
/// [ChunkableModuleReference::is_chunkable] are considered as potentially
/// chunkable references. When all [Module]s of such a reference implement
/// [ChunkableModule] they are placed in [Chunk]s during chunking.
/// They are even potentially placed in the same [Chunk] when a chunk type
/// specific interface is implemented.
#[turbo_tasks::value_trait]
pub trait ChunkableModuleReference: ModuleReference + ValueToString {
    fn chunking_type(self: Vc<Self>) -> Vc<ChunkingTypeOption> {
        Vc::cell(Some(ChunkingType::default()))
    }
}

pub struct ChunkContentResult {
    pub chunk_items: IndexSet<Vc<Box<dyn ChunkItem>>>,
    pub async_modules: IndexSet<Vc<Box<dyn ChunkableModule>>>,
    pub external_module_references: IndexSet<Vc<Box<dyn ModuleReference>>>,
    /// A map from local module to all parents that inherit the async module
    /// status
    pub local_back_edges_inherit_async:
        IndexMap<Vc<Box<dyn ChunkItem>>, Vec<Vc<Box<dyn ChunkItem>>>>,
    /// A map from already available async modules to all local parents that
    /// inherit the async module status
    pub available_async_modules_back_edges_inherit_async:
        IndexMap<Vc<Box<dyn ChunkItem>>, Vec<Vc<Box<dyn ChunkItem>>>>,
}

pub async fn chunk_content(
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    entries: impl IntoIterator<Item = Vc<Box<dyn Module>>>,
    availability_info: Value<AvailabilityInfo>,
) -> Result<ChunkContentResult> {
    chunk_content_internal_parallel(chunking_context, entries, availability_info).await
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum InheritAsyncEdge {
    /// The chunk item is in the current chunk group and async module info need
    /// to be computed for it
    LocalModule,
    /// The chunk item is already available in the parent chunk group and is an
    /// async module. Chunk items that are available but not async modules are
    /// not included in back edges at all since they don't influence the parent
    /// module in terms of being an async module.
    AvailableAsyncModule,
}

#[derive(Eq, PartialEq, Clone, Hash)]
enum ChunkContentGraphNode {
    // An asset not placed in the current chunk, but whose references we will
    // follow to find more graph nodes.
    PassthroughModule {
        module: Vc<Box<dyn Module>>,
    },
    // Chunk items that are placed into the current chunk group
    ChunkItem {
        item: Vc<Box<dyn ChunkItem>>,
        ident: ReadRef<String>,
    },
    // Async module that is referenced from the chunk group
    AsyncModule {
        module: Vc<Box<dyn ChunkableModule>>,
    },
    // ModuleReferences that are not placed in the current chunk group
    ExternalModuleReference(Vc<Box<dyn ModuleReference>>),
    /// A list of directly referenced chunk items from which `is_async_module`
    /// will be inherited.
    InheritAsyncInfo {
        item: Vc<Box<dyn ChunkItem>>,
        references: Vec<(Vc<Box<dyn ChunkItem>>, InheritAsyncEdge)>,
    },
}

#[derive(Clone, Copy)]
struct ChunkContentContext {
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    availability_info: Value<AvailabilityInfo>,
}

async fn reference_to_graph_nodes(
    chunk_content_context: ChunkContentContext,
    reference: Vc<Box<dyn ModuleReference>>,
    parent: Option<Vc<Box<dyn ChunkItem>>>,
) -> Result<Vec<(Option<Vc<Box<dyn Module>>>, ChunkContentGraphNode)>> {
    let Some(chunkable_module_reference) =
        Vc::try_resolve_downcast::<Box<dyn ChunkableModuleReference>>(reference).await?
    else {
        return Ok(vec![(
            None,
            ChunkContentGraphNode::ExternalModuleReference(reference),
        )]);
    };

    let Some(chunking_type) = *chunkable_module_reference.chunking_type().await? else {
        return Ok(vec![(
            None,
            ChunkContentGraphNode::ExternalModuleReference(reference),
        )]);
    };

    let modules = reference.resolve_reference().primary_modules().await?;

    let mut inherit_async_references = Vec::new();

    let mut graph_nodes = vec![];

    for &module in &modules {
        let module = module.resolve().await?;

        if Vc::try_resolve_sidecast::<Box<dyn PassthroughModule>>(module)
            .await?
            .is_some()
        {
            graph_nodes.push((
                Some(module),
                ChunkContentGraphNode::PassthroughModule { module },
            ));
            continue;
        }

        let chunkable_module =
            match Vc::try_resolve_sidecast::<Box<dyn ChunkableModule>>(module).await? {
                Some(chunkable_module) => chunkable_module,
                _ => {
                    return Ok(vec![(
                        None,
                        ChunkContentGraphNode::ExternalModuleReference(reference),
                    )]);
                }
            };

        match chunking_type {
            ChunkingType::Parallel => {
                let chunk_item = chunkable_module
                    .as_chunk_item(chunk_content_context.chunking_context)
                    .resolve()
                    .await?;
                if let Some(available_chunk_items) = chunk_content_context
                    .availability_info
                    .available_chunk_items()
                {
                    if available_chunk_items.get(chunk_item).await?.is_some() {
                        continue;
                    }
                }

                graph_nodes.push((
                    Some(module),
                    ChunkContentGraphNode::ChunkItem {
                        item: chunk_item,
                        ident: module.ident().to_string().await?,
                    },
                ));
            }
            ChunkingType::ParallelInheritAsync => {
                let chunk_item = chunkable_module
                    .as_chunk_item(chunk_content_context.chunking_context)
                    .resolve()
                    .await?;
                if let Some(available_chunk_items) = chunk_content_context
                    .availability_info
                    .available_chunk_items()
                {
                    if let Some(info) = &*available_chunk_items.get(chunk_item).await? {
                        if info.is_async {
                            inherit_async_references
                                .push((chunk_item, InheritAsyncEdge::AvailableAsyncModule));
                        }
                        continue;
                    }
                }
                inherit_async_references.push((chunk_item, InheritAsyncEdge::LocalModule));
                graph_nodes.push((
                    Some(module),
                    ChunkContentGraphNode::ChunkItem {
                        item: chunk_item,
                        ident: module.ident().to_string().await?,
                    },
                ));
            }
            ChunkingType::Async => {
                graph_nodes.push((
                    None,
                    ChunkContentGraphNode::AsyncModule {
                        module: chunkable_module,
                    },
                ));
            }
        }
    }

    if !inherit_async_references.is_empty() {
        if let Some(parent) = parent {
            graph_nodes.push((
                None,
                ChunkContentGraphNode::InheritAsyncInfo {
                    item: parent,
                    references: inherit_async_references,
                },
            ))
        }
    }

    Ok(graph_nodes)
}

struct ChunkContentVisit {
    chunk_content_context: ChunkContentContext,
    processed_modules: HashSet<Vc<Box<dyn Module>>>,
}

type ChunkItemToGraphNodesEdges =
    impl Iterator<Item = (Option<Vc<Box<dyn Module>>>, ChunkContentGraphNode)>;

type ChunkItemToGraphNodesFuture = impl Future<Output = Result<ChunkItemToGraphNodesEdges>>;

impl Visit<ChunkContentGraphNode, ()> for ChunkContentVisit {
    type Edge = (Option<Vc<Box<dyn Module>>>, ChunkContentGraphNode);
    type EdgesIntoIter = ChunkItemToGraphNodesEdges;
    type EdgesFuture = ChunkItemToGraphNodesFuture;

    fn visit(
        &mut self,
        (option_key, node): (Option<Vc<Box<dyn Module>>>, ChunkContentGraphNode),
    ) -> VisitControlFlow<ChunkContentGraphNode, ()> {
        let Some(module) = option_key else {
            return VisitControlFlow::Skip(node);
        };

        if !self.processed_modules.insert(module) {
            return VisitControlFlow::Skip(node);
        }

        VisitControlFlow::Continue(node)
    }

    fn edges(&mut self, node: &ChunkContentGraphNode) -> Self::EdgesFuture {
        let node = node.clone();

        let chunk_content_context = self.chunk_content_context;

        async move {
            let (references, parent) = match node {
                ChunkContentGraphNode::PassthroughModule { module } => (module.references(), None),
                ChunkContentGraphNode::ChunkItem { item, .. } => (item.references(), Some(item)),
                _ => {
                    return Ok(vec![].into_iter());
                }
            };

            Ok(references
                .await?
                .into_iter()
                .map(|reference| {
                    reference_to_graph_nodes(chunk_content_context, *reference, parent)
                })
                .try_flat_join()
                .await?
                .into_iter())
        }
    }

    fn span(&mut self, node: &ChunkContentGraphNode) -> Span {
        if let ChunkContentGraphNode::ChunkItem { ident, .. } = node {
            info_span!("module", name = display(ident))
        } else {
            Span::current()
        }
    }
}

async fn chunk_content_internal_parallel(
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    entries: impl IntoIterator<Item = Vc<Box<dyn Module>>>,
    availability_info: Value<AvailabilityInfo>,
) -> Result<ChunkContentResult> {
    let root_edges = entries
        .into_iter()
        .map(|entry| async move {
            let entry = entry.resolve().await?;
            let Some(chunkable_module) =
                Vc::try_resolve_downcast::<Box<dyn ChunkableModule>>(entry).await?
            else {
                return Ok(None);
            };
            Ok(Some((
                Some(entry),
                ChunkContentGraphNode::ChunkItem {
                    item: chunkable_module
                        .as_chunk_item(chunking_context)
                        .resolve()
                        .await?,
                    ident: chunkable_module.ident().to_string().await?,
                },
            )))
        })
        .try_flat_join()
        .await?;

    let chunk_content_context = ChunkContentContext {
        chunking_context,
        availability_info,
    };

    let visit = ChunkContentVisit {
        chunk_content_context,
        processed_modules: Default::default(),
    };

    let GraphTraversalResult::Completed(traversal_result) =
        AdjacencyMap::new().visit(root_edges, visit).await
    else {
        unreachable!();
    };

    let graph_nodes: Vec<_> = traversal_result?.into_reverse_topological().collect();

    let mut chunk_items = IndexSet::new();
    let mut async_modules = IndexSet::new();
    let mut external_module_references = IndexSet::new();
    let mut local_back_edges_inherit_async = IndexMap::new();
    let mut available_async_modules_back_edges_inherit_async = IndexMap::new();

    for graph_node in graph_nodes {
        match graph_node {
            ChunkContentGraphNode::PassthroughModule { .. } => {}
            ChunkContentGraphNode::ChunkItem { item, .. } => {
                chunk_items.insert(item);
            }
            ChunkContentGraphNode::AsyncModule { module } => {
                let module = module.resolve().await?;
                async_modules.insert(module);
            }
            ChunkContentGraphNode::ExternalModuleReference(reference) => {
                let reference = reference.resolve().await?;
                external_module_references.insert(reference);
            }
            ChunkContentGraphNode::InheritAsyncInfo { item, references } => {
                for &(reference, ty) in &references {
                    match ty {
                        InheritAsyncEdge::LocalModule => local_back_edges_inherit_async
                            .entry(reference)
                            .or_insert_with(Vec::new)
                            .push(item),
                        InheritAsyncEdge::AvailableAsyncModule => {
                            available_async_modules_back_edges_inherit_async
                                .entry(reference)
                                .or_insert_with(Vec::new)
                                .push(item)
                        }
                    }
                }
            }
        }
    }

    Ok(ChunkContentResult {
        chunk_items,
        async_modules,
        external_module_references,
        local_back_edges_inherit_async,
        available_async_modules_back_edges_inherit_async,
    })
}

#[turbo_tasks::value_trait]
pub trait ChunkItem {
    /// The [AssetIdent] of the [Module] that this [ChunkItem] was created from.
    /// For most chunk types this must uniquely identify the chunk item at
    /// runtime as it's the source of the module id used at runtime.
    fn asset_ident(self: Vc<Self>) -> Vc<AssetIdent>;
    /// A [AssetIdent] that uniquely identifies the content of this [ChunkItem].
    /// It is unusally identical to [ChunkItem::asset_ident] but can be
    /// different when the chunk item content depends on available modules e. g.
    /// for chunk loaders.
    fn content_ident(self: Vc<Self>) -> Vc<AssetIdent> {
        self.asset_ident()
    }
    /// A [ChunkItem] can describe different `references` than its original
    /// [Module].
    /// TODO(alexkirsz) This should have a default impl that returns empty
    /// references.
    fn references(self: Vc<Self>) -> Vc<ModuleReferences>;

    /// The type of chunk this item should be assembled into.
    fn ty(self: Vc<Self>) -> Vc<Box<dyn ChunkType>>;

    /// A temporary method to retrieve the module associated with this
    /// ChunkItem. TODO: Remove this as part of the chunk refactoring.
    fn module(self: Vc<Self>) -> Vc<Box<dyn Module>>;

    fn chunking_context(self: Vc<Self>) -> Vc<Box<dyn ChunkingContext>>;

    fn is_self_async(self: Vc<Self>) -> Vc<bool> {
        Vc::cell(false)
    }
}

#[turbo_tasks::value_trait]
pub trait ChunkType: ValueToString {
    /// Create a new chunk for the given chunk items
    fn chunk(
        &self,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
        chunk_items: Vc<ChunkItemsWithAsyncModuleInfo>,
        referenced_output_assets: Vc<OutputAssets>,
    ) -> Vc<Box<dyn Chunk>>;

    fn chunk_item_size(
        &self,
        chunking_context: Vc<Box<dyn ChunkingContext>>,
        chunk_item: Vc<Box<dyn ChunkItem>>,
        async_module_info: Option<Vc<AsyncModuleInfo>>,
    ) -> Vc<usize>;
}

#[turbo_tasks::value(transparent)]
pub struct ChunkItems(Vec<Vc<Box<dyn ChunkItem>>>);

#[turbo_tasks::value]
pub struct AsyncModuleInfo {
    pub referenced_async_modules: AutoSet<Vc<Box<dyn ChunkItem>>>,
}

#[turbo_tasks::value(transparent)]
pub struct ChunkItemsWithAsyncModuleInfo(
    Vec<(Vc<Box<dyn ChunkItem>>, Option<Vc<AsyncModuleInfo>>)>,
);

pub trait ChunkItemExt: Send {
    /// Returns the module id of this chunk item.
    fn id(self: Vc<Self>) -> Vc<ModuleId>;
}

impl<T> ChunkItemExt for T
where
    T: Upcast<Box<dyn ChunkItem>>,
{
    /// Returns the module id of this chunk item.
    fn id(self: Vc<Self>) -> Vc<ModuleId> {
        let chunk_item = Vc::upcast(self);
        chunk_item.chunking_context().chunk_item_id(chunk_item)
    }
}
