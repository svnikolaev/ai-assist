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
            params![],
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
            // Загружаем все записи
            let mut rows = conn.query(
                "SELECT id, content, metadata, embedding FROM memories",
                params![],
            )
            .await?;
            
            let mut results = Vec::new();
            while let Some(row) = rows.next().await? {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let metadata: Option<String> = row.get(2)?;
                let embedding_blob: Vec<u8> = row.get(3)?;
                
                // Преобразуем BLOB в Vec<f32>
                let embedding: Vec<f32> = bytemuck::cast_slice(&embedding_blob).to_vec();
                
                // Вычисляем косинусное расстояние (1 - косинусное сходство)
                let dot: f32 = query_embedding.iter().zip(&embedding).map(|(a, b)| a * b).sum();
                let norm_query: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                let norm_db: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                let distance = if norm_query == 0.0 || norm_db == 0.0 {
                    1.0
                } else {
                    1.0 - (dot / (norm_query * norm_db))
                };
                
                results.push(MemoryEntry {
                    id,
                    content,
                    metadata,
                    distance: distance as f64,
                });
            }
            
            // Сортируем по расстоянию (меньше = ближе)
            results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
            results.truncate(limit);
            
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

pub fn get() -> Option<&'static Memory> {
    MEMORY.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use anyhow::Result;

    const TEST_DIM: usize = 4; // маленькая размерность для простоты

    fn create_test_memory() -> Result<Memory> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");
        let path_str = db_path.to_str().unwrap();
        Memory::new(path_str, TEST_DIM)
    }

    #[test]
    fn test_new_creates_table() -> Result<()> {
        let memory = create_test_memory()?;
        // Проверим, что таблица создалась: попробуем вставить
        let id = "test1";
        let content = "content";
        let embedding = vec![0.1; TEST_DIM];
        memory.insert(id, content, embedding, None)?;
        let entry = memory.get(id)?.unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.content, content);
        Ok(())
    }

    #[test]
    fn test_insert_and_get() -> Result<()> {
        let memory = create_test_memory()?;
        let id = "key1";
        let content = "hello world";
        let embedding = vec![0.2, 0.4, 0.6, 0.8];
        let metadata = Some("test metadata");
        memory.insert(id, content, embedding, metadata)?;

        let entry = memory.get(id)?.expect("entry should exist");
        assert_eq!(entry.id, id);
        assert_eq!(entry.content, content);
        assert_eq!(entry.metadata, metadata.map(String::from));
        Ok(())
    }

    #[test]
    fn test_get_nonexistent() -> Result<()> {
        let memory = create_test_memory()?;
        let entry = memory.get("no-such-id")?;
        assert!(entry.is_none());
        Ok(())
    }

    #[test]
    fn test_insert_replace() -> Result<()> {
        let memory = create_test_memory()?;
        let id = "dup";
        let content1 = "first";
        let embed1 = vec![0.1; TEST_DIM];
        memory.insert(id, content1, embed1, None)?;

        let content2 = "second";
        let embed2 = vec![0.9; TEST_DIM];
        memory.insert(id, content2, embed2, Some("new"))?;

        let entry = memory.get(id)?.unwrap();
        assert_eq!(entry.content, content2);
        assert_eq!(entry.metadata, Some("new".to_string()));
        Ok(())
    }

    #[test]
    fn test_delete() -> Result<()> {
        let memory = create_test_memory()?;
        let id = "to-delete";
        memory.insert(id, "x", vec![0.0; TEST_DIM], None)?;
        memory.delete(id)?;
        let entry = memory.get(id)?;
        assert!(entry.is_none());
        Ok(())
    }

    #[test]
    fn test_list_all() -> Result<()> {
        let memory = create_test_memory()?;
        memory.insert("a", "A", vec![0.1; TEST_DIM], None)?;
        memory.insert("b", "B", vec![0.2; TEST_DIM], Some("b-metadata"))?;
        let list = memory.list_all()?;
        assert_eq!(list.len(), 2);
        let ids: Vec<_> = list.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
        Ok(())
    }

    #[test]
    fn test_search_similar() -> Result<()> {
        let memory = create_test_memory()?;
        // Вставим два вектора: близкий к (1,0,0,0) и далёкий
        let near_id = "near";
        let near_embed = vec![0.9, 0.1, 0.0, 0.0];
        memory.insert(near_id, "near", near_embed, None)?;

        let far_id = "far";
        let far_embed = vec![0.0, 0.9, 0.1, 0.0];
        memory.insert(far_id, "far", far_embed, None)?;

        // Запрос, близкий к near
        let query = vec![1.0, 0.0, 0.0, 0.0];
        let results = memory.search_similar(query, 2)?;
        assert_eq!(results.len(), 2);
        // Первый результат должен быть near (меньшее расстояние)
        assert_eq!(results[0].id, near_id);
        assert!(results[0].distance < results[1].distance);
        Ok(())
    }

    #[test]
    fn test_search_similar_limit() -> Result<()> {
        let memory = create_test_memory()?;
        for i in 0..5 {
            let id = format!("item{}", i);
            let embed = vec![i as f32 / 10.0; TEST_DIM];
            memory.insert(&id, "x", embed, None)?;
        }
        let query = vec![0.0; TEST_DIM];
        let results = memory.search_similar(query, 3)?;
        assert_eq!(results.len(), 3);
        Ok(())
    }

    #[test]
    fn test_search_similar_empty() -> Result<()> {
        let memory = create_test_memory()?;
        let query = vec![1.0; TEST_DIM];
        let results = memory.search_similar(query, 5)?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn test_insert_with_zero_vector() -> Result<()> {
        let memory = create_test_memory()?;
        let id = "zero";
        let embed = vec![0.0; TEST_DIM];
        memory.insert(id, "zero content", embed, None)?;
        let entry = memory.get(id)?.unwrap();
        assert_eq!(entry.content, "zero content");
        Ok(())
    }
}