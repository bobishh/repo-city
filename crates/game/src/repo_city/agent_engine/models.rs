//! Provider-owned model catalogs. Codex pagination follows Ecky df36c9345183;
//! local adaptation retains labels and normalizes ACP session configuration too.
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Model {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Catalog {
    pub choices: Vec<Model>,
    pub current: Option<String>,
}
impl Catalog {
    pub fn contains(&self, id: &str) -> bool {
        self.choices.iter().any(|model| model.id == id)
    }
    fn normalized(mut self) -> Self {
        let mut seen = HashSet::new();
        self.choices
            .retain(|model| !model.id.trim().is_empty() && seen.insert(model.id.clone()));
        self
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexModel {
    model: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    is_default: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexPage {
    data: Vec<CodexModel>,
    next_cursor: Option<String>,
}
pub(super) fn codex_page(value: Value) -> Result<(Catalog, Option<String>), String> {
    let page: CodexPage =
        serde_json::from_value(value).map_err(|e| format!("Invalid provider model list: {e}"))?;
    let mut catalog = Catalog::default();
    for model in page.data.into_iter().filter(|model| !model.hidden) {
        if model.is_default {
            catalog.current = Some(model.model.clone());
        }
        catalog.choices.push(Model {
            name: if model.display_name.is_empty() {
                model.model.clone()
            } else {
                model.display_name
            },
            id: model.model,
        });
    }
    Ok((
        catalog.normalized(),
        page.next_cursor.filter(|cursor| !cursor.is_empty()),
    ))
}

#[derive(Clone, Debug)]
pub(super) struct AcpCatalog {
    pub catalog: Catalog,
    pub config_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpModels {
    current_model_id: String,
    available_models: Vec<AcpModel>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpModel {
    model_id: String,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    current_value: String,
    options: Vec<ConfigOption>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ConfigOption {
    Group { options: Vec<ConfigOption> },
    Choice { value: String, name: String },
}
fn options(items: Vec<ConfigOption>, result: &mut Vec<Model>) {
    for item in items {
        match item {
            ConfigOption::Group { options: group } => options(group, result),
            ConfigOption::Choice { value, name } => result.push(Model { id: value, name }),
        }
    }
}
pub(super) fn acp(value: &Value) -> Result<Option<AcpCatalog>, String> {
    if let Some(configs) = value.get("configOptions") {
        let configs = configs
            .as_array()
            .ok_or("Invalid provider model configuration: expected an array.")?;
        if let Some(config) = configs
            .iter()
            .find(|c| c["category"] == "model" || c["id"] == "model")
        {
            let config: Config = serde_json::from_value(config.clone())
                .map_err(|e| format!("Invalid provider model configuration: {e}"))?;
            if config.kind != "select" {
                return Err("Provider model configuration is not selectable.".into());
            }
            let mut choices = Vec::new();
            options(config.options, &mut choices);
            return Ok(Some(AcpCatalog {
                catalog: Catalog {
                    choices,
                    current: (!config.current_value.is_empty()).then_some(config.current_value),
                }
                .normalized(),
                config_id: Some(config.id),
            }));
        }
    }
    if let Some(models) = value.get("models") {
        let models: AcpModels = serde_json::from_value(models.clone())
            .map_err(|e| format!("Invalid ACP model list: {e}"))?;
        return Ok(Some(AcpCatalog {
            catalog: Catalog {
                current: Some(models.current_model_id),
                choices: models
                    .available_models
                    .into_iter()
                    .map(|m| Model {
                        id: m.model_id,
                        name: m.name,
                    })
                    .collect(),
            }
            .normalized(),
            config_id: None,
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn catalogs_keep_provider_ids_labels_and_reject_broken_contracts() {
        let (page, next) = codex_page(json!({"data":[{"model":"a","displayName":"A","isDefault":true},{"model":"hidden","hidden":true},{"model":"a"}],"nextCursor":"page2"})).unwrap();
        assert_eq!(
            page.choices,
            vec![Model {
                id: "a".into(),
                name: "A".into()
            }]
        );
        assert_eq!(page.current.as_deref(), Some("a"));
        assert_eq!(next.as_deref(), Some("page2"));
        assert!(codex_page(json!({})).is_err());
        assert!(acp(&json!({"models":{"currentModelId":"x"}})).is_err());
    }
    #[test]
    fn acp_prefers_model_configuration_and_flattens_grouped_choices() {
        let catalog = acp(&json!({"configOptions":[{"id":"other","category":"model","type":"select","currentValue":"b","options":[{"name":"Group","options":[{"value":"a","name":"A"},{"value":"b","name":"B"}]}]}],"models":{"currentModelId":"old","availableModels":[]}})).unwrap().unwrap();
        assert_eq!(catalog.config_id.as_deref(), Some("other"));
        assert_eq!(catalog.catalog.current.as_deref(), Some("b"));
        assert!(catalog.catalog.contains("a") && catalog.catalog.contains("b"));
        assert!(acp(&json!({})).unwrap().is_none());
    }
    #[test]
    fn unrelated_configuration_does_not_break_the_model_contract() {
        let value = json!({"configOptions":[
            {"id":"enabled","type":"boolean","currentValue":true},
            {"id":"model","category":"model","type":"select","currentValue":"chosen","options":[{"value":"chosen","name":"Chosen"}]}
        ]});
        assert_eq!(
            acp(&value).unwrap().unwrap().catalog.current.as_deref(),
            Some("chosen")
        );
    }
}
