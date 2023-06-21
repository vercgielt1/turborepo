use std::{
    collections::btree_map::Entry,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use hyper::{
    header::{HeaderName as HyperHeaderName, HeaderValue as HyperHeaderValue},
    Uri,
};
use turbo_tasks::{TransientInstance, Value};

use super::{
    headers::{HeaderValue, Headers},
    query::Query,
    request::SourceRequest,
    ContentSourceContent, ContentSourceDataVary, ContentSourceVc, HeaderListVc, ProxyResultVc,
    RewriteType, StaticContentVc,
};
use crate::source::{ContentSource, ContentSourceData, GetContentSourceContent};

/// The result of [`resolve_source_request`]. Similar to a
/// `ContentSourceContent`, but without the `Rewrite` variant as this is taken
/// care in the function.
#[turbo_tasks::value(serialization = "none")]
pub enum ResolveSourceRequestResult {
    NotFound,
    Static(StaticContentVc, HeaderListVc),
    HttpProxy(ProxyResultVc),
}

/// Resolves a [SourceRequest] within a [super::ContentSource], returning the
/// corresponding content.
#[turbo_tasks::function]
pub async fn resolve_source_request(
    source: ContentSourceVc,
    request: TransientInstance<SourceRequest>,
) -> Result<ResolveSourceRequestResultVc> {
    let original_path = request.uri.path().to_string();
    // Remove leading slash.
    let mut current_asset_path = urlencoding::decode(&original_path[1..])?.into_owned();
    let mut request_overwrites = (*request).clone();
    let mut response_header_overwrites = Vec::new();
    let route_tree = source.get_routes();
    'routes: loop {
        let mut sources = route_tree.get(&current_asset_path);
        'sources: loop {
            for get_content in sources.strongly_consistent().await?.iter() {
                let content_vary = get_content.vary().await?;
                let content_data =
                    request_to_data(&request_overwrites, &request, &content_vary).await?;
                let content = get_content.get(&current_asset_path, Value::new(content_data));
                match &*content.await? {
                    ContentSourceContent::Rewrite(rewrite) => {
                        let rewrite = rewrite.await?;
                        // apply rewrite extras
                        if let Some(headers) = &rewrite.response_headers {
                            response_header_overwrites.extend(headers.await?.iter().cloned());
                        }
                        if let Some(headers) = &rewrite.request_headers {
                            request_overwrites.headers.clear();
                            for (name, value) in &*headers.await? {
                                request_overwrites.headers.insert(
                                    HyperHeaderName::try_from(name)?,
                                    HyperHeaderValue::try_from(value)?,
                                );
                            }
                        }
                        // do the rewrite
                        match &rewrite.ty {
                            RewriteType::Location { path_and_query } => {
                                let new_uri = Uri::try_from(path_and_query)?;
                                let new_asset_path =
                                    urlencoding::decode(&new_uri.path()[1..])?.into_owned();
                                request_overwrites.uri = new_uri;
                                current_asset_path = new_asset_path;
                                continue 'routes;
                            }
                            RewriteType::Sources {
                                sources: new_sources,
                            } => {
                                sources = *new_sources;
                                continue 'sources;
                            }
                        }
                    }
                    ContentSourceContent::NotFound => {
                        return Ok(ResolveSourceRequestResult::NotFound.cell());
                    }
                    ContentSourceContent::Static(static_content) => {
                        return Ok(ResolveSourceRequestResult::Static(
                            *static_content,
                            HeaderListVc::new(response_header_overwrites),
                        )
                        .cell());
                    }
                    ContentSourceContent::HttpProxy(proxy_result) => {
                        return Ok(ResolveSourceRequestResult::HttpProxy(*proxy_result).cell());
                    }
                }
            }
        }
    }
}

static CACHE_BUSTER: AtomicU64 = AtomicU64::new(0);

async fn request_to_data(
    request: &SourceRequest,
    original_request: &SourceRequest,
    vary: &ContentSourceDataVary,
) -> Result<ContentSourceData> {
    let mut data = ContentSourceData::default();
    if vary.method {
        data.method = Some(request.method.clone());
    }
    if vary.url {
        data.url = Some(request.uri.to_string());
    }
    if vary.original_url {
        data.original_url = Some(original_request.uri.to_string());
    }
    if vary.body {
        data.body = Some(request.body.clone().into());
    }
    if vary.raw_query {
        data.raw_query = Some(request.uri.query().unwrap_or("").to_string());
    }
    if vary.raw_headers {
        data.raw_headers = Some(
            request
                .headers
                .iter()
                .map(|(name, value)| Ok((name.to_string(), value.to_str()?.to_string())))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    if let Some(filter) = vary.query.as_ref() {
        if let Some(query) = request.uri.query() {
            let mut query: Query = serde_qs::from_str(query)?;
            query.filter_with(filter);
            data.query = Some(query);
        } else {
            data.query = Some(Query::default())
        }
    }
    if let Some(filter) = vary.headers.as_ref() {
        let mut headers = Headers::default();
        for (header_name, header_value) in request.headers.iter() {
            if !filter.contains(header_name.as_str()) {
                continue;
            }
            match headers.entry(header_name.to_string()) {
                Entry::Vacant(e) => {
                    if let Ok(s) = header_value.to_str() {
                        e.insert(HeaderValue::SingleString(s.to_string()));
                    } else {
                        e.insert(HeaderValue::SingleBytes(header_value.as_bytes().to_vec()));
                    }
                }
                Entry::Occupied(mut e) => {
                    if let Ok(s) = header_value.to_str() {
                        e.get_mut().extend_with_string(s.to_string());
                    } else {
                        e.get_mut()
                            .extend_with_bytes(header_value.as_bytes().to_vec());
                    }
                }
            }
        }
        data.headers = Some(headers);
    }
    if vary.cache_buster {
        data.cache_buster = CACHE_BUSTER.fetch_add(1, Ordering::SeqCst);
    }
    Ok(data)
}
