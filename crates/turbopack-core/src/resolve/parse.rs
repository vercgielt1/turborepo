use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use turbo_tasks::{TryJoinIterExt, Value, ValueToString, Vc};

use super::pattern::Pattern;

#[turbo_tasks::value(shared)]
#[derive(Hash, Clone, Debug)]
pub enum Request {
    Raw {
        path: Pattern,
        query: Vc<String>,
        force_in_lookup_dir: bool,
        fragment: Vc<String>,
    },
    Relative {
        path: Pattern,
        query: Vc<String>,
        force_in_lookup_dir: bool,
        fragment: Vc<String>,
    },
    Module {
        module: String,
        path: Pattern,
        query: Vc<String>,
        fragment: Vc<String>,
    },
    ServerRelative {
        path: Pattern,
        query: Vc<String>,
        fragment: Vc<String>,
    },
    Windows {
        path: Pattern,
        query: Vc<String>,
        fragment: Vc<String>,
    },
    Empty,
    PackageInternal {
        path: Pattern,
    },
    Uri {
        protocol: String,
        remainder: String,
        query: Vc<String>,
        fragment: Vc<String>,
    },
    Unknown {
        path: Pattern,
    },
    Dynamic,
    Alternatives {
        requests: Vec<Vc<Request>>,
    },
}

fn split_off_query_fragment(raw: String) -> (Pattern, Vc<String>, Vc<String>) {
    let Some((raw, query)) = raw.split_once('?') else {
        if let Some((raw, fragment)) = raw.split_once('#') {
            return (
                Pattern::Constant(raw.to_string()),
                Vc::<String>::default(),
                Vc::cell(fragment.to_string()),
            );
        }

        return (
            Pattern::Constant(raw),
            Vc::<String>::default(),
            Vc::<String>::default(),
        );
    };

    let (query, fragment) = query.split_once('#').unwrap_or((query, ""));

    (
        Pattern::Constant(raw.to_string()),
        Vc::cell(format!("?{}", query)),
        Vc::cell(format!("#{}", fragment)),
    )
}

impl Request {
    pub fn request(&self) -> Option<String> {
        Some(match self {
            Request::Raw {
                path: Pattern::Constant(path),
                ..
            } => path.to_string(),
            Request::Relative {
                path: Pattern::Constant(path),
                ..
            } => path.to_string(),
            Request::Module {
                module,
                path: Pattern::Constant(path),
                ..
            } => format!("{module}{path}"),
            Request::ServerRelative {
                path: Pattern::Constant(path),
                ..
            } => path.to_string(),
            Request::Windows {
                path: Pattern::Constant(path),
                ..
            } => path.to_string(),
            Request::Empty => "".to_string(),
            Request::PackageInternal {
                path: Pattern::Constant(path),
                ..
            } => path.to_string(),
            Request::Uri {
                protocol,
                remainder,
                ..
            } => format!("{protocol}{remainder}"),
            Request::Unknown {
                path: Pattern::Constant(path),
            } => path.to_string(),
            _ => return None,
        })
    }

