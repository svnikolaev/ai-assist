mod config;
mod llm;
mod tools;

use crate::config::Config;
use crate::llm::{LLMClient, ToolResponse};
use crate::tools::ToolRegistry;
use anyhow::{Result, anyhow};
use clap::Parser;
use std::io::{self, Read};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Запрос на естественном языке (можно передать несколько слов без кавычек)
    #[arg(required = false)]
    query: Vec<String>,

    /// Показать объяснение (расширенный вывод)
    #[arg(short, long)]
    explain: bool,

    /// Системный промпт (переопределяет default_prompt из конфига)
    #[arg(short, long)]
    prompt: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load()?;

    let system_prompt = args
        .prompt
        .or(config.default_prompt.clone())
        .unwrap_or_else(|| "Ты полезный ассистент.".to_string());

    // Формируем запрос: объединяем все слова, если они есть, иначе читаем из stdin
    let user_message = if !args.query.is_empty() {
        args.query.join(" ")
    } else {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        if buffer.is_empty() {
            eprintln!(
                "Ошибка: не указан запрос. Используйте: ask вопрос без кавычек, ask \"вопрос в кавычках\", или передайте текст через pipe."
            );
            std::process::exit(1);
        }
        buffer.trim().to_string()
    };

    let backend = config
        .backends
        .first()
        .ok_or_else(|| anyhow!("Нет доступных бэкендов"))?;
    let client = LLMClient::new(backend.clone());

    let tool_registry = ToolRegistry::new(&config);
    let tool_descriptions = tool_registry.tool_descriptions();

    let (mut content, tool_calls) =
        client.chat_completion(&system_prompt, &user_message, &tool_descriptions)?;

    if let Some(calls) = tool_calls {
        let messages = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_message}),
            serde_json::json!({
                "role": "assistant",
                "content": content,
                "tool_calls": calls.iter().map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "type": "function",
                        "function": {
                            "name": c.name,
                            "arguments": c.arguments.to_string()
                        }
                    })
                }).collect::<Vec<_>>()
            }),
        ];

        let mut tool_responses = Vec::new();
        for call in calls {
            let output = tool_registry.execute(
                &call.name,
                &call.arguments,
                &config,
                backend.options.as_ref(),
            )?;
            tool_responses.push(ToolResponse {
                tool_call_id: call.id,
                output: output.content.clone(),
            });

            if args.explain {
                if let Some(details) = output.details {
                    eprintln!("🔧 [{}]: {}", call.name, details);
                }
            }
        }

        let final_answer = client.submit_tool_results(&system_prompt, messages, tool_responses)?;
        content = final_answer;
    }

    if let Some(answer) = content {
        if args.explain {
            println!("🤖 Ответ:\n{}", answer);
        } else {
            println!("{}", answer);
        }
    } else {
        eprintln!("Модель не вернула ответ.");
    }

    Ok(())
}