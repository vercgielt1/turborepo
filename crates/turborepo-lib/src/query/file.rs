use std::sync::Arc;

use async_graphql::{Object, SimpleObject};
use itertools::Itertools;
use turbo_trace::Tracer;
use turbopath::AbsoluteSystemPathBuf;

use crate::{
    query::{Array, Error},
    run::Run,
};

pub struct File {
    run: Arc<Run>,
    path: AbsoluteSystemPathBuf,
    ast: Option<swc_ecma_ast::Module>,
}

impl File {
    pub fn new(run: Arc<Run>, path: AbsoluteSystemPathBuf) -> Self {
        Self {
            run,
            path,
            ast: None,
        }
    }

    pub fn with_ast(mut self, ast: Option<swc_ecma_ast::Module>) -> Self {
        self.ast = ast;

        self
    }
}

#[derive(SimpleObject, Debug)]
pub struct TraceError {
    message: String,
    path: Option<String>,
    start: Option<usize>,
    end: Option<usize>,
}

impl From<turbo_trace::TraceError> for TraceError {
    fn from(error: turbo_trace::TraceError) -> Self {
        let message = error.to_string();
        match error {
            turbo_trace::TraceError::FileNotFound(file) => TraceError {
                message,
                path: Some(file.to_string()),
                start: None,
                end: None,
            },
            turbo_trace::TraceError::PathEncoding(_) => TraceError {
                message,
                path: None,
                start: None,
                end: None,
            },
            turbo_trace::TraceError::RootFile(path) => TraceError {
                message,
                path: Some(path.to_string()),
                start: None,
                end: None,
            },
            turbo_trace::TraceError::Resolve { span, text } => TraceError {
                message,
                path: Some(text.name().to_string()),
                start: Some(span.offset()),
                end: Some(span.offset() + span.len()),
            },
        }
    }
}

#[derive(SimpleObject)]
struct TraceResult {
    files: Array<File>,
    errors: Array<TraceError>,
}

impl TraceResult {
    fn new(result: turbo_trace::TraceResult, run: Arc<Run>) -> Self {
        Self {
            files: result
                .files
                .into_iter()
                .sorted_by(|a, b| a.0.cmp(&b.0))
                .map(|(path, file)| File::new(run.clone(), path).with_ast(file.ast))
                .collect(),
            errors: result.errors.into_iter().map(|e| e.into()).collect(),
        }
    }
}

#[Object]
impl File {
    async fn contents(&self) -> Result<String, Error> {
        let contents = self.path.read_to_string()?;
        Ok(contents)
    }

    async fn path(&self) -> Result<String, Error> {
        Ok(self
            .run
            .repo_root()
            .anchor(&self.path)
            .map(|path| path.to_string())?)
    }

    async fn absolute_path(&self) -> Result<String, Error> {
        Ok(self.path.to_string())
    }

    async fn dependencies(&self, depth: Option<usize>) -> TraceResult {
        let tracer = Tracer::new(
            self.run.repo_root().to_owned(),
            vec![self.path.clone()],
            None,
        );

        let mut result = tracer.trace(depth);
        // Remove the file itself from the result
        result.files.remove(&self.path);
        TraceResult::new(result, self.run.clone())
    }

    async fn ast(&self) -> Option<serde_json::Value> {
        let ast = serde_json::to_value(&self.ast).ok()?;
        Some(ast)
    }
}
