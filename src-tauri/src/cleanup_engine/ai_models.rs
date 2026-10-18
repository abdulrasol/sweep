//! AI models and datasets cleanup (PLAN 2.14a).
//!
//! Offers fine-grained (per-model) cleanup for Hugging Face and LM Studio,
//! while keeping login tokens, conversation history and settings strictly protected.

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
