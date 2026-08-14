//! Read-only knowledge tools for the Tapp Playground agent.
//!
//! The model never receives a filesystem or shell tool. It can ask the agent
//! runner for documentation queries, and this module returns bounded excerpts
//! from the repository-owned Tapp contract.

use serde::Serialize;
use std::collections::HashSet;

const MAX_EXCERPT_CHARS: usize = 5_000;

struct KnowledgeDocument {
    id: &'static str,
    description: &'static str,
    content: &'static str,
}

const DOCUMENTS: &[KnowledgeDocument] = &[
    KnowledgeDocument {
        id: "TAPP_DEVELOPMENT",
        description: "Tapp development entry point and document routing",
        content: include_str!("../../../docs/development/TAPP_DEVELOPMENT.md"),
    },
    KnowledgeDocument {
        id: "QUICKSTART",
        description: "minimal application structure and first working Tapp",
        content: include_str!("../../../docs/development/tapp/QUICKSTART.md"),
    },
    KnowledgeDocument {
        id: "ARCHITECTURE",
        description: "core, page, widget, runtime, background, and ownership architecture",
        content: include_str!("../../../docs/development/tapp/ARCHITECTURE.md"),
    },
    KnowledgeDocument {
        id: "MANIFEST",
        description: "complete manifest fields including locales (host name/description i18n), permissions, settings, APIs, AI, events, and agent",
        content: include_str!("../../../docs/development/tapp/MANIFEST.md"),
    },
    KnowledgeDocument {
        id: "API_REFERENCE",
        description: "complete Tapp JavaScript SDK reference: storage, federation notes/media, permissions",
        content: include_str!("../../../docs/development/tapp/API_REFERENCE.md"),
    },
    KnowledgeDocument {
        id: "PAGE",
        description: "page templates, modules, lifecycle, layout, and page runtime",
        content: include_str!("../../../docs/development/tapp/PAGE.md"),
    },
    KnowledgeDocument {
        id: "WIDGET",
        description: "widget declarations, sizes, templates, settings, refresh, and runtime",
        content: include_str!("../../../docs/development/tapp/WIDGET.md"),
    },
    KnowledgeDocument {
        id: "SANDBOX",
        description: "iframe isolation, CSP, bridge payload limits, runtime grants, and security boundaries",
        content: include_str!("../../../docs/development/tapp/SANDBOX.md"),
    },
    KnowledgeDocument {
        id: "STYLING",
        description: "CSS modes, host tokens, responsive design, themes, and component styles",
        content: include_str!("../../../docs/development/tapp/STYLING.md"),
    },
    KnowledgeDocument {
        id: "GRAPHICS",
        description: "graphics, canvas, animation, and rendering guidance",
        content: include_str!("../../../docs/development/tapp/GRAPHICS.md"),
    },
    KnowledgeDocument {
        id: "REST_API",
        description: "Tapp installation, storage, settings, runtime, and host REST contracts",
        content: include_str!("../../../docs/development/tapp/REST_API.md"),
    },
    KnowledgeDocument {
        id: "RUNTIME_CONTRACT_DESIGN",
        description: "AI tasks, events, agent interactions, data exchange, scheduler, and runtime contracts",
        content: include_str!("../../../docs/development/tapp/RUNTIME_CONTRACT_DESIGN.md"),
    },
    KnowledgeDocument {
        id: "TROUBLESHOOTING",
        description: "common runtime, permission, federation media, storeSource, and installation failures",
        content: include_str!("../../../docs/development/tapp/TROUBLESHOOTING.md"),
    },
    KnowledgeDocument {
        id: "STORE",
        description: "remote Tapp store catalog index.json, catalog locales for long_description/preview, storeSource vs SDK install shapes, assets path rules, publish checklist",
        content: include_str!("../../../docs/development/tapp/STORE.md"),
    },
    KnowledgeDocument {
        id: "TAPP_FILE_FORMAT",
        description: "packaged .tapp archive structure and resource rules",
        content: include_str!("../../../docs/features/TAPP_FILE_FORMAT.md"),
    },
    KnowledgeDocument {
        id: "PLAYGROUND_GENERATION_CONTEXT",
        description: "safe temporary-preview contract; manifest.locales vs code.i18n; federation install-only",
        content: include_str!("../../../docs/development/tapp/PLAYGROUND_GENERATION_CONTEXT.md"),
    },
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExcerpt {
    pub document: String,
    pub section: String,
    pub excerpt: String,
}

#[derive(Debug)]
struct ScoredSection {
    score: usize,
    document: &'static str,
    section: String,
    body: String,
}

pub fn catalog_for_prompt() -> String {
    DOCUMENTS
        .iter()
        .map(|document| format!("- {}: {}", document.id, document.description))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn search(query: &str, limit: usize) -> Vec<KnowledgeExcerpt> {
    let query = expand_query_aliases(query);
    let terms = query_terms(&query);
    if terms.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut scored = Vec::new();
    for document in DOCUMENTS {
        for (section, body) in split_sections(document.content) {
            let heading_lower = section.to_lowercase();
            let body_lower = body.to_lowercase();
            let document_lower = document.id.to_lowercase();
            let mut score = 0usize;
            for term in &terms {
                if heading_lower.contains(term) {
                    score += 12;
                }
                if document_lower.contains(term) {
                    score += 8;
                }
                score += body_lower.matches(term).count().min(8);
            }
            if score > 0 {
                scored.push(ScoredSection {
                    score,
                    document: document.id,
                    section,
                    body,
                });
            }
        }
    }

    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.document.cmp(right.document))
            .then_with(|| left.section.cmp(&right.section))
    });

    let mut seen = HashSet::new();
    scored
        .into_iter()
        .filter(|item| seen.insert((item.document, item.section.clone())))
        .take(limit.min(8))
        .map(|item| KnowledgeExcerpt {
            document: item.document.to_string(),
            section: item.section,
            excerpt: truncate_chars(&item.body, MAX_EXCERPT_CHARS),
        })
        .collect()
}

