use crate::config::Backend;
use anyhow::{Result, anyhow};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug)]
pub struct LLMClient {
    client: Client,
    backend: Backend,
}

#[derive(Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug)]
pub struct ToolResponse {
    pub tool_call_id: String,
    pub output: String,
}

impl LLMClient {
    pub fn new(backend: Backend) -> Self {
        let timeout = backend.timeout_secs.unwrap_or(30);
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout))
            .build()
            .unwrap();
        LLMClient { client, backend }
    }

    pub fn chat_completion(
        &self,
        system_prompt: &str,
        user_message: &str,
        tool_descriptions: &[Value],
    ) -> Result<(Option<String>, Option<Vec<ToolCall>>)> {
        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": user_message}),
        ];

        let mut request_body = json!({
            "model": self.backend.model,
            "messages": messages,
        });

        // Исправление: преобразуем Map в Value::Object
        if let Some(options) = &self.backend.options {
            request_body["options"] = Value::Object(options.clone());
        }

        if !tool_descriptions.is_empty() {
            request_body["tools"] = json!(tool_descriptions);
            request_body["tool_choice"] = json!("auto");
        }

        let mut req = self.client.post(&self.backend.api_url).json(&request_body);
        if let Some(key) = &self.backend.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(anyhow!("API error {}: {}", status, text));
        }

        let json: Value = resp.json()?;
        let choice = json["choices"][0]
            .as_object()
            .ok_or_else(|| anyhow!("Нет выбора в ответе"))?;

        let content = choice["message"]["content"].as_str().map(String::from);
        let tool_calls = if let Some(calls) = choice["message"]["tool_calls"].as_array() {
            let mut vec = Vec::new();
            for call in calls {
                if call["type"] == "function" {
                    let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                    let args_str = call["function"]["arguments"].as_str().unwrap_or("{}");
                    let arguments: Value = serde_json::from_str(args_str)?;
                    vec.push(ToolCall {
                        id: call["id"].as_str().unwrap_or("").to_string(),
                        name,
                        arguments,
                    });
                }
            }
            Some(vec)
        } else {
            None
        };

        Ok((content, tool_calls))
    }

    pub fn submit_tool_results(
        &self,
        _system_prompt: &str,
        original_messages: Vec<Value>,
        tool_responses: Vec<ToolResponse>,
    ) -> Result<Option<String>> {
        let mut messages = original_messages;
        for tr in tool_responses {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": tr.tool_call_id,
                "content": tr.output
            }));
        }

        let mut request_body = json!({
            "model": self.backend.model,
            "messages": messages,
        });

        // Исправление здесь тоже
        if let Some(options) = &self.backend.options {
            request_body["options"] = Value::Object(options.clone());
        }

        let mut req = self.client.post(&self.backend.api_url).json(&request_body);
        if let Some(key) = &self.backend.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send()?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(anyhow!("API error {}: {}", status, text));
        }

        let json: Value = resp.json()?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from);
        Ok(content)
    }
}