//! AI models and datasets cleanup (PLAN 2.14a and 2.14b).
//!
//! Offers fine-grained (per-model) cleanup for Hugging Face, LM Studio, and Ollama,
//! while keeping login tokens, keys, conversation history and settings strictly protected.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub const PROTECTED: &[&str] = &[
    ".cache/huggingface/token",
    ".cache/huggingface/stored_tokens",
    ".ollama/id_ed25519",
    ".ollama/id_ed25519.pub",
    ".lmstudio/conversations",
    ".lmstudio/.internal",
    ".lmstudio/settings.json",
];

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("ai") {
        return;
    }

    collect_huggingface_hub(&env.home, out);
    collect_huggingface_extra(&env.home, out);
    collect_lm_studio(&env.home, out);
    collect_ollama(&env.home, out);
}

fn collect_huggingface_hub(home: &Path, out: &mut Vec<Candidate>) {
    let hub_dir = home.join(".cache/huggingface/hub");
    let Ok(entries) = fs::read_dir(hub_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !super::is_real_dir(&path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let (kind_prefix, file_type, kind_word) = if name.starts_with("models--") {
            ("models--", "Hugging Face Models", "model")
        } else if name.starts_with("datasets--") {
            ("datasets--", "Hugging Face Datasets", "dataset")
        } else if name.starts_with("spaces--") {
            ("spaces--", "Hugging Face Spaces", "space")
        } else {
            continue;
        };

        let rest = match name.strip_prefix(kind_prefix) {
            Some(r) => r,
            None => continue,
        };
        let decoded = rest.replace("--", "/");

        let description = format!(
            "{decoded}: {kind_word} downloaded from the Hugging Face Hub. Downloaded again when a script needs it, which can take a long time for large models. Models you uploaded or trained locally only exist here."
        );

        out.push(Candidate {
            path,
            file_type: file_type.to_string(),
            category: "AI Model Cache".to_string(),
            safety: Safety::Review,
            description,
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            group: "AI Tools",
            catch_all: false,
        });
    }
}

fn collect_huggingface_extra(home: &Path, out: &mut Vec<Candidate>) {
    let datasets_dir = home.join(".cache/huggingface/datasets");
    if super::is_real_dir(&datasets_dir) {
        out.push(Candidate {
            path: datasets_dir,
            file_type: "Hugging Face Datasets Cache".to_string(),
            category: "AI Model Cache".to_string(),
            safety: Safety::Review,
            description: "Cache directory for Hugging Face datasets. Downloaded again when a script needs it.".to_string(),
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            group: "AI Tools",
            catch_all: false,
        });
    }

    let xet_dir = home.join(".cache/huggingface/xet");
    if super::is_real_dir(&xet_dir) {
        out.push(Candidate {
            path: xet_dir,
            file_type: "Hugging Face Xet Cache".to_string(),
            category: "AI Model Cache".to_string(),
            safety: Safety::Review,
            description: "Cache directory for Hugging Face Xet transfer acceleration. Rebuilt automatically when needed.".to_string(),
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            group: "AI Tools",
            catch_all: false,
        });
    }
}

fn collect_lm_studio(home: &Path, out: &mut Vec<Candidate>) {
    let roots = [
        home.join(".lmstudio/models"),
        home.join(".cache/lm-studio/models"),
    ];

    for root in roots {
        if !super::is_real_dir(&root) {
            continue;
        }
        let Ok(publishers) = fs::read_dir(&root) else {
            continue;
        };
        for pub_entry in publishers.flatten() {
            let pub_path = pub_entry.path();
            if !super::is_real_dir(&pub_path) {
                continue;
            }
            let pub_name = pub_entry.file_name().to_string_lossy().into_owned();
            if pub_name.starts_with('.') {
                continue;
            }

            let Ok(models) = fs::read_dir(&pub_path) else {
                continue;
            };
            for model_entry in models.flatten() {
                let model_path = model_entry.path();
                if !super::is_real_dir(&model_path) {
                    continue;
                }
                let model_name = model_entry.file_name().to_string_lossy().into_owned();
                if model_name.starts_with('.') {
                    continue;
                }

                let description = format!(
                    "{pub_name}/{model_name}: a model downloaded in LM Studio. Downloaded again from the catalog when you need it. Models you imported yourself cannot be downloaded again."
                );

                out.push(Candidate {
                    path: model_path,
                    file_type: "LM Studio Models".to_string(),
                    category: "AI Model Cache".to_string(),
                    safety: Safety::Review,
                    description,
                    blocked_if_running: vec!["LM Studio"],
                    project_root: None,
                    action: Action::DeletePath,
                    known_size: None,
                    min_bytes: 1,
                    group: "AI Tools",
                    catch_all: false,
                });
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct OllamaManifest {
    config: Option<OllamaManifestConfig>,
    #[serde(default)]
    layers: Vec<OllamaManifestLayer>,
}

#[derive(serde::Deserialize)]
struct OllamaManifestConfig {
    #[serde(default)]
    size: u64,
}

#[derive(serde::Deserialize)]
struct OllamaManifestLayer {
    #[serde(default)]
    size: u64,
}

fn collect_ollama(home: &Path, out: &mut Vec<Candidate>) {
    let Some(ollama) = native_commands::find_tool(home, "ollama") else {
        return;
    };
    let models_dir = home.join(".ollama/models");
    let manifests_root = models_dir.join("manifests");
    if !super::is_real_dir(&manifests_root) {
        return;
    }

    let Ok(hosts) = fs::read_dir(&manifests_root) else {
        return;
    };
    for host_entry in hosts.flatten() {
        let host_path = host_entry.path();
        if !super::is_real_dir(&host_path) {
            continue;
        }
        let host_name = host_entry.file_name().to_string_lossy().into_owned();
        if host_name.starts_with('.') {
            continue;
        }

        let Ok(namespaces) = fs::read_dir(&host_path) else {
            continue;
        };
        for ns_entry in namespaces.flatten() {
            let ns_path = ns_entry.path();
            if !super::is_real_dir(&ns_path) {
                continue;
            }
            let ns_name = ns_entry.file_name().to_string_lossy().into_owned();
            if ns_name.starts_with('.') {
                continue;
            }

            let Ok(models) = fs::read_dir(&ns_path) else {
                continue;
            };
            for model_entry in models.flatten() {
                let model_path = model_entry.path();
                if !super::is_real_dir(&model_path) {
                    continue;
                }
                let model_name = model_entry.file_name().to_string_lossy().into_owned();
                if model_name.starts_with('.') {
                    continue;
                }

                let Ok(tags) = fs::read_dir(&model_path) else {
                    continue;
                };
                for tag_entry in tags.flatten() {
                    let tag_path = tag_entry.path();
                    let Ok(meta) = fs::symlink_metadata(&tag_path) else {
                        continue;
                    };
                    if meta.file_type().is_symlink() || !meta.is_file() {
                        continue;
                    }
                    let tag_name = tag_entry.file_name().to_string_lossy().into_owned();
                    if tag_name.starts_with('.') {
                        continue;
                    }

                    let display_name = if host_name == "registry.ollama.ai" {
                        if ns_name == "library" {
                            format!("{model_name}:{tag_name}")
                        } else {
                            format!("{ns_name}/{model_name}:{tag_name}")
                        }
                    } else {
                        format!("{host_name}/{ns_name}/{model_name}:{tag_name}")
                    };

                    if !native_commands::valid_ollama_model(&display_name) {
                        continue;
                    }
                    if native_commands::manifest_path(&models_dir, &display_name).as_deref()
                        != Some(&tag_path)
                    {
                        continue;
                    }

                    let Ok(content) = fs::read_to_string(&tag_path) else {
                        continue;
                    };
                    let Ok(manifest) = serde_json::from_str::<OllamaManifest>(&content) else {
                        continue;
                    };

                    let mut total_size = manifest.config.map(|c| c.size).unwrap_or(0);
                    for layer in manifest.layers {
                        total_size = total_size.saturating_add(layer.size);
                    }

                    let description = format!(
                        "{display_name}: an Ollama model, removed with ollama rm. Downloaded again with ollama pull. Models you made yourself with ollama create cannot be downloaded again. The Ollama app must be running. The size includes layers shared with other models, so less space may be freed."
                    );

                    out.push(Candidate {
                        path: tag_path,
                        file_type: "Ollama Models".to_string(),
                        category: "AI Model Cache".to_string(),
                        safety: Safety::Review,
                        description,
                        blocked_if_running: Vec::new(),
                        project_root: None,
                        action: Action::Command(NativeCommand::OllamaRemove {
                            ollama: ollama.clone(),
                            models_dir: models_dir.clone(),
                            model: display_name,
                        }),
                        known_size: Some(total_size),
                        min_bytes: 1,
                        group: "AI Tools",
                        catch_all: false,
                    });
                }
            }
        }
    }
}