    pub fn parse_ref(mut request: Pattern) -> Self {
        request.normalize();
        match request {
            Pattern::Dynamic => Request::Dynamic,
            Pattern::Constant(r) => {
                if r.is_empty() {
                    Request::Empty
                } else if r.starts_with('/') {
                    let (path, query, fragment) = split_off_query_fragment(r);

                    Request::ServerRelative {
                        path,
                        query,
                        fragment,
                    }
                } else if r.starts_with('#') {
                    Request::PackageInternal {
                        path: Pattern::Constant(r),
                    }
                } else if r.starts_with("./") || r.starts_with("../") || r == "." || r == ".." {
                    let (path, query, fragment) = split_off_query_fragment(r);

                    Request::Relative {
                        path,
                        force_in_lookup_dir: false,
                        query,
                        fragment,
                    }
                } else {
                    lazy_static! {
                        static ref WINDOWS_PATH: Regex = Regex::new(r"^[A-Za-z]:\\|\\\\").unwrap();
                        static ref URI_PATH: Regex = Regex::new(r"^([^/\\]+:)(.+)$").unwrap();
                        static ref MODULE_PATH: Regex =
                            Regex::new(r"^((?:@[^/]+/)?[^/]+)(.*)$").unwrap();
                    }

                    if WINDOWS_PATH.is_match(&r) {
                        let (path, query, fragment) = split_off_query_fragment(r);

                        return Request::Windows {
                            path,
                            query,
                            fragment,
                        };
                    }

                    if let Some(caps) = URI_PATH.captures(&r) {
                        if let (Some(protocol), Some(remainder)) = (caps.get(1), caps.get(2)) {
                            // TODO data uri
                            return Request::Uri {
                                protocol: protocol.as_str().to_string(),
                                remainder: remainder.as_str().to_string(),
                                query: Vc::<String>::default(),
                                fragment: Vc::<String>::default(),
                            };
                        }
                    }

                    if let Some((module, path)) = MODULE_PATH
                        .captures(&r)
                        .and_then(|caps| caps.get(1).zip(caps.get(2)))
                    {
                        let (path, query, fragment) =
                            split_off_query_fragment(path.as_str().to_string());

                        return Request::Module {
                            module: module.as_str().to_string(),
                            path,
                            query,
                            fragment,
                        };
                    }

                    Request::Unknown {
                        path: Pattern::Constant(r),
                    }
                }
            }
            Pattern::Concatenation(list) => {
                let mut iter = list.into_iter();
                if let Some(first) = iter.next() {
                    let mut result = Self::parse_ref(first);
                    match &mut result {
                        Request::Raw { path, .. } => {
                            path.extend(iter);
                        }
                        Request::Relative { path, .. } => {
                            path.extend(iter);
                        }
                        Request::Module { path, .. } => {
                            path.extend(iter);
                        }
                        Request::ServerRelative { path, .. } => {
                            path.extend(iter);
                        }
                        Request::Windows { path, .. } => {
                            path.extend(iter);
                        }
                        Request::Empty => {
                            result = Request::parse_ref(Pattern::Concatenation(iter.collect()))
                        }
                        Request::PackageInternal { path } => {
                            path.extend(iter);
                        }
                        Request::Uri { .. } => {
                            result = Request::Dynamic;
                        }
                        Request::Unknown { path } => {
                            path.extend(iter);
                        }
                        Request::Dynamic => {}
                        Request::Alternatives { .. } => unreachable!(),
                    };
                    result
                } else {
                    Request::Empty
                }
            }
            Pattern::Alternatives(list) => Request::Alternatives {
                requests: list
                    .into_iter()
                    .map(Value::new)
                    .map(Request::parse)
                    .collect(),
            },
        }
    }
}

#[turbo_tasks::value_impl]
impl Request {
    #[turbo_tasks::function]
    pub fn parse(request: Value<Pattern>) -> Vc<Self> {
        Self::cell(Request::parse_ref(request.into_value()))
    }

    #[turbo_tasks::function]
    pub fn parse_string(request: String) -> Vc<Self> {
        Self::cell(Request::parse_ref(request.into()))
    }

    #[turbo_tasks::function]
    pub fn raw(
        request: Value<Pattern>,
        query: Vc<String>,
        fragment: Vc<String>,
        force_in_lookup_dir: bool,
    ) -> Vc<Self> {
        Self::cell(Request::Raw {
            path: request.into_value(),
            force_in_lookup_dir,
            query,
            fragment,
        })
    }

    #[turbo_tasks::function]
    pub fn relative(
        request: Value<Pattern>,
        query: Vc<String>,
        fragment: Vc<String>,
        force_in_lookup_dir: bool,
    ) -> Vc<Self> {
        Self::cell(Request::Relative {
            path: request.into_value(),
            force_in_lookup_dir,
            query,
            fragment,
        })
    }

    #[turbo_tasks::function]
    pub fn module(
        module: String,
        path: Value<Pattern>,
        query: Vc<String>,
        fragment: Vc<String>,
    ) -> Vc<Self> {
        Self::cell(Request::Module {
            module,
            path: path.into_value(),
            query,
            fragment,
        })
    }

