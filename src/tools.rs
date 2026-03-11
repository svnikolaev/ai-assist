use crate::config::{Config, ToolExecution};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::process::Command;

pub struct ToolOutput {
    pub content: String,
    pub details: Option<String>,
}


type BuiltinFn = fn(&Value, &Config, Option<&serde_json::Map<String, Value>>) -> Result<ToolOutput>;

struct BuiltinTool {
    name: &'static str,
    description: &'static str,
    schema: Value,
    func: BuiltinFn,
}

struct ExternalTool {
    description: String,
    schema: Value,
    execution: ToolExecution,
}

pub struct ToolRegistry {
    builtins: HashMap<String, BuiltinTool>,
    externals: HashMap<String, ExternalTool>,
}

impl ToolRegistry {
    pub fn new(config: &Config) -> Self {
        let mut builtins = HashMap::new();

        let builtins_list = vec![
            BuiltinTool {
                name: "calculator",
                description: "Выполняет математические вычисления. Используй для арифметики.",
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "expression": {
                            "type": "string",
                            "description": "Выражение для вычисления (например, '2+2*3')"
                        }
                    },
                    "required": ["expression"]
                }),
                func: calculator_tool,
            },
            BuiltinTool {
                name: "run_shell",
                description:
                    "Выполняет команду в оболочке (осторожно!). Принимает команду строкой.",
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Команда для выполнения"
                        }
                    },
                    "required": ["command"]
                }),
                func: run_shell_tool,
            },
            BuiltinTool {
                name: "current_time",
                description: "Возвращает текущие дату и время.",
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                func: current_time_tool,
            },
        ];

        for tool in builtins_list {
            builtins.insert(tool.name.to_string(), tool);
        }

        let mut externals = HashMap::new();
        if let Some(tools) = &config.tools {
            for tool in tools {
                if let Some(execution) = &tool.execution {
                    if builtins.contains_key(&tool.name) {
                        eprintln!(
                            "Предупреждение: внешний инструмент '{}' конфликтует со встроенным и будет проигнорирован.",
                            tool.name
                        );
                        continue;
                    }
                    externals.insert(
                        tool.name.clone(),
                        ExternalTool {
                            description: tool.description.clone(),
                            schema: tool.schema.clone(),
                            execution: execution.clone(),
                        },
                    );
                }
            }
        }

        ToolRegistry {
            builtins,
            externals,
        }
    }

    pub fn tool_descriptions(&self) -> Vec<Value> {
        let mut descriptions = Vec::new();
        for tool in self.builtins.values() {
            descriptions.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.schema,
                }
            }));
        }
        for (name, tool) in &self.externals {
            descriptions.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": tool.description,
                    "parameters": tool.schema,
                }
            }));
        }
        descriptions
    }

    // Добавляем параметр backend_options
    pub fn execute(
        &self,
        name: &str,
        args: &Value,
        config: &Config,
        backend_options: Option<&serde_json::Map<String, Value>>,
    ) -> Result<ToolOutput> {
        if let Some(builtin) = self.builtins.get(name) {
            (builtin.func)(args, config, backend_options)
        } else if let Some(external) = self.externals.get(name) {
            // Внешние инструменты пока не используют опции бэкенда, но можно передать, если нужно
            execute_external(&external.execution, args)
        } else {
            Err(anyhow!("Инструмент '{}' не найден", name))
        }
    }
}

fn execute_external(execution: &ToolExecution, args: &Value) -> Result<ToolOutput> {
    match execution {
        ToolExecution::Command { command } => {
            let mut cmd_str = command.clone();
            if let Some(args_obj) = args.as_object() {
                for (key, val) in args_obj {
                    let placeholder = format!("{{{}}}", key);
                    let val_str = val.as_str().unwrap_or(&val.to_string()).to_string();
                    cmd_str = cmd_str.replace(&placeholder, &val_str);
                }
            }
            let output = Command::new("sh").arg("-c").arg(&cmd_str).output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if !stderr.is_empty() {
                eprintln!("[external tool stderr]: {}", stderr);
            }
            Ok(ToolOutput {
                content: stdout,
                details: Some(format!("Команда выполнена, код выхода: {}", output.status)),
            })
        }
        ToolExecution::ApiCall {
            url,
            method: _,
            headers: _,
            params: _,
        } => {
            let client = reqwest::blocking::Client::new();
            let resp = client.get(url).send()?;
            let text = resp.text()?;
            Ok(ToolOutput {
                content: text,
                details: None,
            })
        }
        ToolExecution::Script { path, interpreter } => {
            let interpreter = interpreter.as_deref().unwrap_or("sh");
            let output = Command::new(interpreter).arg(path).output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if !stderr.is_empty() {
                eprintln!("[external script stderr]: {}", stderr);
            }
            Ok(ToolOutput {
                content: stdout,
                details: Some(format!("Скрипт выполнен, код выхода: {}", output.status)),
            })
        }
    }
}

// ---------- Встроенные инструменты ----------

fn calculator_tool(
    args: &Value,
    _config: &Config,
    _backend_options: Option<&serde_json::Map<String, Value>>,
) -> Result<ToolOutput> {
    let expression = args
        .get("expression")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Не указано выражение (поле 'expression')"))?;

    let result = meval::eval_str(expression).map_err(|e| anyhow!("Ошибка в выражении: {}", e))?;

    Ok(ToolOutput {
        content: result.to_string(),
        details: Some(format!("Вычислено: {} = {}", expression, result)),
    })
}

fn run_shell_tool(
    args: &Value,
    config: &Config,
    backend_options: Option<&serde_json::Map<String, Value>>,
) -> Result<ToolOutput> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Не указана команда (поле 'command')"))?;

    // Проверка по стоп-листу
    if let Some(stop_list) = &config.stop_list {
        for pattern in stop_list {
            if command.contains(pattern) {
                return Err(anyhow!(
                    "Команда запрещена стоп-листом (содержит '{}')",
                    pattern
                ));
            }
        }
    }

    // Определяем, нужно ли подтверждение (по умолчанию true)
    let confirm_shell = backend_options
        .and_then(|opts| opts.get("confirm_shell"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if confirm_shell {
        eprintln!("\n⚠️  Команда для выполнения: {}", command);
        eprint!("Подтвердите выполнение [y/N]: ");
        std::io::stdout().flush()?; // обязательно flush, чтобы приглашение появилось

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Err(anyhow!("Выполнение отменено пользователем"));
        }
    }

    // Выполняем команду
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|e| anyhow!("Ошибка выполнения команды: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.is_empty() {
        eprintln!("[run_shell stderr]: {}", stderr);
    }

    Ok(ToolOutput {
        content: stdout,
        details: Some(format!("Команда выполнена, код выхода: {}", output.status)),
    })
}

fn current_time_tool(
    _args: &Value,
    _config: &Config,
    _backend_options: Option<&serde_json::Map<String, Value>>,
) -> Result<ToolOutput> {
    let now = chrono::Local::now();
    Ok(ToolOutput {
        content: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        details: None,
    })
}
