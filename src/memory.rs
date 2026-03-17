use anyhow::{Result, anyhow};
use bytemuck;
use once_cell::sync::{Lazy, OnceCell};
use tokio::runtime::Runtime;
use turso::{Builder, Database, params};

static RT: Lazy<Runtime> = Lazy::new(|| Runtime::new().expect("Failed to create runtime"));
static MEMORY: OnceCell<Memory> = OnceCell::new();

pub struct Memory {
    db: Database,
    pub embedding_dim: usize,
}

impl Memory {
    async fn new_async(path: &str, embedding_dim: usize) -> Result<Self> {
        let db = Builder::new_local(path).build().await?;
        let conn = db.connect()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                metadata TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            params![], // исправлено: используем макрос params!
        )
        .await?;
        Ok(Memory { db, embedding_dim })
    }

    pub fn new(path: &str, embedding_dim: usize) -> Result<Self> {
        RT.block_on(Self::new_async(path, embedding_dim))
    }

    pub fn insert(&self, id: &str, content: &str, embedding: Vec<f32>, metadata: Option<&str>) -> Result<()> {
        RT.block_on(async {
            let conn = self.db.connect()?;
            let embedding_blob = bytemuck::cast_slice(&embedding).to_vec();
            conn.execute(
                "INSERT OR REPLACE INTO memories (id, content, embedding, metadata) VALUES (?1, ?2, ?3, ?4)",
                params![id, content, embedding_blob, metadata],
            )
            .await?;
            Ok(())
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<MemoryEntry>> {
        RT.block_on(async {
            let conn = self.db.connect()?;
            let mut rows = conn.query(
                "SELECT id, content, metadata FROM memories WHERE id = ?1",
                params![id],
            )
            .await?;
            if let Some(row) = rows.next().await? {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let metadata: Option<String> = row.get(2)?;
                Ok(Some(MemoryEntry {
                    id,
                    content,
                    metadata,
                    distance: 0.0,
                }))
            } else {
                Ok(None)
            }
        })
    }

    pub fn search_similar(&self, query_embedding: Vec<f32>, limit: usize) -> Result<Vec<MemoryEntry>> {
        RT.block_on(async {
            let conn = self.db.connect()?;
            let query_blob = bytemuck::cast_slice(&query_embedding).to_vec();
            // Внимание: функция vec_distance_cosine должна быть предоставлена Turso.
            // Если её нет, замените на ручное вычисление после загрузки всех векторов.
            let mut rows = conn.query(
                "SELECT id, content, metadata, vec_distance_cosine(embedding, ?1) as distance
                 FROM memories
                 ORDER BY distance
                 LIMIT ?2",
                params![query_blob, limit as i64],
            )
            .await?;
            let mut results = Vec::new();
            while let Some(row) = rows.next().await? {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let metadata: Option<String> = row.get(2)?;
                let distance: f64 = row.get(3)?;
                results.push(MemoryEntry {
                    id,
                    content,
                    metadata,
                    distance,
                });
            }
            Ok(results)
        })
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        RT.block_on(async {
            let conn = self.db.connect()?;
            conn.execute("DELETE FROM memories WHERE id = ?1", params![id]).await?;
            Ok(())
        })
    }

    pub fn list_all(&self) -> Result<Vec<MemoryEntry>> {
        RT.block_on(async {
            let conn = self.db.connect()?;
            let mut rows = conn.query("SELECT id, content, metadata FROM memories", params![]).await?;
            let mut results = Vec::new();
            while let Some(row) = rows.next().await? {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let metadata: Option<String> = row.get(2)?;
                results.push(MemoryEntry {
                    id,
                    content,
                    metadata,
                    distance: 0.0,
                });
            }
            Ok(results)
        })
    }
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub metadata: Option<String>,
    pub distance: f64,
}

/// Инициализирует глобальную память из конфигурации.
/// Должна вызываться один раз при старте.
pub fn init(config: Option<&crate::config::MemoryConfig>) -> Result<()> {
    if let Some(cfg) = config {
        let db_path = cfg
            .db_path
            .clone()
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap()
                    .join("ai-assist/memory.db")
                    .to_str()
                    .unwrap()
                    .to_string()
            });
        let dim = cfg.embedding_dim.unwrap_or(384);
        let memory = Memory::new(&db_path, dim)?;
        MEMORY.set(memory).map_err(|_| anyhow!("Memory already initialized"))?;
    }
    Ok(())
}

/// Возвращает ссылку на глобальную память, если она инициализирована.
pub fn get() -> Option<&'static Memory> {
    MEMORY.get()
}