    #[turbo_tasks::function]
    pub async fn as_relative(self: Vc<Self>) -> Result<Vc<Self>> {
        let this = self.await?;
        Ok(match &*this {
            Request::Empty
            | Request::Raw { .. }
            | Request::ServerRelative { .. }
            | Request::Windows { .. }
            | Request::Relative { .. }
            | Request::Uri { .. }
            | Request::Dynamic => self,
            Request::Module {
                module,
                path,
                query: _,
                fragment: _,
            } => {
                let mut pat = Pattern::Constant(format!("./{module}"));
                pat.push(path.clone());
                // TODO add query
                Self::parse(Value::new(pat))
            }
            Request::PackageInternal { path } => {
                let mut pat = Pattern::Constant("./".to_string());
                pat.push(path.clone());
                Self::parse(Value::new(pat))
            }
            Request::Unknown { path } => {
                let mut pat = Pattern::Constant("./".to_string());
                pat.push(path.clone());
                Self::parse(Value::new(pat))
            }
            Request::Alternatives { requests } => {
                let requests = requests.iter().copied().map(Request::as_relative).collect();
                Request::Alternatives { requests }.cell()
            }
        })
    }

    #[turbo_tasks::function]
    pub async fn with_query(self: Vc<Self>, query: Vc<String>) -> Result<Vc<Self>> {
        Ok(match &*self.await? {
            Request::Raw {
                path,
                query: _,
                force_in_lookup_dir,
                fragment,
            } => Request::Raw {
                path: path.clone(),
                query,
                force_in_lookup_dir: *force_in_lookup_dir,
                fragment: *fragment,
            }
            .cell(),
            Request::Relative {
                path,
                query: _,
                force_in_lookup_dir,
                fragment,
            } => Request::Relative {
                path: path.clone(),
                query,
                force_in_lookup_dir: *force_in_lookup_dir,
                fragment: *fragment,
            }
            .cell(),
            Request::Module {
                module,
                path,
                query: _,
                fragment,
            } => Request::Module {
                module: module.clone(),
                path: path.clone(),
                query,
                fragment: *fragment,
            }
            .cell(),
            Request::ServerRelative {
                path,
                query: _,
                fragment,
            } => Request::ServerRelative {
                path: path.clone(),
                query,
                fragment: *fragment,
            }
            .cell(),
            Request::Windows {
                path,
                query: _,
                fragment,
            } => Request::Windows {
                path: path.clone(),
                query,
                fragment: *fragment,
            }
            .cell(),
            Request::Empty => self,
            Request::PackageInternal { .. } => self,
            Request::Uri { .. } => self,
            Request::Unknown { .. } => self,
            Request::Dynamic => self,
            Request::Alternatives { requests } => {
                let requests = requests
                    .iter()
                    .copied()
                    .map(|req| req.with_query(query))
                    .collect();
                Request::Alternatives { requests }.cell()
            }
        })
    }

    #[turbo_tasks::function]
    pub async fn with_fragment(self: Vc<Self>, fragment: Vc<String>) -> Result<Vc<Self>> {
        Ok(match &*self.await? {
            Request::Raw {
                path,
                query,
                force_in_lookup_dir,
                fragment: _,
            } => Request::Raw {
                path: path.clone(),
                query: *query,
                force_in_lookup_dir: *force_in_lookup_dir,
                fragment,
            }
            .cell(),
            Request::Relative {
                path,
                query,
                force_in_lookup_dir,
                fragment: _,
            } => Request::Relative {
                path: path.clone(),
                query: *query,
                force_in_lookup_dir: *force_in_lookup_dir,
                fragment,
            }
            .cell(),
            Request::Module {
                module,
                path,
                query,
                fragment: _,
            } => Request::Module {
                module: module.clone(),
                path: path.clone(),
                query: *query,
                fragment,
            }
            .cell(),
            Request::ServerRelative {
                path,
                query,
                fragment: _,
            } => Request::ServerRelative {
                path: path.clone(),
                query: *query,
                fragment,
            }
            .cell(),
            Request::Windows {
                path,
                query,
                fragment: _,
            } => Request::Windows {
                path: path.clone(),
                query: *query,
                fragment,
            }
            .cell(),
            Request::Empty => self,
            Request::PackageInternal { .. } => self,
            Request::Uri { .. } => self,
            Request::Unknown { .. } => self,
            Request::Dynamic => self,
            Request::Alternatives { requests } => {
                let requests = requests
                    .iter()
                    .copied()
                    .map(|req| req.with_fragment(fragment))
                    .collect();
                Request::Alternatives { requests }.cell()
            }
        })
    }

