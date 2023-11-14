use anyhow::Result;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileSystemPath;

use super::{Issue, IssueSeverity, StyledString};

#[turbo_tasks::value(shared)]
pub struct UnsupportedModuleIssue {
    pub file_path: Vc<FileSystemPath>,
    pub package: String,
    pub package_path: Option<String>,
}

#[turbo_tasks::value_impl]
impl Issue for UnsupportedModuleIssue {
    #[turbo_tasks::function]
    fn severity(&self) -> Vc<IssueSeverity> {
        IssueSeverity::Warning.into()
    }

    #[turbo_tasks::function]
    fn category(&self) -> Vc<String> {
        Vc::cell("resolve".to_string())
    }

    #[turbo_tasks::function]
    fn title(&self) -> Vc<String> {
        Vc::cell("Unsupported module".into())
    }

    #[turbo_tasks::function]
    fn file_path(&self) -> Vc<FileSystemPath> {
        self.file_path
    }

    #[turbo_tasks::function]
    async fn description(&self) -> Result<Vc<StyledString>> {
        Ok(StyledString::Text(match &self.package_path {
            Some(path) => format!("The module {}{} is not yet supported", self.package, path),
            None => format!("The package {} is not yet supported", self.package),
        })
        .cell())
    }
}