fn split_sections(content: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut heading = "Overview".to_string();
    let mut body = String::new();

    for line in content.lines() {
        if let Some(next_heading) = line.strip_prefix("## ") {
            if !body.trim().is_empty() {
                sections.push((heading, body.trim().to_string()));
            }
            heading = next_heading.trim().to_string();
            body.clear();
        } else if let Some(next_heading) = line.strip_prefix("### ") {
            if !body.trim().is_empty() {
                sections.push((heading, body.trim().to_string()));
            }
            heading = next_heading.trim().to_string();
            body.clear();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !body.trim().is_empty() {
        sections.push((heading, body.trim().to_string()));
    }
    sections
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|character: char| {
            !character.is_alphanumeric() && character != ':' && character != '_'
        })
        .filter(|term| term.chars().count() >= 2)
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn expand_query_aliases(query: &str) -> String {
    let aliases = [
        ("小组件", " widget "),
        ("组件", " widget "),
        ("页面", " page "),
        ("权限", " permission manifest "),
        ("存储", " storage settings "),
        ("设置", " settings "),
        ("主题", " theme styling "),
        ("样式", " css styling "),
        ("后台", " background core scheduler "),
        ("定时", " scheduler "),
        ("事件", " events "),
        ("智能体", " agent interaction "),
        ("代理", " agent interaction "),
        ("数据交换", " data exchange "),
        ("接口", " api "),
        ("网络", " api network fetch "),
        ("图形", " graphics canvas "),
        ("动画", " animation graphics "),
        ("安装", " install package manifest "),
        (
            "联邦",
            " federation publish media note uploadMedia createNote ",
        ),
        (
            "federation",
            " federation publish media note uploadMedia createNote ",
        ),
        (
            "多语言",
            " locales i18n name description manifest store catalog long_description preview ",
        ),
        (
            "locales",
            " locales name description manifest store catalog long_description preview en-US ja-JP ",
        ),
        ("长介绍", " long_description locales store catalog preview "),
        ("标题", " locales name description manifest "),
        ("商店", " store install locales catalog preview package manifest "),
    ];
    aliases
        .iter()
        .fold(query.to_string(), |expanded, (from, to)| {
            expanded.replace(from, to)
        })
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        let mut truncated = value.chars().take(max_chars).collect::<String>();
        truncated.push_str("\n[excerpt truncated]");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog_for_prompt, search};

    #[test]
    fn catalog_covers_the_full_tapp_contract() {
        let catalog = catalog_for_prompt();
        for document in [
            "MANIFEST",
            "API_REFERENCE",
            "WIDGET",
            "SANDBOX",
            "STORE",
            "RUNTIME_CONTRACT_DESIGN",
            "TAPP_FILE_FORMAT",
            "PLAYGROUND_GENERATION_CONTEXT",
        ] {
            assert!(
                catalog.contains(document),
                "catalog missing {document}: {catalog}"
            );
        }
    }

    #[test]
    fn store_source_query_hits_store_or_api_or_troubleshooting() {
        let results = search("storeSource index.json catalog install store", 5);
        assert!(!results.is_empty());
        assert!(results.iter().any(|result| {
            result.document == "STORE"
                || result.document == "API_REFERENCE"
                || result.document == "TROUBLESHOOTING"
                || result.document == "REST_API"
        }));
    }

    #[test]
    fn chinese_widget_query_retrieves_widget_document() {
        let results = search("小组件尺寸和设置", 5);
        assert!(!results.is_empty());
        assert!(results.iter().any(|result| result.document == "WIDGET"));
    }

    #[test]
    fn result_count_is_bounded() {
        assert!(search("tapp api storage widget page manifest", 3).len() <= 3);
    }

    #[test]
    fn federation_media_query_hits_api_or_sandbox() {
        let results = search("federation uploadMedia createNote payload", 5);
        assert!(!results.is_empty());
        assert!(results.iter().any(|result| {
            result.document == "API_REFERENCE"
                || result.document == "SANDBOX"
                || result.document == "TROUBLESHOOTING"
                || result.document == "PLAYGROUND_GENERATION_CONTEXT"
        }));
    }

    #[test]
    fn locales_queries_hit_manifest_or_generation_context() {
        for query in [
            "locales",
            "多语言",
            "manifest locales en-US name description",
        ] {
            let results = search(query, 5);
            assert!(
                !results.is_empty(),
                "expected knowledge hits for query {query:?}"
            );
            assert!(
                results.iter().any(|result| {
                    result.document == "MANIFEST"
                        || result.document == "PLAYGROUND_GENERATION_CONTEXT"
                        || result.document == "QUICKSTART"
                        || result.document == "API_REFERENCE"
                }),
                "locales-related docs missing for query {query:?}: {:?}",
                results
                    .iter()
                    .map(|r| r.document.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
}