    #[turbo_tasks::function]
    pub async fn append_path(self: Vc<Self>, suffix: String) -> Result<Vc<Self>> {
        Ok(match &*self.await? {
            Request::Raw {
                path,
                query,
                force_in_lookup_dir,
                fragment,
            } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::raw(Value::new(pat), *query, *fragment, *force_in_lookup_dir)
            }
            Request::Relative {
                path,
                query,
                force_in_lookup_dir,
                fragment,
            } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::relative(Value::new(pat), *query, *fragment, *force_in_lookup_dir)
            }
            Request::Module {
                module,
                path,
                query,
                fragment,
            } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::module(module.clone(), Value::new(pat), *query, *fragment)
            }
            Request::ServerRelative {
                path,
                query,
                fragment,
            } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::ServerRelative {
                    path: pat,
                    query: *query,
                    fragment: *fragment,
                }
                .cell()
            }
            Request::Windows {
                path,
                query,
                fragment,
            } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::Windows {
                    path: pat,
                    query: *query,
                    fragment: *fragment,
                }
                .cell()
            }
            Request::Empty => Self::parse(Value::new(suffix.into())),
            Request::PackageInternal { path } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::PackageInternal { path: pat }.cell()
            }
            Request::Uri {
                protocol,
                remainder,
                query,
                fragment,
            } => {
                let remainder = format!("{}{}", remainder, suffix);
                Self::Uri {
                    protocol: protocol.clone(),
                    remainder,
                    query: *query,
                    fragment: *fragment,
                }
                .cell()
            }
            Request::Unknown { path } => {
                let mut pat = Pattern::concat([path.clone(), suffix.into()]);
                pat.normalize();
                Self::Unknown { path: pat }.cell()
            }
            Request::Dynamic => self,
            Request::Alternatives { requests } => {
                let requests = requests
                    .iter()
                    .map(|req| req.append_path(suffix.clone()))
                    .collect();
                Request::Alternatives { requests }.cell()
            }
        })
    }

    #[turbo_tasks::function]
    pub fn query(&self) -> Vc<String> {
        match self {
            Request::Raw { query, .. } => *query,
            Request::Relative { query, .. } => *query,
            Request::Module { query, .. } => *query,
            Request::ServerRelative { query, .. } => *query,
            Request::Windows { query, .. } => *query,
            Request::Empty => Vc::<String>::default(),
            Request::PackageInternal { .. } => Vc::<String>::default(),
            Request::Uri { .. } => Vc::<String>::default(),
            Request::Unknown { .. } => Vc::<String>::default(),
            Request::Dynamic => Vc::<String>::default(),
            // TODO: is this correct, should we return the first one instead?
            Request::Alternatives { .. } => Vc::<String>::default(),
        }
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for Request {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(match self {
            Request::Raw {
                path,
                force_in_lookup_dir,
                ..
            } => {
                if *force_in_lookup_dir {
                    format!("in-lookup-dir {path}")
                } else {
                    format!("{path}")
                }
            }
            Request::Relative {
                path,
                force_in_lookup_dir,
                ..
            } => {
                if *force_in_lookup_dir {
                    format!("relative-in-lookup-dir {path}")
                } else {
                    format!("relative {path}")
                }
            }
            Request::Module { module, path, .. } => {
                if path.could_match_others("") {
                    format!("module \"{module}\" with subpath {path}")
                } else {
                    format!("module \"{module}\"")
                }
            }
            Request::ServerRelative { path, .. } => format!("server relative {path}"),
            Request::Windows { path, .. } => format!("windows {path}"),
            Request::Empty => "empty".to_string(),
            Request::PackageInternal { path } => format!("package internal {path}"),
            Request::Uri {
                protocol,
                remainder,
                ..
            } => format!("uri \"{protocol}\" \"{remainder}\""),
            Request::Unknown { path } => format!("unknown {path}"),
            Request::Dynamic => "dynamic".to_string(),
            Request::Alternatives { requests } => {
                let vec = requests.iter().map(|i| i.to_string()).try_join().await?;
                vec.iter()
                    .map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(" or ")
            }
        }))
    }
}
