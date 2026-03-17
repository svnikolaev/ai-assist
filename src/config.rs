use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Backend {
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
    pub options: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolExecution {
    Command {
        command: String,
    },
    ApiCall {
        url: String,
        method: Option<String>,
        headers: Option<HashMap<String, String>>,
        params: Option<serde_json::Value>,
    },
    Script {
        path: String,
        interpreter: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ToolExecution>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MemoryConfig {
    pub db_path: Option<String>,
    pub embedding_dim: Option<usize>,
    pub obsidian_vault: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub default_prompt: Option<String>,
    pub explain_language: Option<String>,
    pub stop_list: Option<Vec<String>>,
    pub backends: Vec<Backend>,
    pub tools: Option<Vec<Tool>>,
    pub memory: Option<MemoryConfig>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ai-assist")
            .join("config.toml");

        if !config_path.exists() {
            // Создаём конфиг по умолчанию
            let mut default_options = serde_json::Map::new();
            default_options.insert("nothink".to_string(), serde_json::Value::Bool(true));
            default_options.insert("confirm_shell".to_string(), serde_json::Value::Bool(true));

            let default = Config {
                default_prompt: Some("Ты — полезный ассистент. Отвечай кратко и по делу.".into()),
                explain_language: Some("ru".into()),
                stop_list: Some(vec!["rm -rf /".into(), "mkfs".into()]),
                backends: vec![Backend {
                    api_url: "http://localhost:11434/v1/chat/completions".into(),
                    api_key: None,
                    model: "qwen3.5:0.8b".into(),
                    timeout_secs: Some(60),
                    options: Some(default_options),
                }],
                tools: Some(vec![]),
                memory: Some(MemoryConfig {
                    db_path: Some(
                        dirs::config_dir()
                            .unwrap()
                            .join("ai-assist/memory.db")
                            .to_str()
                            .unwrap()
                            .to_string(),
                    ),
                    embedding_dim: Some(384),
                    obsidian_vault: None,
                }),
            };
            let toml_string = toml::to_string_pretty(&default)?;
            fs::create_dir_all(config_path.parent().unwrap())?;
            fs::write(&config_path, toml_string)?;
            eprintln!("Создан конфиг по умолчанию: {:?}", config_path);
            Ok(default)
        } else {
            let contents = fs::read_to_string(config_path)?;
            let config: Config = toml::from_str(&contents)?;
            Ok(config)
        }
    }
}